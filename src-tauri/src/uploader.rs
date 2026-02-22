use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::storage::B2Credentials;

type HmacSha256 = Hmac<Sha256>;

// Encode everything except unreserved chars and forward slash
const PATH_SEGMENT_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Build a fresh S3 client with secure credential handoff.
/// The Zeroizing wrappers wipe the credential copies immediately after
/// the AWS SDK copies them into its internal Arc buffer.
fn build_client(endpoint: &str, creds: &B2Credentials) -> S3Client {
    let region = parse_region(endpoint);

    // Wrap in Zeroizing so originals are wiped after handoff to Credentials::new()
    let key_id = Zeroizing::new(creds.key_id.clone());
    let secret_key = Zeroizing::new(creds.app_key.clone());

    let aws_creds = Credentials::new(
        key_id.as_str(),
        secret_key.as_str(),
        None,
        None,
        "b2upload",
    );
    // key_id and secret_key Zeroizing wrappers drop here, wiping the cloned strings

    let config = S3ConfigBuilder::new()
        .endpoint_url(format!("https://{}", endpoint))
        .region(Region::new(region))
        .credentials_provider(aws_creds)
        .force_path_style(true)
        .build();

    S3Client::from_conf(config)
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

/// Percent-encode each segment of an object key, preserving `/` separators.
fn encode_object_key(object_key: &str) -> String {
    object_key
        .split('/')
        .map(|segment| utf8_percent_encode(segment, PATH_SEGMENT_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub async fn upload_file(
    file_path: &str,
    mode: &str,
    config: &HashMap<String, String>,
    creds: &B2Credentials,
    ttl: Option<u64>,
) -> Result<String, String> {
    let input_path = Path::new(file_path);
    if !input_path.exists() {
        return Err(format!("File not found: {}", file_path));
    }
    if input_path.is_dir() {
        return Err("Directories are not supported. Drop individual files instead.".to_string());
    }

    let endpoint = config.get("S3_ENDPOINT").ok_or("Missing S3_ENDPOINT")?;
    let bucket = config.get("BUCKET_NAME").ok_or("Missing BUCKET_NAME")?;
    let domain = config.get("DOMAIN").ok_or("Missing DOMAIN")?;

    // Map mode to folder/token
    let (folder, token) = if mode == "folder2" {
        (
            config.get("FOLDER_2").map(|s| s.as_str()).unwrap_or(""),
            &creds.folder_2_token,
        )
    } else {
        (
            config.get("FOLDER_1").map(|s| s.as_str()).unwrap_or(""),
            &creds.folder_1_token,
        )
    };

    // Read upload options
    let use_date = config.get("DATE_FOLDERS").map(|s| s.as_str()).unwrap_or("on") != "off";
    let use_uuid = config.get("UUID_FILENAMES").map(|s| s.as_str()).unwrap_or("on") != "off";
    let allow_overwrite = config.get("OVERWRITE_UPLOADS").map(|s| s.as_str()).unwrap_or("no") == "yes";

    // Build a fresh client for this operation; drops when function returns
    let client = build_client(endpoint, creds);

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

    // Percent-encode the object key for the URL
    let encoded_key = encode_object_key(&object_key);

    // Build URL with optional token
    let token_mode = config.get("TOKEN_MODE").map(|s| s.as_str()).unwrap_or("static");

    let url = if token_mode == "dynamic" {
        if let Some(ttl_secs) = ttl {
            if creds.token_secret.is_empty() {
                return Err("TOKEN_SECRET is required for dynamic token mode".to_string());
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            let expires = now + ttl_secs;
            let hmac_path = format!("/{}", object_key);
            let sig = generate_hmac_token(&hmac_path, expires, &creds.token_secret);
            format!("https://{}/{}?token={}&expires={}", domain, encoded_key, sig, expires)
        } else {
            format!("https://{}/{}", domain, encoded_key)
        }
    } else if token.is_empty() {
        format!("https://{}/{}", domain, encoded_key)
    } else {
        format!("https://{}/{}?token={}", domain, encoded_key, token)
    };

    // client drops here -- AWS SDK zeroizes its internal credential buffers
    Ok(url)
}

pub async fn test_connection(
    config: &HashMap<String, String>,
    creds: &B2Credentials,
) -> Result<String, String> {
    let endpoint = config.get("S3_ENDPOINT").ok_or("Missing S3_ENDPOINT")?;
    let bucket = config.get("BUCKET_NAME").ok_or("Missing BUCKET_NAME")?;

    // Build a fresh client for this operation; drops when function returns
    let client = build_client(endpoint, creds);

    client
        .head_bucket()
        .bucket(bucket)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    // client drops here -- AWS SDK zeroizes its internal credential buffers
    Ok("Connection successful".to_string())
}
