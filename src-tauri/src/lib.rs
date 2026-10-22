mod paths;
mod state;
mod tauri_cmds;
mod tray;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            tauri_cmds::settings_cmds::get_settings,
            tauri_cmds::settings_cmds::save_settings,
            tauri_cmds::settings_cmds::set_pet_avatar,
            tauri_cmds::settings_cmds::reset_pet_avatar,
            tauri_cmds::launcher_cmds::get_launcher,
            tauri_cmds::launcher_cmds::save_launcher,
            tauri_cmds::launcher_cmds::launch_item,
            tauri_cmds::notes_cmds::get_notes,
            tauri_cmds::notes_cmds::save_notes,
            tauri_cmds::clipboard_cmds::get_clipboard_history,
            tauri_cmds::clipboard_cmds::clear_clipboard_history,
            tauri_cmds::clipboard_cmds::delete_clipboard_entry,
            tauri_cmds::clipboard_cmds::copy_to_clipboard,
            tauri_cmds::system_cmds::quick_action,
            tauri_cmds::system_cmds::get_platform,
            tauri_cmds::toolbox_cmds::get_clipboard_image,
            tauri_cmds::toolbox_cmds::set_clipboard_image,
            tauri_cmds::toolbox_cmds::save_qr_png,
            tauri_cmds::toolbox_cmds::save_image,
        ])
        .setup(|app| {
            use tauri::{Listener, Manager};

            let handle = app.handle();
            let dir = paths::data_dir(handle)?;
            let store = deskpet_store::Store::open(&dir)?;
            handle.manage(state::AppState::new(store));

            let settings = deskpet_store::settings::load(&dir);
            let locale = settings.locale.unwrap_or_else(|| "zh-CN".to_string());
            tray::create_tray(handle, &locale)?;
            tauri_cmds::clipboard_cmds::start_poll(handle.clone());

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
