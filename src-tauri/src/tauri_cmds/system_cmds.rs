use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::paths::data_dir;

/// debug.log 大小上限：超过后保留尾部一半，防止无限增长。
const DEBUG_LOG_MAX_BYTES: usize = 512 * 1024;

/// UTC 时间戳（YYYY-MM-DDTHH:MM:SSZ），debug.log 行前缀用。
fn log_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86400, secs % 86400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z",
        h = rem / 3600,
        m = rem % 3600 / 60,
        s = rem % 60
    )
}

#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

/// 前端调试日志：追加一行带时间戳的文本到 app data 的 debug.log，返回日志文件路径。
#[tauri::command]
pub fn debug_log(app: AppHandle, scope: String, message: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let path: PathBuf = dir.join("debug.log");
    let bytes = std::fs::read(&path).unwrap_or_default();
    if bytes.len() > DEBUG_LOG_MAX_BYTES {
        let start = bytes.len() - DEBUG_LOG_MAX_BYTES / 2;
        let start = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i + 1)
            .unwrap_or(start);
        std::fs::write(&path, &bytes[start..]).map_err(|e| e.to_string())?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "[{}] [{}] {}", log_time(), scope, message).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn quick_action(action: String) -> Result<(), String> {
    deskpet_os::actions::quick_action(&action).map_err(|e| e.to_string())
}

/// 激活策略切换后，macOS 可能把主窗口隐藏或重置层级，这里重新拉起。
#[cfg(target_os = "macos")]
fn ensure_main_visible(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_always_on_top(true);
        let _ = main.set_visible_on_all_workspaces(true);
    }
}

/// 设置是否在 Dock 中显示（仅 macOS，其它平台为空操作）。
#[tauri::command]
pub fn set_show_dock(app: AppHandle, visible: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        app.set_dock_visibility(visible).map_err(|e| e.to_string())?;
        ensure_main_visible(&app);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, visible);
    }
    Ok(())
}
