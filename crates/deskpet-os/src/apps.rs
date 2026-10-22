use serde::{Deserialize, Serialize};

/// 系统已安装应用条目。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppEntry {
    /// 显示名称
    pub name: String,
    /// 启动值（macOS 为 .app 路径或名称；Windows 为 .lnk 快捷方式路径）
    pub value: String,
}

/// 枚举系统已安装应用（按名称排序）。
pub fn list_apps() -> Vec<AppEntry> {
    let mut apps = collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.value == b.value);
    apps
}

#[cfg(target_os = "macos")]
fn collect() -> Vec<AppEntry> {
    use std::fs;
    use std::path::Path;

    fn scan_dir(dir: &Path, depth: u8, out: &mut Vec<AppEntry>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if file_name.starts_with('.') {
                continue;
            }
            if file_name.ends_with(".app") {
                let name = file_name.trim_end_matches(".app").to_string();
                out.push(AppEntry {
                    name,
                    value: path.to_string_lossy().into_owned(),
                });
            } else if path.is_dir() {
                scan_dir(&path, depth - 1, out);
            }
        }
    }

    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        scan_dir(Path::new(&home).join("Applications").as_path(), 2, &mut out);
    }
    scan_dir(Path::new("/Applications"), 2, &mut out);
    scan_dir(Path::new("/System/Applications"), 2, &mut out);
    scan_dir(Path::new("/System/Library/CoreServices"), 1, &mut out);
    out
}

#[cfg(target_os = "windows")]
fn collect() -> Vec<AppEntry> {
    use std::fs;
    use std::path::Path;

    fn scan_dir(dir: &Path, depth: u8, out: &mut Vec<AppEntry>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, depth - 1, out);
            } else if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|n| n.to_str()) {
                    if !stem.starts_with('.') {
                        out.push(AppEntry {
                            name: stem.to_string(),
                            value: path.to_string_lossy().into_owned(),
                        });
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    let roots = [
        "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs",
        &format!(
            "{}\\Microsoft\\Windows\\Start Menu\\Programs",
            std::env::var("APPDATA").unwrap_or_default()
        ),
    ];
    for root in roots {
        scan_dir(Path::new(root), 3, &mut out);
    }
    out
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn collect() -> Vec<AppEntry> {
    Vec::new()
}
