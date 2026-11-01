//! 更新提示弹窗：面板折叠态检测到新版本时，用独立小窗提示并承载升级操作。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 打开（或聚焦）更新提示弹窗。返回是否为新创建。
#[tauri::command]
pub async fn updater_open(app: AppHandle) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            "updater",
            WebviewUrl::App("index.html?view=updater".into()),
        )
        .title("软件更新")
        .inner_size(380.0, 216.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        .is_ok();
        if let Some(window) = app_for_window.get_webview_window("updater") {
            // 无边框透明窗：内容层裁圆角（与主面板/分析窗口同一套外观语言）。
            // ns_window 是 macOS 专属 API，Windows 由系统自动圆角，无需处理。
            #[cfg(target_os = "macos")]
            if let Ok(ns_window) = window.ns_window() {
                unsafe {
                    deskpet_os::window::round_window(ns_window, 14.0);
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
