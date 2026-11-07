import { listen } from "@tauri-apps/api/event";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { i18n } from "@/i18n";

/** 番茄钟完成提醒浮层：接收主窗口事件，屏幕上方显示大字，点击/按 Esc 关闭。
 * 同时承载 gRPC 通道推送提醒（channel://push-notice，标题缺省用「新消息」）。 */

interface SessionDonePayload {
  title: string;
  message: string;
  /** 消息时间戳（秒，服务端 server_time）；缺省用当前时间 */
  time?: number;
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
  timeEl: HTMLElement,
  closeBtn: HTMLElement
) {
  /** 把时间戳（秒）格式化为完整日期时间 yyyy-MM-dd HH:mm:ss；无效值回退当前时间。 */
  function formatTime(ts?: number): string {
    const d = new Date((ts && ts > 0 ? ts : Date.now() / 1000) * 1000);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }

  /** 按显示器缩放比例把窗口放到主屏上方约 24% 处并显示。 */
  async function showOverlay(payload: SessionDonePayload) {
    titleEl.textContent = payload.title || i18n.global.t("channel.newMessage");
    messageEl.textContent = payload.message;
    timeEl.textContent = formatTime(payload.time);
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

  void listen<SessionDonePayload>("channel://push-notice", (event) => {
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
const timeEl = document.getElementById("time");
const closeBtn = document.getElementById("close");

if (card && titleEl && messageEl && hintEl && timeEl && closeBtn) {
  setup(card, titleEl, messageEl, hintEl, timeEl, closeBtn);
}
