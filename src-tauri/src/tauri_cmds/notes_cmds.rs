use deskpet_store::Note;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

#[tauri::command]
pub fn get_notes(app: AppHandle) -> Vec<Note> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .get_notes()
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_notes(app: AppHandle, notes: Vec<Note>) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .save_notes(&notes)
        .map_err(|e| e.to_string())
}
