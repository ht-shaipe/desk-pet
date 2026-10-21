use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub pet_x: Option<i32>,
    pub pet_y: Option<i32>,
    pub always_on_top: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pet_x: None,
            pet_y: None,
            always_on_top: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LauncherItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// app | url | command
    pub item_type: String,
    pub value: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LauncherConfig {
    pub items: Vec<LauncherItem>,
}

pub(crate) fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录: {e}"))?;
    Ok(dir)
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned + Default>(
    app: &AppHandle,
    file: &str,
) -> Result<T, String> {
    let path = data_dir(app)?.join(file);
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub(crate) fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(
    app: &AppHandle,
    file: &str,
) -> T {
    read_json(app, file).unwrap_or_default()
}

pub(crate) fn write_json<T: Serialize>(app: &AppHandle, file: &str, data: &T) -> Result<(), String> {
    let path = data_dir(app)?.join(file);
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    read_json_or_default(&app, "settings.json")
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    write_json(&app, "settings.json", &settings)
}

#[tauri::command]
pub fn get_launcher(app: AppHandle) -> LauncherConfig {
    read_json_or_default(&app, "launcher.json")
}

#[tauri::command]
pub fn save_launcher(app: AppHandle, config: LauncherConfig) -> Result<(), String> {
    write_json(&app, "launcher.json", &config)
}
