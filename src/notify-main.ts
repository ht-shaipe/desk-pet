import { listen } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { i18n } from "@/i18n";

/** 番茄钟完成提醒浮层：接收主窗口事件，屏幕上方显示大字，点击/按 Esc 关闭。 */

interface SessionDonePayload {
  title: string;
  message: string;
}

const LOGICAL_W = 460;
const LOGICAL_H = 190;

const win = getCurrentWindow();

// 禁用提醒浮层的默认右键菜单
document.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });

function setup(
  card: HTMLElement,
  titleEl: HTMLElement,
  messageEl: HTMLElement,
  hintEl: HTMLElement,
  closeBtn: HTMLElement
) {
  /** 按显示器缩放比例把窗口放到主屏上方约 24% 处并显示。 */
  async function showOverlay(payload: SessionDonePayload) {
    titleEl.textContent = payload.title;
    messageEl.textContent = payload.message;
    hintEl.textContent = i18n.global.t("pomodoro.overlayHint");
    card.classList.remove("hidden");
    try {
      const sf = await win.scaleFactor();
      const w = Math.round(LOGICAL_W * sf);
      const h = Math.round(LOGICAL_H * sf);
      await win.setSize(new PhysicalSize(w, h));
      const mon = await currentMonitor();
      if (mon) {
        const x = mon.position.x + Math.round((mon.size.width - w) / 2);
        const y = mon.position.y + Math.round(mon.size.height * 0.24);
        await win.setPosition(new PhysicalPosition(x, y));
      }
    } catch {
      // 定位失败时仍直接显示
    }
    await win.show();
  }

  async function hideOverlay() {
    card.classList.add("hidden");
    await win.hide();
  }

  void listen<SessionDonePayload>("pomodoro://session-done", (event) => {
    void showOverlay(event.payload);
  });

  closeBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    void hideOverlay();
  });
  // 点击卡片任意处关闭
  card.addEventListener("click", () => {
    void hideOverlay();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      void hideOverlay();
    }
  });
}

const card = document.getElementById("card");
const titleEl = document.getElementById("title");
const messageEl = document.getElementById("message");
const hintEl = document.getElementById("hint");
const closeBtn = document.getElementById("close");

if (card && titleEl && messageEl && hintEl && closeBtn) {
  setup(card, titleEl, messageEl, hintEl, closeBtn);
}
