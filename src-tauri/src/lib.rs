mod commands;
mod state;
mod tray;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::config::get_settings,
            commands::config::save_settings,
            commands::config::get_launcher,
            commands::config::save_launcher,
            commands::notes::get_notes,
            commands::notes::save_notes,
            commands::launcher::launch_item,
            commands::clipboard::get_clipboard_history,
            commands::clipboard::clear_clipboard_history,
            commands::clipboard::delete_clipboard_entry,
            commands::clipboard::copy_to_clipboard,
            commands::system::quick_action,
            commands::system::get_platform,
        ])
        .setup(|app| {
            use commands::config::read_json_or_default;
            use tauri::Listener;

            let handle = app.handle();
            let settings = read_json_or_default::<commands::config::AppSettings>(handle, "settings.json");
            let locale = settings.locale.unwrap_or_else(|| "zh-CN".to_string());
            tray::create_tray(handle, &locale)?;
            commands::clipboard::start_poll(handle.clone());

            // 界面语言切换时重建托盘菜单
            let h = handle.clone();
            app.listen("i18n://locale-changed", move |event| {
                if let Ok(locale) = serde_json::from_str::<String>(event.payload()) {
                    let _ = tray::rebuild_tray(&h, &locale);
                }
            });

            handle.plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, _shortcut, event| {
                        if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                            tray::toggle_main(app);
                        }
                    })
                    .build(),
            )?;

            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            app.global_shortcut().register("CmdOrCtrl+Shift+P")?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
