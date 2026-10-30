use deskpet_os::pointer::pointer_state as native_pointer_state;

/// 全局指针状态：光标位置（全局显示坐标，顶左原点）+ 左键是否按下。
/// 面板边缘/角落手动 resize 逐帧轮询用，替代可能中断的 DOM 指针事件流。
#[tauri::command]
pub fn pointer_state() -> deskpet_os::pointer::PointerState {
    native_pointer_state()
}
