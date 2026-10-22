use deskpet_store::{LauncherConfig, LauncherItem};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

#[tauri::command]
pub fn get_launcher(app: AppHandle) -> LauncherConfig {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .get_launcher()
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_launcher(app: AppHandle, config: LauncherConfig) -> Result<(), String> {
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .save_launcher(&config.items)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launch_item(item: LauncherItem) -> Result<(), String> {
    let value = item.value.trim();
    if value.is_empty() {
        return Err("启动项内容为空".into());
    }
    match item.item_type.as_str() {
        "url" => open_url(value),
        "app" => open_app(value),
        "command" => run_command(value),
        other => Err(format!("未知启动类型: {other}")),
    }
}

fn spawn_detached(cmd: &mut std::process::Command) -> Result<(), String> {
    let mut child = cmd.spawn().map_err(|e| format!("启动失败: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn open_url(url: &str) -> Result<(), String> {
    let mut url = url.to_string();
    if !url.contains("://") {
        url = format!("https://{url}");
    }
    #[cfg(target_os = "macos")]
    {
        spawn_detached(std::process::Command::new("open").arg(&url))
    }
    #[cfg(target_os = "windows")]
    {
        spawn_detached(std::process::Command::new("cmd").args(["/C", "start", "", &url]))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
        Err("不支持的平台".into())
    }
}

fn open_app(value: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if value.contains('/') {
            spawn_detached(std::process::Command::new("open").arg(value))
        } else {
            spawn_detached(std::process::Command::new("open").args(["-a", value]))
        }
    }
    #[cfg(target_os = "windows")]
    {
        spawn_detached(std::process::Command::new("cmd").args(["/C", "start", "", value]))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = value;
        Err("不支持的平台".into())
    }
}

fn run_command(value: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        spawn_detached(std::process::Command::new("sh").arg("-c").arg(value))
    }
    #[cfg(target_os = "windows")]
    {
        spawn_detached(std::process::Command::new("cmd").args(["/C", value]))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = value;
        Err("不支持的平台".into())
    }
}
