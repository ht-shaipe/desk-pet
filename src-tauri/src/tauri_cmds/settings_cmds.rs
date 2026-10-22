use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

use crate::paths::data_dir;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub pet_x: Option<i32>,
    pub pet_y: Option<i32>,
    pub always_on_top: bool,
    #[serde(default)]
    pub locale: Option<String>,
    /// light | dark | auto，缺省视为 auto（跟随系统）
    #[serde(default)]
    pub theme: Option<String>,
    /// 精灵图标显示大小（逻辑像素，32-64），缺省 48
    #[serde(default)]
    pub sprite_size: Option<u32>,
    /// 主题色（#rrggbb），缺省用内置蓝色
    #[serde(default)]
    pub accent_color: Option<String>,
    /// 远端账户 token（同步用，裸 Authorization 头）
    #[serde(default)]
    pub auth_token: Option<String>,
    /// 已登录账户（手机号）
    #[serde(default)]
    pub auth_user: Option<String>,
    /// 自定义精灵头像（app data 内绝对路径），None 用内置图标
    #[serde(default)]
    pub pet_avatar: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pet_x: None,
            pet_y: None,
            always_on_top: true,
            locale: None,
            theme: None,
            sprite_size: None,
            accent_color: None,
            auth_token: None,
            auth_user: None,
            pet_avatar: None,
        }
    }
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

pub(crate) fn write_json<T: Serialize>(
    app: &AppHandle,
    file: &str,
    data: &T,
) -> Result<(), String> {
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

/// 支持的图片后缀（含点号）
const AVATAR_EXTS: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

/// 更换精灵头像：把用户选择的图片拷贝到 app data，返回新文件路径。
/// 旧头像文件（avatar.* 命名）会被清理，避免残留。
#[tauri::command]
pub fn set_pet_avatar(app: AppHandle, src: String) -> Result<String, String> {
    let src = PathBuf::from(&src);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    if !AVATAR_EXTS.contains(&ext.as_str()) {
        return Err(format!("不支持的图片格式: {ext}"));
    }

    let dir = data_dir(&app)?;
    let dst = dir.join(format!("avatar{ext}"));
    fs::copy(&src, &dst).map_err(|e| format!("复制头像失败: {e}"))?;

    // 清理其他后缀的旧头像
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("avatar.") && name != format!("avatar{ext}") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(dst.to_string_lossy().to_string())
}

/// 恢复默认头像：删除 app data 中的自定义头像文件。
#[tauri::command]
pub fn reset_pet_avatar(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("avatar.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}
