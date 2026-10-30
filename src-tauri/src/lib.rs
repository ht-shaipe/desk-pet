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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            tauri_cmds::settings_cmds::get_settings,
            tauri_cmds::settings_cmds::save_settings,
            tauri_cmds::settings_cmds::set_pet_avatar,
            tauri_cmds::settings_cmds::reset_pet_avatar,
            tauri_cmds::launcher_cmds::get_launcher,
            tauri_cmds::launcher_cmds::save_launcher,
            tauri_cmds::launcher_cmds::launch_item,
            tauri_cmds::launcher_cmds::get_app_icon,
            tauri_cmds::launcher_cmds::get_app_icon_by_value,
            tauri_cmds::launcher_cmds::list_apps,
            tauri_cmds::chat_cmds::get_chat_messages,
            tauri_cmds::chat_cmds::add_chat_message,
            tauri_cmds::chat_cmds::delete_chat_message,
            tauri_cmds::chat_cmds::clear_chat_messages,
            tauri_cmds::clipboard_cmds::get_clipboard_history,
            tauri_cmds::clipboard_cmds::clear_clipboard_history,
            tauri_cmds::clipboard_cmds::delete_clipboard_entry,
            tauri_cmds::clipboard_cmds::delete_clipboard_entries,
            tauri_cmds::clipboard_cmds::copy_to_clipboard,
            tauri_cmds::clipboard_cmds::copy_image_to_clipboard,
            tauri_cmds::clipboard_cmds::get_clip_image_dir,
            tauri_cmds::favorite_cmds::get_favorites,
            tauri_cmds::favorite_cmds::upsert_favorites,
            tauri_cmds::favorite_cmds::delete_favorites,
            tauri_cmds::favorite_cmds::clear_favorites,
            tauri_cmds::favorite_cmds::read_favorite_import_file,
            tauri_cmds::favorite_cmds::write_favorite_export_file,
            tauri_cmds::system_cmds::quick_action,
            tauri_cmds::system_cmds::get_platform,
            tauri_cmds::system_cmds::set_show_dock,
            tauri_cmds::pointer_cmds::pointer_state,
            tauri_cmds::toolbox_cmds::get_clipboard_image,
            tauri_cmds::toolbox_cmds::set_clipboard_image,
            tauri_cmds::toolbox_cmds::save_qr_png,
            tauri_cmds::toolbox_cmds::save_image,
            tauri_cmds::toolbox_cmds::list_image_files,
            tauri_cmds::toolbox_cmds::read_image_data_url,
            tauri_cmds::toolbox_cmds::save_compressed_image,
            tauri_cmds::toolbox_cmds::reveal_path,
            tauri_cmds::toolbox_cmds::http_request,
            tauri_cmds::cleanup_cmds::disk_cleanup_scan,
            tauri_cmds::cleanup_cmds::disk_cleanup_delete,
            tauri_cmds::cleanup_cmds::disk_cleanup_cancel,
            tauri_cmds::cleanup_cmds::disk_cleanup_usage,
        ])
        .setup(|app| {
            use tauri::{Listener, Manager};

            let handle = app.handle();
            let dir = paths::data_dir(handle)?;
            let store = deskpet_store::Store::open(&dir)?;
            handle.manage(state::AppState::new(store));

            // 窗口最外层圆角：卡片 CSS 阴影延伸进透明边距后被方形窗口边界硬裁，
            // 底部露出直角半透明带（阴影向下偏移，底边最明显）。把窗口内容层裁成
            // 圆角，半径与卡片圆角 + 边距同心（主窗 16+8=24，通知窗卡片贴边 20）。
            #[cfg(target_os = "macos")]
            for (label, radius) in [("main", 24.0), ("notify", 20.0)] {
                if let Some(win) = handle.get_webview_window(label) {
                    if let Ok(ptr) = win.ns_window() {
                        unsafe { deskpet_os::window::round_window(ptr, radius) };
                    }
                }
            }

            let mut settings = deskpet_store::settings::load(&dir);
            // 旧版聊天历史存于 settings.json，启动时迁入 SQLite 并从设置中清除
            {
                let state = handle.state::<state::AppState>();
                let st = state.store.lock().unwrap();
                if st.migrate_chat_messages(&settings.ai_messages).unwrap_or(0) > 0 {
                    settings.ai_messages.clear();
                    let _ = deskpet_store::settings::save(&dir, &settings);
                }
            }
            let locale = settings.locale.clone().unwrap_or_else(|| "zh-CN".to_string());
            let app_name = settings.app_name.clone().unwrap_or_else(|| "Desk Pet".to_string());

            // 启动时应用「在 Dock 中显示」设置（仅 macOS；仅在用户明确隐藏时才改激活策略）
            // 切换激活策略会隐藏/降级主窗口，需要重新拉起精灵窗口
            #[cfg(target_os = "macos")]
            if settings.show_dock == Some(false) {
                let _ = handle.set_dock_visibility(false);
                if let Some(main) = handle.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_always_on_top(true);
                    let _ = main.set_visible_on_all_workspaces(true);
                }
            }
            // notify 提醒窗口启动时强制隐藏（macOS 透明窗口有创建时短暂可见的怪癖，
            // 防止一块全透明区域遮挡点击）
            if let Some(notify_win) = handle.get_webview_window("notify") {
                let _ = notify_win.hide();
            }
            tray::create_tray(handle, &locale, &app_name)?;
            tauri_cmds::clipboard_cmds::start_poll(handle.clone());

            // 界面语言切换时重建托盘菜单
            let h = handle.clone();
            app.listen("i18n://locale-changed", move |event| {
                if let Ok(locale) = serde_json::from_str::<String>(event.payload()) {
                    let dir = paths::data_dir(&h).unwrap_or_default();
                    let name = deskpet_store::settings::load(&dir)
                        .app_name
                        .unwrap_or_else(|| "Desk Pet".to_string());
                    let _ = tray::rebuild_tray(&h, &locale, &name);
                }
            });

            // 应用名称变更时更新托盘提示
            let h = handle.clone();
            app.listen("app://name-changed", move |event| {
                if let Ok(name) = serde_json::from_str::<String>(event.payload()) {
                    let locale = {
                        let dir = paths::data_dir(&h).unwrap_or_default();
                        deskpet_store::settings::load(&dir)
                            .locale
                            .unwrap_or_else(|| "zh-CN".to_string())
                    };
                    let _ = tray::rebuild_tray(&h, &locale, &name);
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
