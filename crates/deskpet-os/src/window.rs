//! 窗口外观辅助（macOS）。

use objc2::msg_send;
use objc2::runtime::AnyObject;

/// 把 NSWindow 内容层裁成圆角。
///
/// 面板/通知卡片的 CSS 阴影会延伸进窗口四周的透明边距，被方形窗口边界硬裁出
/// 直角半透明带（阴影向下偏移，底边最明显）；裁剪后窗体最外层轮廓变为圆角，
/// 阴影带随之圆角收边。`radius_points` 应与页面卡片圆角 + 内缩边距同心：
/// 主窗口 16 + 8 = 24，通知窗口卡片贴边 20 → 20。
///
/// # Safety
/// `ns_window` 必须是有效的 NSWindow 指针（Tauri `ns_window()` 的返回值），
/// 且窗口已创建。
pub unsafe fn round_window(ns_window: *mut std::ffi::c_void, radius_points: f64) {
    let window = ns_window as *mut AnyObject;
    let content: *mut AnyObject = msg_send![window, contentView];
    if content.is_null() {
        return;
    }
    let mut layer: *mut AnyObject = msg_send![content, layer];
    if layer.is_null() {
        // 非层守护视图先开启层守护再取层（WKWebView 本身层守护，通常不会走到）
        let _: () = msg_send![content, setWantsLayer: true];
        layer = msg_send![content, layer];
    }
    if layer.is_null() {
        return;
    }
    let _: () = msg_send![layer, setCornerRadius: radius_points];
    let _: () = msg_send![layer, setMasksToBounds: true];
}
