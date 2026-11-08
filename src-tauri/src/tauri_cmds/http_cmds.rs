//! 接口请求:独立窗口(Rust 代理发送逻辑在 toolbox_cmds/deskpet-os,此处仅负责窗口)。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 打开(或聚焦)接口请求窗口,返回是否为新创建。
#[tauri::command]
pub async fn http_open(app: AppHandle) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            "http",
            WebviewUrl::App("index.html?view=http".into()),
        )
        .title("接口请求")
        .inner_size(980.0, 680.0)
        .min_inner_size(760.0, 560.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .build()
        .is_ok();
        if let Some(window) = app_for_window.get_webview_window("http") {
            #[cfg(target_os = "macos")]
            if let Ok(ns_window) = window.ns_window() {
                unsafe {
                    deskpet_os::window::round_window(ns_window, 12.0);
                }
            }
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = tx.send(created);
    })
    .map_err(|err| err.to_string())?;
    rx.await.map_err(|err| err.to_string())
}
