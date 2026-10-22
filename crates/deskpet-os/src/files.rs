use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{OsError, Result};

/// 从 dataURL 解析图片扩展名与二进制内容。
/// 支持 png / jpeg / webp，其他格式报错。
pub fn decode_data_url(data_url: &str) -> Result<(&'static str, Vec<u8>)> {
    let (ext, b64) = if let Some(rest) = data_url.strip_prefix("data:image/png;base64,") {
        ("png", rest)
    } else if let Some(rest) = data_url.strip_prefix("data:image/jpeg;base64,") {
        ("jpg", rest)
    } else if let Some(rest) = data_url.strip_prefix("data:image/webp;base64,") {
        ("webp", rest)
    } else {
        return Err(OsError::Other("不是有效的图片 dataURL".into()));
    };
    let bytes = BASE64.decode(b64)?;
    Ok((ext, bytes))
}

/// 将 PNG dataURL 保存到 `dir/qrcodes/` 下，返回文件路径（不在文件管理器中定位）。
pub fn save_qr_png(dir: &Path, data_url: &str) -> Result<PathBuf> {
    let (ext, bytes) = decode_data_url(data_url)?;
    if ext != "png" {
        return Err(OsError::Other("二维码仅支持 PNG 数据".into()));
    }
    write_image(dir, "qrcodes", "qr", ext, &bytes)
}

/// 将任意图片 dataURL 保存到 `dir/images/` 下，返回文件路径（不在文件管理器中定位）。
pub fn save_image(dir: &Path, data_url: &str) -> Result<PathBuf> {
    let (ext, bytes) = decode_data_url(data_url)?;
    write_image(dir, "images", "img", ext, &bytes)
}

/// 在文件管理器中定位并高亮该文件。
pub fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open")
        .args(["-R", &path.to_string_lossy()])
        .spawn();

    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer")
        .arg(format!("/select,{}", path.to_string_lossy()))
        .spawn();
}

fn write_image(dir: &Path, subdir: &str, prefix: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
    let dir = dir.join(subdir);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{prefix}-{}.{ext}", now_millis()));
    fs::write(&path, bytes)?;
    Ok(path)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1x1 透明 PNG 的 dataURL
    const TINY_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    /// 1x1 黑色 JPEG 的 dataURL
    const TINY_JPG: &str = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwcJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPDs0NDT/wAALCAABAAEBAREA/8QAFAABAQAAAAAAAAAAAAAAAAAAAAv/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAwDAQACEQMRAD8AmAA//9k=";

    #[test]
    fn save_qr_png_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_qr_png(dir.path(), TINY_PNG).unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(dir.path().join("qrcodes")));
        assert_eq!(path.extension().unwrap(), "png");
    }

    #[test]
    fn save_qr_png_rejects_invalid_data() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_qr_png(dir.path(), "not-a-data-url").is_err());
    }

    #[test]
    fn save_image_writes_jpeg_with_jpg_ext() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_image(dir.path(), TINY_JPG).unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(dir.path().join("images")));
        assert_eq!(path.extension().unwrap(), "jpg");
    }

    #[test]
    fn save_image_rejects_unknown_mime() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_image(dir.path(), "data:image/bmp;base64,AAAA").is_err());
    }
}
