use crate::proc::spawn_detached;
use crate::{OsError, Result};

/// 按类型启动：`url` | `app` | `command`。
pub fn launch(item_type: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(OsError::Other("启动项内容为空".into()));
    }
    match item_type {
        "url" => open_url(value),
        "app" => open_app(value),
        "command" => run_command(value),
        other => Err(OsError::Other(format!("未知启动类型: {other}"))),
    }
}

fn open_url(url: &str) -> Result<()> {
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
        Err(OsError::Other("不支持的平台".into()))
    }
}

fn open_app(value: &str) -> Result<()> {
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
        Err(OsError::Other("不支持的平台".into()))
    }
}

fn run_command(value: &str) -> Result<()> {
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
        Err(OsError::Other("不支持的平台".into()))
    }
}
