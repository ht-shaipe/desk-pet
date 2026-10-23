use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
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

/// 目录下的图片文件信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageFileInfo {
    /// 文件名（不含路径）
    pub name: String,
    /// 文件大小（字节）
    pub size: u64,
}

/// 根据扩展名识别图片 MIME，支持 jpg / png / webp / bmp / gif。
pub fn image_mime_by_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// 列出目录下的图片文件：仅当前目录、跳过隐藏文件，按名称排序。
/// 输出目录 compressed/ 为子目录，天然不会被列出。
pub fn list_images(dir: &Path) -> Result<Vec<ImageFileInfo>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let ext = name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if image_mime_by_ext(&ext).is_some() {
            files.push(ImageFileInfo {
                name,
                size: entry.metadata()?.len(),
            });
        }
    }
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(files)
}

/// 将本地图片文件读取为 dataURL，供前端 canvas 压缩。
pub fn read_image_data_url(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mime = image_mime_by_ext(&ext)
        .ok_or_else(|| OsError::Other("不支持的图片格式".into()))?;
    let bytes = fs::read(path)?;
    Ok(format!("data:{mime};base64,{}", BASE64.encode(bytes)))
}

/// 将压缩结果 dataURL 保存到 `dir/compressed/{stem}.min.{ext}`，不覆盖源文件。
pub fn save_compressed_image(dir: &Path, src_name: &str, data_url: &str) -> Result<PathBuf> {
    let (ext, bytes) = decode_data_url(data_url)?;
    let stem = Path::new(src_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| OsError::Other("无效的文件名".into()))?;
    let out = dir.join("compressed");
    fs::create_dir_all(&out)?;
    let path = out.join(format!("{stem}.min.{ext}"));
    fs::write(&path, bytes)?;
    Ok(path)
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

/// 在文件管理器中打开目录或定位文件（批量压缩结果目录用）。
pub fn reveal_entry(path: &Path) {
    if path.is_dir() {
        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(path).spawn();

        #[cfg(target_os = "windows")]
        let _ = Command::new("explorer")
            .arg(path.to_string_lossy().to_string())
            .spawn();
    } else {
        reveal(path);
    }
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

    #[test]
    fn list_images_filters_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.png"), b"x").unwrap();
        fs::write(dir.path().join("a.JPG"), b"yy").unwrap();
        fs::write(dir.path().join("readme.txt"), b"z").unwrap();
        fs::write(dir.path().join(".hidden.jpeg"), b"h").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("nested.png"), b"n").unwrap();

        let files = list_images(dir.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a.JPG", "b.png"]);
        assert_eq!(files[1].size, 1);
    }

    #[test]
    fn read_image_data_url_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.png");
        fs::write(&file, b"\x89PNG\r\n\x1a\n").unwrap();
        let url = read_image_data_url(&file).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));

        let txt = dir.path().join("t.txt");
        fs::write(&txt, b"nope").unwrap();
        assert!(read_image_data_url(&txt).is_err());
    }

    #[test]
    fn save_compressed_image_uses_min_naming() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_compressed_image(dir.path(), "photo-1.jpg", TINY_PNG).unwrap();
        assert_eq!(
            path,
            dir.path().join("compressed").join("photo-1.min.png")
        );
        assert!(path.is_file());
    }

    #[test]
    fn save_compressed_image_rejects_invalid_data() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_compressed_image(dir.path(), "a.jpg", "bad").is_err());
    }
}
