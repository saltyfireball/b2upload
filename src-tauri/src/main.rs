#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;
mod uploader;

use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::Manager;
use tauri::LogicalSize;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[tauri::command]
async fn get_settings(app: tauri::AppHandle) -> Result<HashMap<String, String>, String> {
    storage::get_settings(&app)
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, values: HashMap<String, String>) -> Result<bool, String> {
    storage::save_settings(&app, values)?;
    Ok(true)
}

#[tauri::command]
async fn has_settings(app: tauri::AppHandle) -> Result<bool, String> {
    storage::has_settings(&app)
}

#[tauri::command]
async fn upload_file(
    app: tauri::AppHandle,
    history_mutex: tauri::State<'_, storage::HistoryMutex>,
    file_path: String,
    mode: String,
    auto_clip: bool,
    ttl: Option<u64>,
) -> Result<String, String> {
    let config = storage::get_config(&app);
    let creds = storage::B2Credentials::load()?;
    let url = uploader::upload_file(&file_path, &mode, &config, &creds, ttl).await?;

    if auto_clip {
        app.clipboard()
            .write_text(&url)
            .map_err(|e| e.to_string())?;
    }

    let now = chrono::Local::now();
    let datetime = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let entry = json!({
        "file": file_name,
        "url": url,
        "datetime": datetime,
        "mode": mode,
    });

    {
        let _lock = history_mutex.0.lock().unwrap();
        storage::add_history(&app, entry);
    }

    Ok(url)
}

#[tauri::command]
async fn copy_to_clipboard(app: tauri::AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(&text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Vec<Value> {
    storage::get_history(&app)
}

#[tauri::command]
fn clear_history(
    app: tauri::AppHandle,
    history_mutex: tauri::State<'_, storage::HistoryMutex>,
) -> bool {
    let _lock = history_mutex.0.lock().unwrap();
    storage::clear_history(&app);
    true
}

#[tauri::command]
fn delete_history_entry(
    app: tauri::AppHandle,
    history_mutex: tauri::State<'_, storage::HistoryMutex>,
    url: String,
) -> bool {
    let _lock = history_mutex.0.lock().unwrap();
    storage::delete_history_entry(&app, &url);
    true
}

#[tauri::command]
async fn get_saved_secret_keys() -> Result<Vec<String>, String> {
    storage::get_saved_secret_keys()
}

#[tauri::command]
async fn test_connection(
    app: tauri::AppHandle,
) -> Result<String, String> {
    let config = storage::get_config(&app);
    let creds = storage::B2Credentials::load()?;
    uploader::test_connection(&config, &creds).await
}

#[tauri::command]
async fn resize_window(window: tauri::WebviewWindow, width: u32, height: u32) -> Result<(), String> {
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window.center().map_err(|e| e.to_string())?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(storage::HistoryMutex::new())
        .setup(|app| {
            let path = app
                .path()
                .app_data_dir()
                .expect("no app data dir");
            std::fs::create_dir_all(&path).ok();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            has_settings,
            get_saved_secret_keys,
            upload_file,
            test_connection,
            copy_to_clipboard,
            get_history,
            clear_history,
            delete_history_entry,
            resize_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
