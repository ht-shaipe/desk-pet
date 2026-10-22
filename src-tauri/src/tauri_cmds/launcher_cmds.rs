use deskpet_store::{LauncherConfig, LauncherItem};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

#[tauri::command]
pub fn get_launcher(app: AppHandle) -> LauncherConfig {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .get_launcher()
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_launcher(app: AppHandle, config: LauncherConfig) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .save_launcher(&config.items)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launch_item(item: LauncherItem) -> Result<(), String> {
    deskpet_os::launch::launch(&item.item_type, &item.value).map_err(|e| e.to_string())
}
