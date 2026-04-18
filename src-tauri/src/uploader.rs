use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::primitives::{ByteStream, Length};
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client as S3Client;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

use tokio::io::AsyncWriteExt;

/// Progress callback: (bytes_done, bytes_total).
/// Called from any tokio task so must be Send + Sync.
pub type ProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

use crate::storage::B2Credentials;

type HmacSha256 = Hmac<Sha256>;

// Files larger than this use multipart upload. B2 single-PUT max is 5 GiB;
// multipart also unlocks incremental progress reporting.
const MULTIPART_THRESHOLD: u64 = 16 * 1024 * 1024; // 16 MiB
const PART_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB per part (min 5 MiB for S3)

/// Walk the std::error::Error source chain and join messages.
/// AWS SDK errors wrap the useful details several layers deep, so the top-level
/// Display is often just "service error" or "dispatch failure".
fn format_error_chain<E: std::error::Error + 'static>(err: &E) -> String {
    let mut parts: Vec<String> = vec![err.to_string()];
    let mut source = err.source();
    while let Some(s) = source {
        let msg = s.to_string();
        if !parts.last().map(|p| p == &msg).unwrap_or(false) {
            parts.push(msg);
        }
        source = s.source();
    }
    parts.join(": ")
}

/// Format an AWS SdkError including the raw HTTP response body when available.
/// B2 returns XML error details in the body that the SDK can't always parse
/// into a typed error, so we surface the raw body for diagnosis.
fn format_sdk_error<E: std::error::Error + 'static>(
    e: &aws_sdk_s3::error::SdkError<E, aws_smithy_runtime_api::http::Response>,
) -> String {
    let mut parts: Vec<String> = vec![format_error_chain(e)];
    if let Some(resp) = e.raw_response() {
        parts.push(format!("HTTP {}", resp.status().as_u16()));
        if let Some(body_bytes) = resp.body().bytes() {
            let body = String::from_utf8_lossy(body_bytes);
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                let snippet = if trimmed.len() > 500 {
                    format!("{}...", &trimmed[..500])
                } else {
                    trimmed.to_string()
                };
                parts.push(format!("body: {}", snippet));
            }
        }
    }
    parts.join(" | ")
}

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

/// Upload a file using S3 multipart upload with bounded parallelism.
/// Parts are uploaded concurrently up to `parallelism` at a time; on any
/// part failure the multipart upload is aborted so B2 doesn't bill for
/// orphaned parts.
async fn multipart_upload(
    client: &S3Client,
    bucket: &str,
    key: &str,
    path: &Path,
    content_type: &str,
    file_size: u64,
    parallelism: usize,
    progress: Option<ProgressFn>,
) -> Result<(), String> {
    let create = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .content_type(content_type)
        .send()
        .await
        .map_err(|e| format!("Failed to start multipart upload: {}", format_sdk_error(&e)))?;

    let upload_id = create
        .upload_id()
        .ok_or("Multipart upload created without an upload ID")?
        .to_string();

    let part_count = file_size.div_ceil(PART_SIZE);
    let bytes_uploaded = Arc::new(AtomicU64::new(0));
    let sem = Arc::new(Semaphore::new(parallelism.max(1)));
    let mut joins: tokio::task::JoinSet<Result<(i32, Option<String>), String>> =
        tokio::task::JoinSet::new();

    for part_num in 1..=part_count {
        let offset = (part_num - 1) * PART_SIZE;
        let length = std::cmp::min(PART_SIZE, file_size - offset);

        let sem_c = sem.clone();
        let client_c = client.clone();
        let bucket_c = bucket.to_string();
        let key_c = key.to_string();
        let upload_id_c = upload_id.clone();
        let path_c = path.to_path_buf();
        let bytes_c = bytes_uploaded.clone();
        let progress_c = progress.clone();

        joins.spawn(async move {
            let _permit = sem_c
                .acquire_owned()
                .await
                .map_err(|e| format!("semaphore closed: {}", e))?;

            let body = ByteStream::read_from()
                .path(&path_c)
                .offset(offset)
                .length(Length::Exact(length))
                .build()
                .await
                .map_err(|e| format!("part {}: read failed: {}", part_num, e))?;

            let resp = client_c
                .upload_part()
                .bucket(&bucket_c)
                .key(&key_c)
                .upload_id(&upload_id_c)
                .part_number(part_num as i32)
                .body(body)
                .send()
                .await
                .map_err(|e| {
                    format!("part {}: upload failed: {}", part_num, format_sdk_error(&e))
                })?;

            let done = bytes_c.fetch_add(length, Ordering::SeqCst) + length;
            if let Some(cb) = &progress_c {
                cb(done, file_size);
            }

            Ok((part_num as i32, resp.e_tag().map(String::from)))
        });
    }

    let mut completed_parts: Vec<CompletedPart> = Vec::with_capacity(part_count as usize);
    let mut failure: Option<String> = None;

    while let Some(joined) = joins.join_next().await {
        match joined {
            Ok(Ok((pn, et))) => {
                completed_parts.push(
                    CompletedPart::builder()
                        .part_number(pn)
                        .set_e_tag(et)
                        .build(),
                );
            }
            Ok(Err(e)) => {
                if failure.is_none() {
                    failure = Some(e);
                }
                joins.abort_all();
            }
            Err(join_err) => {
                if failure.is_none() {
                    failure = Some(format!("part task panicked: {}", join_err));
                }
                joins.abort_all();
            }
        }
    }

    if let Some(e) = failure {
        let _ = client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .send()
            .await;
        return Err(e);
    }

    completed_parts.sort_by_key(|p| p.part_number());

    let completed_mpu = CompletedMultipartUpload::builder()
        .set_parts(Some(completed_parts))
        .build();

    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(&upload_id)
        .multipart_upload(completed_mpu)
        .send()
        .await
        .map_err(|e| format!("Failed to complete multipart upload: {}", format_sdk_error(&e)))?;

    Ok(())
}

pub async fn upload_file(
    file_path: &str,
    mode: &str,
    config: &HashMap<String, String>,
    creds: &B2Credentials,
    ttl: Option<u64>,
    parallelism: usize,
    progress: Option<ProgressFn>,
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
                    return Err(format!("Failed to check existing file: {}", format_sdk_error(&e)));
                }
            }
        }
    }

    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    let file_size = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("Failed to stat file: {}", e))?
        .len();

    // Prime the progress bar at 0 so the UI shows something immediately.
    if let Some(cb) = &progress {
        cb(0, file_size);
    }

    if file_size > MULTIPART_THRESHOLD {
        multipart_upload(
            &client,
            bucket,
            &object_key,
            path,
            &content_type,
            file_size,
            parallelism,
            progress.clone(),
        )
        .await?;
    } else {
        let body = ByteStream::from_path(path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        client
            .put_object()
            .bucket(bucket)
            .key(&object_key)
            .content_type(content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| format!("Upload failed: {}", format_sdk_error(&e)))?;

        // Single-PUT has no byte-level progress hook; jump to complete.
        if let Some(cb) = &progress {
            cb(file_size, file_size);
        }
    }

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

/// Download a URL to a temporary file, preserving the original extension.
/// Returns the path to the temp file.
pub async fn download_url(url: &str) -> Result<String, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    // Extract filename from URL path (strip query params)
    let parsed = url.split('?').next().unwrap_or(url);
    let url_filename = parsed.split('/').last().unwrap_or("download");

    // Get extension, default to "bin"
    let ext = Path::new(url_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");

    // Create temp file with the right extension
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("b2upload_{}.{}", Uuid::new_v4(), ext));

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    file.write_all(&bytes)
        .await
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush temp file: {}", e))?;

    Ok(tmp_path.to_string_lossy().to_string())
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
        .map_err(|e| format!("Connection failed: {}", format_sdk_error(&e)))?;

    // client drops here -- AWS SDK zeroizes its internal credential buffers
    Ok("Connection successful".to_string())
}
