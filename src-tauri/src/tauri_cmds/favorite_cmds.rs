use deskpet_store::Favorite;
use std::path::Path;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// 导入文件大小上限：8MB（收藏 JSON 正常远小于此）。
const IMPORT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// 读取用户通过文件对话框选择的 JSON 文本（lenience：仅信任用户主动选择的路径）。
#[tauri::command]
pub fn read_favorite_import_file(path: String) -> Result<String, String> {
    let p = Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > IMPORT_MAX_BYTES {
        return Err("导入文件过大".into());
    }
    std::fs::read_to_string(p).map_err(|e| e.to_string())
}

/// 将导出的收藏 JSON 写到用户通过保存对话框选择的路径。
#[tauri::command]
pub fn write_favorite_export_file(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_favorites(app: AppHandle) -> Vec<Favorite> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .get_favorites()
        .inspect_err(|e| eprintln!("[favorites] read failed: {e}"))
        .unwrap_or_default()
}

/// 批量 upsert 收藏（导入 / 云拉取 / 手动添加共用），返回实际入库条数。
#[tauri::command]
pub fn upsert_favorites(app: AppHandle, items: Vec<Favorite>) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .upsert_favorites(&items)
        .map_err(|e| e.to_string())
}

/// 批量删除（单删、按条件清空共用）。
#[tauri::command]
pub fn delete_favorites(app: AppHandle, urls: Vec<String>) -> Result<usize, String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .delete_favorites(&urls)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_favorites(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .clear_favorites()
        .map_err(|e| e.to_string())
}
