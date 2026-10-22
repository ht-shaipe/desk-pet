use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
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
