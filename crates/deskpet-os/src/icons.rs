use std::fs;
use std::path::{Path, PathBuf};

use crate::{OsError, Result};

/// FNV-1a 64 位哈希，用于生成缓存文件名。
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 提取应用图标为 PNG，返回缓存文件路径；无法提取时返回 `Ok(None)`。
pub fn app_icon_png(value: &str, cache_dir: &Path) -> Result<Option<PathBuf>> {
    fs::create_dir_all(cache_dir).map_err(OsError::Io)?;
    let Some(source) = resolve_source(value) else {
        return Ok(None);
    };
    let out = cache_dir.join(format!("{:016x}.png", fnv1a(&source.to_string_lossy())));
    if needs_refresh(&source, &out) {
        convert(&source, &out)?;
    }
    Ok(Some(out))
}

/// 源文件比缓存新（或缓存缺失）时需要重新转换。
fn needs_refresh(source: &Path, out: &Path) -> bool {
    match (fs::metadata(source), fs::metadata(out)) {
        (Ok(s), Ok(o)) => match (s.modified(), o.modified()) {
            (Ok(sm), Ok(om)) => sm > om,
            _ => true,
        },
        _ => true,
    }
}

/// 把启动项 value 解析为图标源文件（icns / exe）。
#[cfg(target_os = "macos")]
fn resolve_source(value: &str) -> Option<PathBuf> {
    let bundle = if value.contains('/') {
        PathBuf::from(value)
    } else {
        let home = std::env::var("HOME").ok()?;
        let candidates = [
            PathBuf::from("/System/Applications"),
            PathBuf::from("/Applications"),
            PathBuf::from(format!("{home}/Applications")),
            PathBuf::from("/System/Library/CoreServices"),
        ];
        let name = format!("{value}.app");
        candidates
            .into_iter()
            .map(|dir| dir.join(&name))
            .find(|p| p.is_dir())?
    };
    let icon = bundle_icon_file(&bundle)?;
    Some(icon)
}

/// 从 bundle 的 Info.plist 读取 CFBundleIconFile 指向的 icns 文件。
#[cfg(target_os = "macos")]
fn bundle_icon_file(bundle: &Path) -> Option<PathBuf> {
    let plist_path = bundle.join("Contents/Info.plist");
    let value = plist::Value::from_file(&plist_path).ok()?;
    let name = value.as_dictionary()?.get("CFBundleIconFile")?.as_string()?;
    let name = if name.ends_with(".icns") {
        name.to_string()
    } else {
        format!("{name}.icns")
    };
    let resources = bundle.join("Contents/Resources");
    let icon = resources.join(&name);
    if icon.is_file() {
        return Some(icon);
    }
    // 极少数应用把图标放在 Contents 下而非 Resources
    let icon = bundle.join("Contents").join(&name);
    if icon.is_file() {
        return Some(icon);
    }
    None
}

/// macOS：用系统自带 sips 把 icns 转 PNG（取 128px 尺寸）。
#[cfg(target_os = "macos")]
fn convert(source: &Path, out: &Path) -> Result<()> {
    let status = std::process::Command::new("sips")
        .args(["-s", "format", "png", "-Z", "128"])
        .arg(source)
        .arg("--out")
        .arg(out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(OsError::Io)?;
    if status.success() && out.is_file() {
        Ok(())
    } else {
        Err(OsError::Other("icns 图标转换失败".into()))
    }
}

/// Windows：把 value 解析为 exe 文件路径（where 查找 PATH）。
#[cfg(target_os = "windows")]
fn resolve_source(value: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(value);
    if direct.is_file() {
        return Some(direct);
    }
    let output = std::process::Command::new("where")
        .arg(value)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let path = PathBuf::from(line);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Windows：用 PowerShell + System.Drawing 提取 exe 关联图标。
#[cfg(target_os = "windows")]
fn convert(source: &Path, out: &Path) -> Result<()> {
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         [System.Drawing.Icon]::ExtractAssociatedIcon('{}').ToBitmap()\
         .Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)",
        source.to_string_lossy().replace('\'', "''"),
        out.to_string_lossy().replace('\'', "''")
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(OsError::Io)?;
    if status.success() && out.is_file() {
        Ok(())
    } else {
        Err(OsError::Other("exe 图标提取失败".into()))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn resolve_source(_value: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn convert(_source: &Path, _out: &Path) -> Result<()> {
    Err(OsError::Other("不支持的平台".into()))
}
