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

/// 提取系统应用图标为 PNG（缓存于数据目录 icons/），返回文件路径。
#[tauri::command]
pub fn get_app_icon(app: AppHandle, item: LauncherItem) -> Result<Option<String>, String> {
    if item.item_type != "app" {
        return Ok(None);
    }
    let dir = crate::paths::data_dir(&app)?.join("icons");
    deskpet_os::icons::app_icon_png(&item.value, &dir)
        .map(|p| p.map(|path| path.to_string_lossy().into_owned()))
        .map_err(|e| e.to_string())
}

/// 按启动值（app 路径/名称）提取图标，供应用选择下拉列表展示。
#[tauri::command]
pub fn get_app_icon_by_value(app: AppHandle, value: String) -> Result<Option<String>, String> {
    let dir = crate::paths::data_dir(&app)?.join("icons");
    deskpet_os::icons::app_icon_png(&value, &dir)
        .map(|p| p.map(|path| path.to_string_lossy().into_owned()))
        .map_err(|e| e.to_string())
}

/// 枚举系统已安装应用，供启动项选择。
#[tauri::command]
pub fn list_apps() -> Vec<deskpet_os::apps::AppEntry> {
    deskpet_os::apps::list_apps()
}
