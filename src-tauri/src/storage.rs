use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use zeroize::{Zeroize, ZeroizeOnDrop};

const SERVICE: &str = "b2upload";
const SECRETS_ACCOUNT: &str = "secrets";

// Keys stored in config.json (non-sensitive)
const CONFIG_KEYS: &[&str] = &[
    "DOMAIN",
    "BUCKET_NAME",
    "S3_ENDPOINT",
    "FOLDER_1",
    "FOLDER_2",
    "DATE_FOLDERS",
    "UUID_FILENAMES",
    "OVERWRITE_UPLOADS",
    "TOKEN_MODE",
    "DEFAULT_TTL",
    "NOTIFICATIONS",
];

// --- B2Credentials: sensitive data with automatic zeroization ---

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct B2Credentials {
    #[serde(default)]
    pub key_id: String,
    #[serde(default)]
    pub app_key: String,
    #[serde(default)]
    pub folder_1_token: String,
    #[serde(default)]
    pub folder_2_token: String,
    #[serde(default)]
    pub token_secret: String,
}

impl B2Credentials {
    pub fn load() -> Result<Self, String> {
        let entry = keyring::Entry::new(SERVICE, SECRETS_ACCOUNT)
            .map_err(|e| e.to_string())?;

        let mut raw_json = match entry.get_password() {
            Ok(json) => json,
            Err(keyring::Error::NoEntry) => {
                return Ok(Self {
                    key_id: String::new(),
                    app_key: String::new(),
                    folder_1_token: String::new(),
                    folder_2_token: String::new(),
                    token_secret: String::new(),
                });
            }
            Err(e) => return Err(format!("Keyring read error: {}", e)),
        };

        let creds: B2Credentials = serde_json::from_str(&raw_json)
            .map_err(|e| format!("Secrets parse error: {}", e))?;

        // Wipe the raw JSON buffer immediately after parsing
        raw_json.zeroize();

        Ok(creds)
    }

    fn save(&self) -> Result<(), String> {
        let mut raw_json = serde_json::to_string(self)
            .map_err(|e| e.to_string())?;

        let entry = keyring::Entry::new(SERVICE, SECRETS_ACCOUNT)
            .map_err(|e| e.to_string())?;

        let result = entry.set_password(&raw_json)
            .map_err(|e| format!("Keyring set error: {}", e));

        // Wipe the serialized JSON buffer immediately after writing
        raw_json.zeroize();

        result
    }
}

// --- Config file helpers ---

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir");
    fs::create_dir_all(&dir).ok();
    dir.join("config.json")
}

fn read_config(app: &AppHandle) -> HashMap<String, String> {
    let path = config_path(app);
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn write_config(app: &AppHandle, config: &HashMap<String, String>) -> Result<(), String> {
    let path = config_path(app);
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("Config write error: {}", e))
}

// Secret key names used for zeroizing values in HashMaps
const SECRET_KEYS: &[&str] = &[
    "B2_APPLICATION_KEY_ID",
    "B2_APPLICATION_KEY",
    "FOLDER_1_TOKEN",
    "FOLDER_2_TOKEN",
    "TOKEN_SECRET",
];

/// Zeroize any secret values present in a HashMap.
fn zeroize_secrets_in_map(map: &mut HashMap<String, String>) {
    for &key in SECRET_KEYS {
        if let Some(val) = map.get_mut(key) {
            val.zeroize();
        }
    }
}

// --- Public API ---

/// Returns only non-sensitive config (no secrets). Use for upload/connection
/// paths where B2Credentials is loaded separately.
pub fn get_config(app: &AppHandle) -> HashMap<String, String> {
    read_config(app)
}

pub fn save_settings(app: &AppHandle, mut values: HashMap<String, String>) -> Result<(), String> {
    let mut config: HashMap<String, String> = HashMap::new();

    // Load existing credentials so empty fields preserve current values
    let existing = B2Credentials::load()?;

    // Helper: use new value if non-empty, otherwise keep existing
    let merge = |new: Option<&String>, existing: &str| -> String {
        match new {
            Some(v) if !v.is_empty() => v.clone(),
            _ => existing.to_string(),
        }
    };

    let creds = B2Credentials {
        key_id: merge(values.get("B2_APPLICATION_KEY_ID"), &existing.key_id),
        app_key: merge(values.get("B2_APPLICATION_KEY"), &existing.app_key),
        folder_1_token: merge(values.get("FOLDER_1_TOKEN"), &existing.folder_1_token),
        folder_2_token: merge(values.get("FOLDER_2_TOKEN"), &existing.folder_2_token),
        token_secret: merge(values.get("TOKEN_SECRET"), &existing.token_secret),
    };
    // existing is dropped here -> ZeroizeOnDrop wipes fields
    drop(existing);

    for (key, val) in &values {
        if CONFIG_KEYS.contains(&key.as_str()) {
            config.insert(key.clone(), val.clone());
        }
    }

    write_config(app, &config)?;
    creds.save()?;
    // creds is dropped here -> ZeroizeOnDrop wipes fields

    // Zeroize any secret values in the incoming HashMap
    zeroize_secrets_in_map(&mut values);

    #[cfg(debug_assertions)]
    eprintln!(
        "[storage] Saved {} config keys + secrets blob",
        config.len()
    );

    Ok(())
}

/// Returns only non-sensitive config values for the frontend settings form.
/// Secret values are never sent to the frontend.
pub fn get_settings(app: &AppHandle) -> Result<HashMap<String, String>, String> {
    Ok(read_config(app))
}

/// Returns the names of secret keys that have stored (non-empty) values.
/// The actual secret values are never exposed.
pub fn get_saved_secret_keys() -> Result<Vec<String>, String> {
    let creds = B2Credentials::load()?;
    let mut keys = Vec::new();
    if !creds.key_id.is_empty() { keys.push("B2_APPLICATION_KEY_ID".to_string()); }
    if !creds.app_key.is_empty() { keys.push("B2_APPLICATION_KEY".to_string()); }
    if !creds.folder_1_token.is_empty() { keys.push("FOLDER_1_TOKEN".to_string()); }
    if !creds.folder_2_token.is_empty() { keys.push("FOLDER_2_TOKEN".to_string()); }
    if !creds.token_secret.is_empty() { keys.push("TOKEN_SECRET".to_string()); }
    // creds is dropped here -> ZeroizeOnDrop wipes fields
    Ok(keys)
}

pub fn has_settings(app: &AppHandle) -> Result<bool, String> {
    let config = read_config(app);
    // Check non-sensitive connection keys from config
    let config_ok = ["DOMAIN", "BUCKET_NAME", "S3_ENDPOINT"].iter().all(|k| {
        config.get(*k).map(|v| !v.is_empty()).unwrap_or(false)
    });
    if !config_ok {
        return Ok(false);
    }
    // Check sensitive connection keys from credentials
    let creds = B2Credentials::load()?;
    Ok(!creds.key_id.is_empty() && !creds.app_key.is_empty())
    // creds is dropped here -> ZeroizeOnDrop wipes fields
}

// --- History (JSON file) with mutex protection ---

pub struct HistoryMutex(pub std::sync::Mutex<()>);

impl HistoryMutex {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(()))
    }
}

fn history_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("no app data dir");
    fs::create_dir_all(&dir).ok();
    dir.join("history.json")
}

pub fn get_history(app: &AppHandle) -> Vec<Value> {
    let path = history_path(app);
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => vec![],
    }
}

const MAX_HISTORY: usize = 200;

pub fn add_history(app: &AppHandle, entry: Value) {
    let path = history_path(app);
    let mut history = get_history(app);
    history.insert(0, entry);
    history.truncate(MAX_HISTORY);
    let json = serde_json::to_string_pretty(&history).unwrap_or_else(|_| "[]".to_string());
    if let Err(e) = fs::write(&path, json) {
        eprintln!("[history] Failed to write: {}", e);
    }
}

pub fn clear_history(app: &AppHandle) {
    let path = history_path(app);
    if let Err(e) = fs::write(&path, "[]") {
        eprintln!("[history] Failed to clear: {}", e);
    }
}

pub fn delete_history_entry(app: &AppHandle, url: &str) {
    let path = history_path(app);
    let mut history = get_history(app);
    history.retain(|entry| {
        entry.get("url").and_then(|v| v.as_str()).unwrap_or("") != url
    });
    let json = serde_json::to_string_pretty(&history).unwrap_or_else(|_| "[]".to_string());
    if let Err(e) = fs::write(&path, json) {
        eprintln!("[history] Failed to delete entry: {}", e);
    }
}
