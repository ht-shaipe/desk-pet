use deskpet_os::clipboard::{Clipboard, ImagePayload};
use deskpet_os::files::ImageFileInfo;
use std::path::Path;
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

/// 列出目录下的图片文件（仅当前目录，跳过隐藏文件），按名称排序。
#[tauri::command]
pub fn list_image_files(dir: String) -> Result<Vec<ImageFileInfo>, String> {
    deskpet_os::files::list_images(Path::new(&dir)).map_err(|e| e.to_string())
}

/// 将本地图片文件读取为 dataURL，供前端 canvas 压缩。
#[tauri::command]
pub fn read_image_data_url(path: String) -> Result<String, String> {
    deskpet_os::files::read_image_data_url(Path::new(&path)).map_err(|e| e.to_string())
}

/// 将压缩结果保存到 `{dir}/compressed/{stem}.min.{ext}`，返回输出文件路径。
#[tauri::command]
pub fn save_compressed_image(
    dir: String,
    src_name: String,
    data_url: String,
) -> Result<String, String> {
    deskpet_os::files::save_compressed_image(Path::new(&dir), &src_name, &data_url)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// 在文件管理器中打开目录或定位文件。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    deskpet_os::files::reveal_entry(Path::new(&path));
    Ok(())
}

/// 由 Rust 代理发送 HTTP 请求，绕开 WebView 跨域限制，并回传请求/响应头。
#[tauri::command]
pub async fn http_request(
    spec: deskpet_os::http::HttpRequestSpec,
) -> Result<deskpet_os::http::HttpResponseData, String> {
    deskpet_os::http::send(&spec).await
}
