import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

export type PomoMode = "focus" | "short" | "long";

export const MODE_MINUTES: Record<PomoMode, number> = { focus: 25, short: 5, long: 15 };
export const MODE_LABELS: Record<PomoMode, string> = { focus: "专注", short: "短休息", long: "长休息" };

async function notify(title: string, body: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch {
    // 通知不可用时静默忽略
  }
}

export const usePomodoroStore = defineStore("pomodoro", () => {
  const mode = ref<PomoMode>("focus");
  const running = ref(false);
  const remainingMs = ref(MODE_MINUTES.focus * 60 * 1000);
  const focusDone = ref(0);
  const endAt = ref(0);

  let timer: ReturnType<typeof setInterval> | undefined;

  const display = computed(() => {
    const totalSec = Math.max(0, Math.ceil(remainingMs.value / 1000));
    const m = Math.floor(totalSec / 60);
    const s = totalSec % 60;
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  });

  function ensureTimer() {
    if (timer !== undefined) {
      return;
    }
    timer = setInterval(tick, 500);
  }

  /** 用时间戳计算剩余时间，规避 WebView 定时器节流导致的漂移。 */
  function tick() {
    if (!running.value) {
      return;
    }
    remainingMs.value = endAt.value - Date.now();
    if (remainingMs.value <= 0) {
      finish();
    }
  }

  function start() {
    endAt.value = Date.now() + remainingMs.value;
    running.value = true;
    ensureTimer();
  }

  function pause() {
    running.value = false;
  }

  function reset() {
    running.value = false;
    remainingMs.value = MODE_MINUTES[mode.value] * 60 * 1000;
  }

  function setMode(m: PomoMode) {
    mode.value = m;
    running.value = false;
    remainingMs.value = MODE_MINUTES[m] * 60 * 1000;
  }

  function finish() {
    running.value = false;
    if (mode.value === "focus") {
      focusDone.value += 1;
      const next: PomoMode = focusDone.value % 4 === 0 ? "long" : "short";
      void notify("番茄钟", "专注结束，休息一下吧！");
      setMode(next);
    } else {
      void notify("番茄钟", "休息结束，开始新的专注！");
      setMode("focus");
    }
  }

  return { mode, running, remainingMs, focusDone, display, start, pause, reset, setMode };
});
