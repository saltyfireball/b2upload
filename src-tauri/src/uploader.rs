use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use std::sync::Mutex;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub struct S3ClientCache {
    client: Mutex<Option<(String, S3Client)>>, // (cache_key, client)
}

impl S3ClientCache {
    pub fn new() -> Self {
        Self { client: Mutex::new(None) }
    }

    fn get_or_build(&self, endpoint: &str, key_id: &str, secret_key: &str) -> S3Client {
        let cache_key = format!("{}:{}:{}", endpoint, key_id, secret_key);
        let mut guard = self.client.lock().unwrap();
        if let Some((ref k, ref c)) = *guard {
            if *k == cache_key {
                return c.clone();
            }
        }
        let region = parse_region(endpoint);
        let creds = Credentials::new(key_id, secret_key, None, None, "b2upload");
        let config = S3ConfigBuilder::new()
            .endpoint_url(format!("https://{}", endpoint))
            .region(Region::new(region))
            .credentials_provider(creds)
            .force_path_style(true)
            .build();
        let client = S3Client::from_conf(config);
        *guard = Some((cache_key, client.clone()));
        client
    }
}

fn generate_hmac_token(path: &str, expires: u64, secret: &str) -> String {
    let message = format!("{}:{}", path, expires);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    URL_SAFE_NO_PAD.encode(result.into_bytes())
}

fn parse_region(endpoint: &str) -> String {
    // Extract region from "s3.REGION.backblazeb2.com"
    endpoint
        .split('.')
        .nth(1)
        .filter(|_| endpoint.starts_with("s3."))
        .unwrap_or("us-east-005")
        .to_string()
}

pub async fn upload_file(
    file_path: &str,
    mode: &str,
    settings: &HashMap<String, String>,
    ttl: Option<u64>,
    cache: &S3ClientCache,
) -> Result<String, String> {
    let input_path = Path::new(file_path);
    if !input_path.exists() {
        return Err(format!("File not found: {}", file_path));
    }
    if input_path.is_dir() {
        return Err("Directories are not supported. Drop individual files instead.".to_string());
    }

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

    let client = cache.get_or_build(endpoint, key_id, secret_key);

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
        match client.head_object().bucket(bucket).key(&object_key).send().await {
            Ok(_) => return Err("File already exists (overwrite is disabled)".to_string()),
            Err(e) => {
                let is_not_found = e.as_service_error()
                    .map(|se| se.is_not_found())
                    .unwrap_or(false);
                if !is_not_found {
                    return Err(format!("Failed to check existing file: {}", e));
                }
            }
        }
    }

    let body = ByteStream::from_path(path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    client
        .put_object()
        .bucket(bucket)
        .key(&object_key)
        .content_type(content_type)
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

pub async fn test_connection(
    settings: &HashMap<String, String>,
    cache: &S3ClientCache,
) -> Result<String, String> {
    let endpoint = settings.get("S3_ENDPOINT").ok_or("Missing S3_ENDPOINT")?;
    let key_id = settings
        .get("B2_APPLICATION_KEY_ID")
        .ok_or("Missing B2_APPLICATION_KEY_ID")?;
    let secret_key = settings
        .get("B2_APPLICATION_KEY")
        .ok_or("Missing B2_APPLICATION_KEY")?;
    let bucket = settings.get("BUCKET_NAME").ok_or("Missing BUCKET_NAME")?;

    let client = cache.get_or_build(endpoint, key_id, secret_key);

    client
        .head_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    Ok("Connection successful".to_string())
}
