use deskpet_os::clipboard::{Clipboard, ImagePayload};
use tauri::AppHandle;

use crate::paths::data_dir;

/// 读取系统剪贴板中的图像（无图像时返回 None）。
#[tauri::command]
pub fn get_clipboard_image() -> Result<Option<ImagePayload>, String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    board
        .read_image()
        .map_err(|e| format!("读取剪贴板图像失败: {e}"))
}

/// 将 RGBA 图像写入系统剪贴板。
#[tauri::command]
pub fn set_clipboard_image(payload: ImagePayload) -> Result<(), String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    board
        .write_image(&payload)
        .map_err(|e| format!("写入剪贴板失败: {e}"))
}

/// 将 PNG dataURL 保存到应用数据目录 qrcodes/ 下，并在文件管理器中定位。
#[tauri::command]
pub fn save_qr_png(app: AppHandle, data_url: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let path = deskpet_os::files::save_qr_png(&dir, &data_url).map_err(|e| e.to_string())?;
    deskpet_os::files::reveal(&path);
    Ok(path.to_string_lossy().to_string())
}

/// 将任意图片 dataURL（png/jpeg/webp）保存到应用数据目录 images/ 下，并在文件管理器中定位。
#[tauri::command]
pub fn save_image(app: AppHandle, data_url: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let path = deskpet_os::files::save_image(&dir, &data_url).map_err(|e| e.to_string())?;
    deskpet_os::files::reveal(&path);
    Ok(path.to_string_lossy().to_string())
}
