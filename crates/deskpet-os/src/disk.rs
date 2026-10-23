//! 磁盘用量查询。

use std::path::Path;

use serde::Serialize;

use crate::{OsError, Result};

/// 指定路径所在磁盘卷的空间情况（字节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub total: u64,
    pub available: u64,
}

/// 查询 `path` 所在卷的总容量与可用空间；路径不存在时回退到其最近的父目录。
pub fn disk_usage(path: &Path) -> Result<DiskUsage> {
    // statfs 按路径查询所在卷，路径本身不存在也可以查到卷信息，
    // 但部分平台要求路径存在，逐级向上回退到可访问的祖先目录
    let mut probe = Some(path.to_path_buf());
    while let Some(current) = probe {
        if current.as_os_str().is_empty() {
            break;
        }
        if current.exists() {
            let total = fs4::total_space(&current).map_err(OsError::Io)?;
            let available = fs4::available_space(&current).map_err(OsError::Io)?;
            return Ok(DiskUsage { total, available });
        }
        probe = current.parent().map(|parent| parent.to_path_buf());
    }
    Err(OsError::Io(std::io::Error::other(format!(
        "无法访问路径: {}",
        path.display()
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_positive_usage_for_existing_dir() {
        let usage = disk_usage(Path::new(".")).expect("应能查询当前目录所在卷");
        assert!(usage.total > 0);
        assert!(usage.available > 0);
        assert!(usage.available <= usage.total);
    }

    #[test]
    fn falls_back_to_parent_for_missing_path() {
        let usage = disk_usage(Path::new("./no-such-dir/nested"))
            .expect("不存在的路径应回退到父目录查询");
        assert!(usage.total > 0);
    }
}
