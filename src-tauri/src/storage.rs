use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const SERVICE: &str = "b2upload";
const ACCOUNT: &str = "settings";

const CONNECTION_KEYS: &[&str] = &[
    "DOMAIN",
    "BUCKET_NAME",
    "S3_ENDPOINT",
    "B2_APPLICATION_KEY_ID",
    "B2_APPLICATION_KEY",
];

pub fn save_settings(_app: &AppHandle, values: HashMap<String, String>) -> Result<(), String> {
    let json = serde_json::to_string(&values).map_err(|e| e.to_string())?;
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| format!("Keyring set error: {}", e))?;
    eprintln!("[keyring] Saved {} keys", values.len());
    Ok(())
}

pub fn get_settings(_app: &AppHandle) -> Result<HashMap<String, String>, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(json) => {
            serde_json::from_str(&json).map_err(|e| format!("Settings parse error: {}", e))
        }
        Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
        Err(e) => Err(format!("Keyring get error: {}", e)),
    }
}

pub fn has_settings(_app: &AppHandle) -> Result<bool, String> {
    let settings = get_settings(_app)?;
    Ok(CONNECTION_KEYS.iter().all(|k| {
        settings
            .get(*k)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }))
}

// --- History (JSON file) ---

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

pub fn add_history(app: &AppHandle, entry: Value) {
    let path = history_path(app);
    let mut history = get_history(app);
    history.insert(0, entry);
    let json = serde_json::to_string_pretty(&history).unwrap_or_else(|_| "[]".to_string());
    fs::write(&path, json).ok();
}

pub fn clear_history(app: &AppHandle) {
    let path = history_path(app);
    fs::write(&path, "[]").ok();
}
