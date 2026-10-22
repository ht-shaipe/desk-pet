use std::fs;
use std::path::{Path, PathBuf};

use crate::{Result, StoreError};

/// 支持的图片后缀（含点号）
const AVATAR_EXTS: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

/// 更换精灵头像：把用户选择的图片拷贝到 `dir`，返回新文件路径。
/// 旧头像文件（avatar.* 命名）会被清理，避免残留。
pub fn set(dir: &Path, src: &Path) -> Result<PathBuf> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    if !AVATAR_EXTS.contains(&ext.as_str()) {
        return Err(StoreError::Other(format!("不支持的图片格式: {ext}")));
    }

    let dst = dir.join(format!("avatar{ext}"));
    fs::copy(src, &dst)?;

    // 清理其他后缀的旧头像
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("avatar.") && name != format!("avatar{ext}") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(dst)
}

/// 恢复默认头像：删除 `dir` 中的自定义头像文件。
pub fn reset(dir: &Path) -> Result<()> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("avatar.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_copies_and_cleans_old_avatar() {
        let dir = tempfile::tempdir().unwrap();
        // 预置旧头像
        let old = dir.path().join("avatar.png");
        fs::write(&old, b"old").unwrap();

        // 新头像源文件
        let src = dir.path().join("src.jpg");
        fs::write(&src, b"new").unwrap();
        let dst = set(dir.path(), &src).unwrap();
        assert_eq!(dst, dir.path().join("avatar.jpg"));
        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert!(!old.exists());
    }

    #[test]
    fn set_rejects_unsupported_ext() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pic.txt");
        fs::write(&src, b"x").unwrap();
        assert!(set(dir.path(), &src).is_err());
    }

    #[test]
    fn reset_removes_all_avatars() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("avatar.png"), b"a").unwrap();
        fs::write(dir.path().join("avatar.jpg"), b"b").unwrap();
        fs::write(dir.path().join("keep.txt"), b"c").unwrap();
        reset(dir.path()).unwrap();
        assert!(!dir.path().join("avatar.png").exists());
        assert!(!dir.path().join("avatar.jpg").exists());
        assert!(dir.path().join("keep.txt").exists());
    }
}
