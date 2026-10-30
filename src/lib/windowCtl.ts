import { invoke } from "@tauri-apps/api/core";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";

export const PET_SIZE = 72;
export const PANEL_WIDTH = 388;
export const PANEL_HEIGHT = 588;
const MIN_PANEL_WIDTH = 300;
const MIN_PANEL_HEIGHT = 420;

const win = getCurrentWindow();

interface SavedPos {
  x: number;
  y: number;
}

interface SavedSize {
  width: number;
  height: number;
}

let savedPetPos: SavedPos | null = null;
let savedPanelSize: SavedSize | null = null;
/** 展开时面板的落点（物理像素）；收起时与当前位置对比判断是否拖动过。 */
let expandedPanelPos: SavedPos | null = null;

/** 展开态记录用户手动调整后的面板尺寸（物理像素），下次展开沿用。 */
export function rememberPanelSize(size: SavedSize): void {
  savedPanelSize = size;
}

/** 与 @tauri-apps/api/window 的 ResizeDirection 一致（该模块未导出类型）。 */
export type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

/** macOS 上 tao 未实现 drag_resize_window：startResizeDragging 恒返回 NotSupported，
 *  且错误在 runtime 层被丢弃（promise 正常 resolve），无法事后感知，只能按平台分流。 */
const IS_MAC = navigator.userAgent.includes("Macintosh");

const RESIZE_EDGES: Record<ResizeDirection, { east?: boolean; west?: boolean; north?: boolean; south?: boolean }> = {
  East: { east: true },
  North: { north: true },
  NorthEast: { east: true, north: true },
  NorthWest: { west: true, north: true },
  South: { south: true },
  SouthEast: { east: true, south: true },
  SouthWest: { west: true, south: true },
  West: { west: true },
};

/** pointer_state 命令返回的全局指针状态（顶左原点，points）。 */
interface PointerState {
  x: number;
  y: number;
  down: boolean;
}

/** 边缘/四角手柄按下时进入对应方向的 resize 拖拽。
 *  非 macOS 走原生 startResizeDragging；macOS 原生未实现，手动驱动。 */
export function startWindowResize(direction: ResizeDirection, event: PointerEvent): void {
  if (!IS_MAC) {
    void win.startResizeDragging(direction).catch(() => undefined);
    return;
  }
  beginManualResize(direction, event);
}

/** macOS 手动 resize 会话进行中标记（防重入：角/边手柄存在重叠区）。 */
let manualResizeActive = false;

/** macOS 手动 resize：窗口被程序化移动（setPosition）后 WebKit 的指针事件流
 *  会中断（现象：含 N/W 方向的拖动一拖即停，纯改尺寸的 E/S/SE 不受影响），
 *  因此拖动期间不依赖 DOM 事件，逐帧轮询原生 pointer_state（光标位置 + 左键状态）
 *  驱动几何更新；DOM 的 pointerup/pointercancel 仅作为正常情况下的快速结束通道。 */
function beginManualResize(direction: ResizeDirection, event: PointerEvent): void {
  if (manualResizeActive) {
    return;
  }
  manualResizeActive = true;

  const target = event.currentTarget;
  if (target instanceof HTMLElement) {
    try {
      // 捕获仅用于拖动期间保持手柄光标样式，几何更新不依赖 DOM 事件
      target.setPointerCapture(event.pointerId);
    } catch {
      // 捕获失败不影响 resize 本身
    }
  }

  const edges = RESIZE_EDGES[direction];
  let ended = false;
  let timer = 0;

  const finish = () => {
    if (ended) {
      return;
    }
    ended = true;
    manualResizeActive = false;
    if (timer) {
      clearTimeout(timer);
    }
  };
  // 事件流正常时直接在松开时结束；中断时由轮询发现左键抬起兜底
  window.addEventListener("pointerup", finish, { once: true });
  window.addEventListener("pointercancel", finish, { once: true });

  void (async () => {
    try {
      const [sf, pos, size] = await Promise.all([scaleFactor(), win.outerPosition(), win.outerSize()]);
      const first = await invoke<PointerState>("pointer_state");
      if (ended) {
        return;
      }
      if (!first.down) {
        finish();
        return;
      }
      const start = {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
        // 起点取首次轮询的光标位置（不依赖 pointerdown 的 screenX，规避 WKWebView 差异）
        px: first.x,
        py: first.y,
      };
      const minW = Math.round(MIN_PANEL_WIDTH * sf);
      const minH = Math.round(MIN_PANEL_HEIGHT * sf);

      /** 按方向反推新几何：E/S 只改尺寸，含 N/W 时先平移原点钉住对边再缩放。 */
      const apply = (st: PointerState) => {
        const dx = Math.round((st.x - start.px) * sf);
        const dy = Math.round((st.y - start.py) * sf);
        let width = start.w;
        let height = start.h;
        let x = start.x;
        let y = start.y;
        if (edges.east) {
          width = Math.max(minW, start.w + dx);
        }
        if (edges.south) {
          height = Math.max(minH, start.h + dy);
        }
        if (edges.west) {
          width = Math.max(minW, start.w - dx);
          x = start.x + (start.w - width);
        }
        if (edges.north) {
          height = Math.max(minH, start.h - dy);
          y = start.y + (start.h - height);
        }
        if (width === start.w && height === start.h && x === start.x && y === start.y) {
          return;
        }
        if (x !== start.x || y !== start.y) {
          void win.setPosition(new PhysicalPosition(x, y));
        }
        void win.setSize(new PhysicalSize(width, height));
      };

      let failures = 0;
      const step = () => {
        if (ended) {
          return;
        }
        void invoke<PointerState>("pointer_state")
          .then((st) => {
            if (ended) {
              return;
            }
            failures = 0;
            if (!st.down) {
              finish();
              return;
            }
            apply(st);
            timer = window.setTimeout(step, 8);
          })
          .catch(() => {
            failures += 1;
            // IPC 持续失败（异常场景）时退出，避免空转
            if (failures > 20) {
              finish();
              return;
            }
            timer = window.setTimeout(step, 16);
          });
      };
      step();
    } catch {
      finish();
    }
  })();
}

async function scaleFactor(): Promise<number> {
  const sf = await win.scaleFactor();
  return sf > 0 ? sf : 1;
}

/** 启动时恢复精灵位置；无记录则默认放在主显示器右下角。 */
export async function initWindowPosition(
  hasSaved: boolean,
  x: number | null,
  y: number | null
): Promise<void> {
  const sf = await scaleFactor();
  const size = Math.round(PET_SIZE * sf);
  const mon = await currentMonitor();
  if (!mon) {
    return;
  }
  if (hasSaved && x !== null && y !== null) {
    const px = Math.min(
      Math.max(x, mon.position.x),
      mon.position.x + Math.max(0, mon.size.width - size)
    );
    const py = Math.min(
      Math.max(y, mon.position.y),
      mon.position.y + Math.max(0, mon.size.height - size)
    );
    await win.setPosition(new PhysicalPosition(px, py));
  } else {
    const margin = Math.round(32 * sf);
    const bottomGap = Math.round(96 * sf);
    const px = mon.position.x + mon.size.width - size - margin;
    const py = mon.position.y + mon.size.height - size - bottomGap;
    await win.setPosition(new PhysicalPosition(px, py));
  }
}

/** 折叠态 -> 展开态：窗口放大为面板尺寸（沿用用户上次调整的大小），屏幕边缘时自动钳制位置。 */
export async function expandWindow(): Promise<void> {
  const sf = await scaleFactor();
  const pos = await win.outerPosition();
  savedPetPos = { x: pos.x, y: pos.y };

  const w = savedPanelSize?.width ?? Math.round(PANEL_WIDTH * sf);
  const h = savedPanelSize?.height ?? Math.round(PANEL_HEIGHT * sf);
  let x = pos.x;
  let y = pos.y;

  const mon = await currentMonitor();
  if (mon) {
    x = Math.min(x, mon.position.x + mon.size.width - w);
    y = Math.min(y, mon.position.y + mon.size.height - h);
    x = Math.max(x, mon.position.x);
    y = Math.max(y, mon.position.y);
  }

  await win.setPosition(new PhysicalPosition(x, y));
  expandedPanelPos = { x, y };
  await win.setSize(new PhysicalSize(w, h));
  await win.setMinSize(
    new PhysicalSize(Math.round(MIN_PANEL_WIDTH * sf), Math.round(MIN_PANEL_HEIGHT * sf))
  );
  await win.setResizable(true);
  // 隐藏 Dock（accessory）模式下点击不会自动激活应用，键盘输入会失效，
  // 展开面板即代表用户要交互，这里主动取焦点
  await win.setFocus();
}

/** 展开态 -> 折叠态：面板被拖动过就地收起（精灵显示在面板最新位置的左上角），
 *  未拖动则恢复展开前记录的精灵位置；还原为不可调整的小窗。 */
export async function collapseWindow(): Promise<void> {
  const sf = await scaleFactor();
  const size = Math.round(PET_SIZE * sf);
  // 先解除最小尺寸并缩小窗口，最后才锁定 resizable（macOS 上先锁再缩可能失效）
  await win.setMinSize(null);
  const pos = await win.outerPosition();
  // 与展开落点对比（1px 容差吸收系统取整误差）：不一致说明用户拖动过面板
  const dragged =
    expandedPanelPos !== null &&
    (Math.abs(pos.x - expandedPanelPos.x) > 1 || Math.abs(pos.y - expandedPanelPos.y) > 1);
  if (dragged) {
    let x = pos.x;
    let y = pos.y;
    const mon = await currentMonitor();
    if (mon) {
      x = Math.min(Math.max(x, mon.position.x), mon.position.x + Math.max(0, mon.size.width - size));
      y = Math.min(Math.max(y, mon.position.y), mon.position.y + Math.max(0, mon.size.height - size));
    }
    await win.setPosition(new PhysicalPosition(x, y));
  } else if (savedPetPos) {
    await win.setPosition(new PhysicalPosition(savedPetPos.x, savedPetPos.y));
  }
  await win.setSize(new PhysicalSize(size, size));
  await win.setResizable(false);
}
