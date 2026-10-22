#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn quick_action(action: String) -> Result<(), String> {
    deskpet_os::actions::quick_action(&action).map_err(|e| e.to_string())
}
