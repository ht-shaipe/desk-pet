import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";

export const PET_SIZE = 72;
export const PANEL_WIDTH = 360;
export const PANEL_HEIGHT = 560;

const win = getCurrentWindow();

interface SavedPos {
  x: number;
  y: number;
}

let savedPetPos: SavedPos | null = null;

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

/** 折叠态 -> 展开态：窗口放大为面板尺寸，屏幕边缘时自动钳制位置。 */
export async function expandWindow(): Promise<void> {
  const sf = await scaleFactor();
  const pos = await win.outerPosition();
  savedPetPos = { x: pos.x, y: pos.y };

  const w = Math.round(PANEL_WIDTH * sf);
  const h = Math.round(PANEL_HEIGHT * sf);
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
  await win.setSize(new PhysicalSize(w, h));
}

/** 展开态 -> 折叠态：恢复展开前记录的精灵位置。 */
export async function collapseWindow(): Promise<void> {
  const sf = await scaleFactor();
  const size = Math.round(PET_SIZE * sf);
  if (savedPetPos) {
    await win.setPosition(new PhysicalPosition(savedPetPos.x, savedPetPos.y));
  }
  await win.setSize(new PhysicalSize(size, size));
}
