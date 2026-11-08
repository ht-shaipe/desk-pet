//! 系统监控采样命令（工具箱「系统监控」工具）与悬浮监控条窗口。

use deskpet_os::menubar::ChipSections;
use deskpet_os::monitor::SystemSample;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::state::AppState;

/// 悬浮监控条窗口 label。
pub const MONITORBAR_LABEL: &str = "monitorbar";

/// 采样一次系统运行状态：CPU 使用率、内存、网络上下行速率、磁盘卷空间。
#[tauri::command]
pub async fn monitor_sample(app: AppHandle) -> Result<SystemSample, String> {
    // CPU 使用率需相邻两次刷新间隔 ≥ MINIMUM_CPU_UPDATE_INTERVAL，可能短暂阻塞，
    // 同步命令会占用主线程，丢到阻塞线程池执行
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.monitor.lock().unwrap().sample()
    })
    .await
    .map_err(|err| err.to_string())
}

/// 切换悬浮监控条：已开则关闭并返回 false，未开则创建并返回 true。
/// 成功后广播 `monitorbar://state` 事件（主窗口据此持久化开关，托盘/工具页共用）。
#[tauri::command]
pub async fn monitorbar_toggle(app: AppHandle) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window(MONITORBAR_LABEL) {
        let _ = window.close();
        let _ = app.emit("monitorbar://state", false);
        return Ok(false);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        // 模板同 updater_cmds：无边框透明置顶小窗；HUD 形态不抢焦点、跨工作区可见。
        // 内容宽度随分区开关变化，由悬浮条窗口前端按实际渲染尺寸自适应修正。
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            MONITORBAR_LABEL,
            WebviewUrl::App("index.html?view=monitorbar".into()),
        )
        .title("系统监控")
        .inner_size(220.0, 36.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible_on_all_workspaces(true)
        .build()
        .is_ok();
        if let Some(window) = app_for_window.get_webview_window(MONITORBAR_LABEL) {
            // 无边框透明窗：内容层裁胶囊圆角
            #[cfg(target_os = "macos")]
            if let Ok(ns_window) = window.ns_window() {
                unsafe {
                    deskpet_os::window::round_window(ns_window, 8.0);
                }
            }
            let _ = window.show();
        }
        let _ = tx.send(created);
    })
    .map_err(|err| err.to_string())?;
    let open = rx.await.map_err(|err| err.to_string())?;
    let _ = app.emit("monitorbar://state", open);
    Ok(open)
}

/// 更新菜单栏监控（macOS：采样绘制为双行芯片图显示在状态项上，只渲染 `sections`
/// 启用的分区（与悬浮条共用同一组开关）；sample 为空即清除）。
/// `show_icon` 控制监控显示期间是否保留桌宠托盘图标。
#[tauri::command]
pub async fn monitor_menubar_set(
    app: AppHandle,
    sample: Option<SystemSample>,
    show_icon: bool,
    sections: Option<ChipSections>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let app_for_tray = app.clone();
        app.run_on_main_thread(move || {
            let res = match &sample {
                Some(s) => deskpet_os::menubar::set_chip(s, &sections.unwrap_or_default()),
                None => deskpet_os::menubar::hide(),
            };
            if let Err(err) = res {
                eprintln!("[monitor] 菜单栏状态项更新失败: {err}");
            }
            if let Some(tray) = app_for_tray.tray_by_id("desk-pet-tray") {
                let _ = tray.set_visible(sample.is_none() || show_icon);
            }
        })
        .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, sample, show_icon, sections);
        Ok(())
    }
}
