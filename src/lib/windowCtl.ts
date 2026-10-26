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
