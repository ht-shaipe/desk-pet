use tauri::AppHandle;

#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn quick_action(action: String) -> Result<(), String> {
    deskpet_os::actions::quick_action(&action).map_err(|e| e.to_string())
}

/// 设置是否在 Dock 中显示（仅 macOS，其它平台为空操作）。
#[tauri::command]
pub fn set_show_dock(app: AppHandle, visible: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        app.set_dock_visibility(visible).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, visible);
    }
    Ok(())
}
