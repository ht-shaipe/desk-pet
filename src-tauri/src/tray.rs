use tauri::{AppHandle, Manager};

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn toggle_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(true) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

/// 按语言返回托盘文案：(显示/隐藏, 退出, tooltip)。
fn tray_texts(locale: &str) -> (&'static str, &'static str, &'static str) {
    if locale.starts_with("zh") {
        ("显示/隐藏精灵", "退出", "Desk Pet 桌面精灵")
    } else {
        ("Show / Hide Pet", "Quit", "Desk Pet")
    }
}

fn build_menu(app: &AppHandle, locale: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};

    let (toggle_s, quit_s, _) = tray_texts(locale);
    let toggle_item = MenuItemBuilder::with_id("toggle", toggle_s).build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", quit_s).build(app)?;

    MenuBuilder::new(app)
        .item(&toggle_item)
        .item(&quit_item)
        .build()
}

pub fn create_tray(app: &AppHandle, locale: &str) -> tauri::Result<()> {
    use tauri::tray::TrayIconBuilder;

    let menu = build_menu(app, locale)?;
    let (_, _, tooltip_s) = tray_texts(locale);

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("default window icon missing");

    let _tray = TrayIconBuilder::with_id("desk-pet-tray")
        .icon(icon)
        .tooltip(tooltip_s)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => toggle_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// 语言切换时重建托盘菜单与提示文案。
pub fn rebuild_tray(app: &AppHandle, locale: &str) -> tauri::Result<()> {
    let menu = build_menu(app, locale)?;
    let (_, _, tooltip_s) = tray_texts(locale);
    if let Some(tray) = app.tray_by_id("desk-pet-tray") {
        tray.set_menu(Some(menu))?;
        let _ = tray.set_tooltip(Some(tooltip_s));
    }
    Ok(())
}
