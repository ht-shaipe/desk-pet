#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn quick_action(action: String) -> Result<(), String> {
    match action.as_str() {
        "dark-mode" => toggle_dark_mode(),
        "mute" => toggle_mute(),
        "lock" => lock_screen(),
        "sleep" => sleep_screen(),
        a if a.starts_with("open-settings:") => open_settings(&a["open-settings:".len()..]),
        other => Err(format!("未知操作: {other}")),
    }
}

fn spawn_detached(cmd: &mut std::process::Command) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("执行失败: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// ---------- macOS ----------

#[cfg(target_os = "macos")]
fn toggle_dark_mode() -> Result<(), String> {
    spawn_detached(std::process::Command::new("osascript").args([
        "-e",
        "tell application \"System Events\" to tell appearance preferences to set dark mode to not dark mode",
    ]))
}

#[cfg(target_os = "macos")]
fn toggle_mute() -> Result<(), String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", "output muted of (get volume settings)"])
        .output()
        .map_err(|e| format!("查询音量状态失败: {e}"))?;
    let muted = String::from_utf8_lossy(&output.stdout).trim() == "true";
    let script = if muted {
        "set volume without output muted"
    } else {
        "set volume with output muted"
    };
    spawn_detached(std::process::Command::new("osascript").args(["-e", script]))
}

#[cfg(target_os = "macos")]
fn lock_screen() -> Result<(), String> {
    spawn_detached(std::process::Command::new("pmset").arg("displaysleepnow"))
}

#[cfg(target_os = "macos")]
fn sleep_screen() -> Result<(), String> {
    spawn_detached(std::process::Command::new("pmset").arg("sleepnow"))
}

#[cfg(target_os = "macos")]
fn open_settings(pane: &str) -> Result<(), String> {
    let target = match pane {
        "main" => "x-apple.systempreferences:com.apple.preference".to_string(),
        "network" => "x-apple.systempreferences:com.apple.preference.network".to_string(),
        "sound" => "x-apple.systempreferences:com.apple.preference.sound".to_string(),
        "display" => "x-apple.systempreferences:com.apple.preference.displays".to_string(),
        "bluetooth" => "x-apple.systempreferences:com.apple.preference.bluetooth".to_string(),
        other => format!("x-apple.systempreferences:{other}"),
    };
    spawn_detached(std::process::Command::new("open").arg(target))
}

// ---------- Windows ----------

#[cfg(target_os = "windows")]
fn toggle_dark_mode() -> Result<(), String> {
    Err("Windows 暂不支持一键切换深浅模式".into())
}

#[cfg(target_os = "windows")]
fn toggle_mute() -> Result<(), String> {
    Err("Windows 暂不支持一键静音".into())
}

#[cfg(target_os = "windows")]
fn lock_screen() -> Result<(), String> {
    spawn_detached(
        std::process::Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]),
    )
}

#[cfg(target_os = "windows")]
fn sleep_screen() -> Result<(), String> {
    spawn_detached(std::process::Command::new("rundll32.exe").args([
        "powrprof.dll,SetSuspendState",
        "0,1,0",
    ]))
}

#[cfg(target_os = "windows")]
fn open_settings(pane: &str) -> Result<(), String> {
    let target = match pane {
        "main" => "ms-settings:".to_string(),
        "network" => "ms-settings:network".to_string(),
        "sound" => "ms-settings:sound".to_string(),
        "display" => "ms-settings:display".to_string(),
        "bluetooth" => "ms-settings:bluetooth".to_string(),
        other => format!("ms-settings:{other}"),
    };
    spawn_detached(std::process::Command::new("cmd").args(["/C", "start", "", &target]))
}

// ---------- 其他平台占位 ----------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn toggle_dark_mode() -> Result<(), String> {
    Err("不支持的平台".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn toggle_mute() -> Result<(), String> {
    Err("不支持的平台".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn lock_screen() -> Result<(), String> {
    Err("不支持的平台".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn sleep_screen() -> Result<(), String> {
    Err("不支持的平台".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_settings(_pane: &str) -> Result<(), String> {
    Err("不支持的平台".into())
}
