use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

fn generate_hmac_token(path: &str, expires: u64, secret: &str) -> String {
    let message = format!("{}:{}", path, expires);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    URL_SAFE_NO_PAD.encode(result.into_bytes())
}

fn parse_region(endpoint: &str) -> String {
    // e.g. s3.us-east-005.backblazeb2.com → us-east-005
    let re = regex_lite(endpoint);
    re.unwrap_or_else(|| "us-east-005".to_string())
}

fn regex_lite(endpoint: &str) -> Option<String> {
    // s3.REGION.backblazeb2.com
    let parts: Vec<&str> = endpoint.split('.').collect();
    if parts.len() >= 3 && parts[0] == "s3" {
        Some(parts[1].to_string())
    } else {
        None
    }
}

pub async fn upload_file(
    file_path: &str,
    mode: &str,
    settings: &HashMap<String, String>,
    ttl: Option<u64>,
) -> Result<String, String> {
    let endpoint = settings.get("S3_ENDPOINT").ok_or("Missing S3_ENDPOINT")?;
    let key_id = settings
        .get("B2_APPLICATION_KEY_ID")
        .ok_or("Missing B2_APPLICATION_KEY_ID")?;
    let secret_key = settings
        .get("B2_APPLICATION_KEY")
        .ok_or("Missing B2_APPLICATION_KEY")?;
    let bucket = settings.get("BUCKET_NAME").ok_or("Missing BUCKET_NAME")?;
    let domain = settings.get("DOMAIN").ok_or("Missing DOMAIN")?;

    // Map mode to folder/token settings
    let (folder_key, token_key) = if mode == "folder2" {
        ("FOLDER_2", "FOLDER_2_TOKEN")
    } else {
        ("FOLDER_1", "FOLDER_1_TOKEN")
    };
    let folder = settings.get(folder_key).map(|s| s.as_str()).unwrap_or("");
    let token = settings.get(token_key).map(|s| s.as_str()).unwrap_or("");

    // Read upload options
    let use_date = settings.get("DATE_FOLDERS").map(|s| s.as_str()).unwrap_or("on") != "off";
    let use_uuid = settings.get("UUID_FILENAMES").map(|s| s.as_str()).unwrap_or("on") != "off";
    let allow_overwrite = settings.get("OVERWRITE_UPLOADS").map(|s| s.as_str()).unwrap_or("no") == "yes";

    let region = parse_region(endpoint);

    let creds = Credentials::new(key_id, secret_key, None, None, "b2upload");

    let config = S3ConfigBuilder::new()
        .endpoint_url(format!("https://{}", endpoint))
        .region(Region::new(region))
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    let client = S3Client::from_conf(config);

    let path = Path::new(file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");

    // Build filename
    let filename = if use_uuid {
        format!("{}.{}", Uuid::new_v4(), ext)
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&format!("file.{}", ext))
            .to_string()
    };

    // Build object key: [folder/][date/]filename
    let mut parts: Vec<String> = Vec::new();
    if !folder.is_empty() {
        parts.push(folder.to_string());
    }
    if use_date {
        let now = chrono::Local::now();
        parts.push(now.format("%Y/%m/%d").to_string());
    }
    parts.push(filename);
    let object_key = parts.join("/");

    // Overwrite guard: only check when overwrite is off AND uuid is off (original filenames)
    if !allow_overwrite && !use_uuid {
        let head_result = client
            .head_object()
            .bucket(bucket)
            .key(&object_key)
            .send()
            .await;
        if head_result.is_ok() {
            return Err("File already exists (overwrite is disabled)".to_string());
        }
    }

    let body = ByteStream::from_path(path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    client
        .put_object()
        .bucket(bucket)
        .key(&object_key)
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {}", e))?;

    // Build URL with optional token
    let token_mode = settings.get("TOKEN_MODE").map(|s| s.as_str()).unwrap_or("static");

    let url = if token_mode == "dynamic" {
        if let Some(ttl_secs) = ttl {
            let secret = settings.get("TOKEN_SECRET").map(|s| s.as_str()).unwrap_or("");
            if secret.is_empty() {
                return Err("TOKEN_SECRET is required for dynamic token mode".to_string());
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            let expires = now + ttl_secs;
            let path = format!("/{}", object_key);
            let sig = generate_hmac_token(&path, expires, secret);
            format!("https://{}/{}?token={}&expires={}", domain, object_key, sig, expires)
        } else {
            format!("https://{}/{}", domain, object_key)
        }
    } else if token.is_empty() {
        format!("https://{}/{}", domain, object_key)
    } else {
        format!("https://{}/{}?token={}", domain, object_key, token)
    };

    Ok(url)
}
