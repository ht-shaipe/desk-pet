//! 全局指针状态（macOS）：光标位置 + 左键是否按下。
//!
//! 用途：面板窗口边缘/角落的手动 resize。窗口被程序化移动（setPosition）后
//! WebKit 的指针事件流可能中断，前端改为逐帧轮询这里的原生状态来驱动几何更新。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PointerState {
    /// 光标全局显示坐标（顶左原点，points），与 DOM screenX/Y 同向
    pub x: f64,
    pub y: f64,
    /// 鼠标左键当前是否按下
    pub down: bool,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::PointerState;
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    unsafe extern "C" {
        // core-graphics crate 未封装该符号，直接声明；CoreGraphics 由依赖链链接
        unsafe fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    }

    pub fn pointer_state() -> PointerState {
        let (x, y) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .ok()
            .and_then(|source| CGEvent::new(source).ok())
            .map(|event| {
                let point = event.location();
                (point.x, point.y)
            })
            .unwrap_or((0.0, 0.0));
        // kCGEventSourceStateCombinedSessionState = 0，kCGMouseButtonLeft = 0
        let down = unsafe { CGEventSourceButtonState(0, 0) };
        PointerState { x, y, down }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::PointerState;

    pub fn pointer_state() -> PointerState {
        PointerState {
            x: 0.0,
            y: 0.0,
            down: false,
        }
    }
}

pub use imp::pointer_state;

#[cfg(test)]
mod tests {
    use super::pointer_state;

    #[test]
    fn pointer_state_returns_finite_coords() {
        let state = pointer_state();
        assert!(state.x.is_finite() && state.y.is_finite());
    }
}
