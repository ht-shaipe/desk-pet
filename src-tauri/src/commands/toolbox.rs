use arboard::{Clipboard, ImageData};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

use super::config::data_dir;

/// RGBA 原始图像，前后端通用载体。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImagePayload {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 读取系统剪贴板中的图像（无图像时返回 None）。
#[tauri::command]
pub fn get_clipboard_image() -> Result<Option<ImagePayload>, String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    match board.get_image() {
        Ok(img) => Ok(Some(ImagePayload {
            width: img.width as u32,
            height: img.height as u32,
            rgba: img.bytes.into_owned(),
        })),
        Err(_) => Ok(None),
    }
}

/// 将 RGBA 图像写入系统剪贴板。
#[tauri::command]
pub fn set_clipboard_image(payload: ImagePayload) -> Result<(), String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    let data = ImageData {
        width: payload.width as usize,
        height: payload.height as usize,
        bytes: Cow::Owned(payload.rgba),
    };
    board
        .set_image(data)
        .map_err(|e| format!("写入剪贴板失败: {e}"))
}

/// 在文件管理器中定位并高亮该文件。
fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open")
        .args(["-R", &path.to_string_lossy()])
        .spawn();

    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer")
        .arg(format!("/select,{}", path.to_string_lossy()))
        .spawn();
}

/// 将 PNG dataURL 保存到应用数据目录 qrcodes/ 下，并在文件管理器中定位。
#[tauri::command]
pub fn save_qr_png(app: AppHandle, data_url: String) -> Result<String, String> {
    let b64 = data_url.trim_start_matches("data:image/png;base64,");
    let bytes = BASE64.decode(b64).map_err(|e| format!("图像数据无效: {e}"))?;

    let dir = data_dir(&app)?.join("qrcodes");
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建目录: {e}"))?;
    let path = dir.join(format!("qr-{}.png", now_millis()));
    fs::write(&path, bytes).map_err(|e| format!("写入文件失败: {e}"))?;

    reveal_in_file_manager(&path);
    Ok(path.to_string_lossy().to_string())
}
