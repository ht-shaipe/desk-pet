//! 卷空间：删除前后用 statvfs 实测，不拿估算冒充结果。
//!
//! 口径说明（macOS/APFS）：`available` 是 `df` 口径的可用空间；
//! Finder 显示更大是因为算入了 purgeable space；APFS 克隆文件各自按全量
//! 计入估算，删除后实测释放可能小于估算；Time Machine 本地快照不是文件，
//! 不出现在扫描结果里，但会占用可用空间。

use std::{io, path::Path};

use crate::removal::read_mount_points;

use serde::Serialize;

/// 卷空间（字节）。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInfo {
    pub total: u64,
    pub available: u64,
}

/// 查询 `path` 所在卷的总容量与可用空间（非 root 可用口径）。
///
/// 路径不存在（如删除后的目标）时逐级向上回退到最近的祖先目录，
/// 保证"删除后实测"总是可测。
pub fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    let mut probe = Some(path.to_path_buf());
    while let Some(current) = probe {
        if current.as_os_str().is_empty() {
            break;
        }
        if current.exists() {
            let total = fs4::total_space(&current)?;
            let available = fs4::available_space(&current)?;
            return Ok(SpaceInfo { total, available });
        }
        probe = current.parent().map(|parent| parent.to_path_buf());
    }
    Err(io::Error::other(format!(
        "无法访问路径: {}",
        path.display()
    )))
}

/// 一个可扫描的卷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    /// 挂载点（扫描根）
    pub path: String,
    pub total: u64,
    pub available: u64,
}

/// 列出可扫描的真实卷：根分区 + /Volumes 下已挂载的外接/网络卷。
/// 过滤伪文件系统（devfs/autofs）与系统私有挂载；查询不到容量的卷跳过。
pub fn list_volumes() -> Vec<VolumeInfo> {
    read_mount_points()
        .into_iter()
        .filter(|mount| is_scannable_volume(&mount.to_string_lossy()))
        .filter_map(|mount| {
            let info = space_info(&mount).ok()?;
            Some(VolumeInfo {
                path: mount.to_string_lossy().to_string(),
                total: info.total,
                available: info.available,
            })
        })
        .collect()
}

/// /Volumes 下的伪卷名（Time Machine 快照、devfs 等）不入列。
fn is_pseudo_volume_name(mount: &str) -> bool {
    let name = mount.rsplit('/').next().unwrap_or("");
    name == "devfs" || name.starts_with("com.apple.")
}

/// 挂载点是否值得展示为可扫描卷。
fn is_scannable_volume(mount: &str) -> bool {
    // 根卷不作为可扫描卷：扫描 `/` 需要系统树剪枝，当前命令层会拒绝它。
    if mount == "/" {
        return false;
    }
    // 只认 /Volumes 下的显式挂载（外接盘/网络盘）；系统私有挂载不入列
    if !mount.starts_with("/Volumes/") {
        return false;
    }
    !is_pseudo_volume_name(mount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_info_reports_current_volume() {
        let info = space_info(Path::new(".")).expect("应能查询当前目录所在卷");
        assert!(info.total > 0);
        assert!(info.available <= info.total);
    }

    #[test]
    fn missing_path_falls_back_to_existing_ancestor() {
        let info = space_info(Path::new("./no-such-dir/nested")).expect("应回退到祖先卷");
        assert!(info.total > 0);
    }

    #[test]
    fn volumes_exclude_root_and_pseudo_mounts() {
        let volumes = list_volumes();
        assert!(
            volumes.iter().all(|volume| volume.path != "/"),
            "根分区当前不可扫描，不应入列: {volumes:?}"
        );
        assert!(
            volumes.iter().all(|volume| volume.total > 0),
            "查询不到容量的卷应被跳过"
        );
    }

    #[test]
    fn pseudo_volume_names_are_filtered() {
        assert!(!is_scannable_volume("/"));
        assert!(is_scannable_volume("/Volumes/USB"));
        assert!(!is_scannable_volume("/System/Volumes/Data"));
        assert!(!is_scannable_volume("/private/var"));
        assert!(!is_scannable_volume("/Volumes/devfs"));
    }
}
