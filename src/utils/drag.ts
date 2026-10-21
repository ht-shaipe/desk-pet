import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();
const DRAG_THRESHOLD = 4;

interface DragHandlers {
  onClick: () => void;
}

export interface PetDragHandlers {
  onPointerdown: (e: PointerEvent) => void;
  onPointermove: (e: PointerEvent) => void;
  onPointerup: (e: PointerEvent) => void;
  onPointercancel: (e: PointerEvent) => void;
}

/**
 * 精灵拖拽与点击判定：
 * - 按下后位移超过阈值 -> 调用原生 startDragging 拖动窗口；
 * - 未产生位移即抬起 -> 触发 onClick。
 * startDragging 后 webview 可能收不到 pointerup，
 * 内部状态在下次 pointerdown 时会自愈复位。
 */
export function usePetDrag(handlers: DragHandlers): PetDragHandlers {
  const state = { pending: false, dragged: false, startX: 0, startY: 0 };
  return {
    onPointerdown(e) {
      state.pending = true;
      state.dragged = false;
      state.startX = e.clientX;
      state.startY = e.clientY;
      const el = e.currentTarget;
      if (el instanceof HTMLElement) {
        el.setPointerCapture(e.pointerId);
      }
    },
    async onPointermove(e) {
      if (!state.pending || state.dragged) {
        return;
      }
      const dx = e.clientX - state.startX;
      const dy = e.clientY - state.startY;
      if (Math.sqrt(dx * dx + dy * dy) > DRAG_THRESHOLD) {
        state.dragged = true;
        try {
          await win.startDragging();
        } catch {
          // 忽略原生拖拽调用失败
        }
      }
    },
    onPointerup() {
      if (state.pending && !state.dragged) {
        handlers.onClick();
      }
      state.pending = false;
      state.dragged = false;
    },
    onPointercancel() {
      state.pending = false;
      state.dragged = false;
    },
  };
}
