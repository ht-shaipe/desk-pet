use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::config::{read_json_or_default, write_json};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub color: String,
    pub updated_at: u64,
}

#[tauri::command]
pub fn get_notes(app: AppHandle) -> Vec<Note> {
    read_json_or_default(&app, "notes.json")
}

#[tauri::command]
pub fn save_notes(app: AppHandle, notes: Vec<Note>) -> Result<(), String> {
    write_json(&app, "notes.json", &notes)
}
