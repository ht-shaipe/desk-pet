use deskpet_store::settings::AppSettings;
use std::path::PathBuf;
use tauri::AppHandle;

use crate::paths::data_dir;

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    let dir = data_dir(&app).unwrap_or_default();
    deskpet_store::settings::load(&dir)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let dir = data_dir(&app)?;
    // device_id 由 Rust 侧 channel_start 生成并持久化；前端设置快照可能载于生成之前
    // （device_id 为 null），整存快照时保留磁盘已有值，避免设备指纹被清掉后
    // 下次启动重新生成、服务端把同一台设备当成新设备（补发任务错位）。
    let mut settings = settings;
    if settings.device_id.is_none() {
        settings.device_id = deskpet_store::settings::load(&dir).device_id;
    }
    deskpet_store::settings::save(&dir, &settings).map_err(|e| e.to_string())
}

/// 更换精灵头像：把用户选择的图片拷贝到 app data，返回新文件路径。
#[tauri::command]
pub fn set_pet_avatar(app: AppHandle, src: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let dst = deskpet_store::avatar::set(&dir, &PathBuf::from(&src)).map_err(|e| e.to_string())?;
    Ok(dst.to_string_lossy().to_string())
}

/// 恢复默认头像：删除 app data 中的自定义头像文件。
#[tauri::command]
pub fn reset_pet_avatar(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?;
    deskpet_store::avatar::reset(&dir).map_err(|e| e.to_string())
}
