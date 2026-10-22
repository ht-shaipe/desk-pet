use deskpet_store::ChatEntry;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

#[tauri::command]
pub fn get_chat_messages(app: AppHandle) -> Vec<ChatEntry> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .list_chat_messages()
        .unwrap_or_default()
}

#[tauri::command]
pub fn add_chat_message(app: AppHandle, role: String, content: String) -> Result<ChatEntry, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .add_chat_message(&role, &content)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_chat_message(app: AppHandle, id: i64) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .delete_chat_message(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_chat_messages(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .clear_chat_messages()
        .map_err(|e| e.to_string())
}
