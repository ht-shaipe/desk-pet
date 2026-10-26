import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { emit } from "@tauri-apps/api/event";
import { i18n } from "@/i18n";
import { useConfigStore } from "./config";

export type PomoMode = "focus" | "short" | "long";

export const DEFAULT_MINUTES: Record<PomoMode, number> = { focus: 25, short: 5, long: 15 };

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
  const config = useConfigStore();
  const mode = ref<PomoMode>("focus");
  const running = ref(false);
  const remainingMs = ref(DEFAULT_MINUTES.focus * 60 * 1000);
  const focusDone = ref(0);
  const endAt = ref(0);

  let timer: ReturnType<typeof setInterval> | undefined;

  const minutes = computed<Record<PomoMode, number>>(() => ({
    focus: config.settings.pomo_focus_min ?? DEFAULT_MINUTES.focus,
    short: config.settings.pomo_short_min ?? DEFAULT_MINUTES.short,
    long: config.settings.pomo_long_min ?? DEFAULT_MINUTES.long,
  }));

  const display = computed(() => {
    const totalSec = Math.max(0, Math.ceil(remainingMs.value / 1000));
    const m = Math.floor(totalSec / 60);
    const s = totalSec % 60;
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  });

  /** 未运行时，自定义时长变更后同步剩余时间。 */
  watch(minutes, (val) => {
    if (!running.value) {
      remainingMs.value = val[mode.value] * 60 * 1000;
    }
  }, { deep: true, immediate: true });

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
    remainingMs.value = minutes.value[mode.value] * 60 * 1000;
  }

  function setMode(m: PomoMode) {
    mode.value = m;
    running.value = false;
    remainingMs.value = minutes.value[m] * 60 * 1000;
  }

  function setMinutes(m: PomoMode, value: number) {
    config.setPomoMinutes(m, value);
  }

  function finish() {
    running.value = false;
    const t = i18n.global.t;
    if (mode.value === "focus") {
      focusDone.value += 1;
      const next: PomoMode = focusDone.value % 4 === 0 ? "long" : "short";
      void notify(t("pomodoro.notifTitle"), t("pomodoro.notifFocusDone"));
      // 屏幕大字提醒浮层（notify 窗口监听）
      void emit("pomodoro://session-done", {
        title: t("pomodoro.overlayFocusTitle"),
        message: t("pomodoro.overlayFocusMsg"),
      });
      setMode(next);
    } else {
      void notify(t("pomodoro.notifTitle"), t("pomodoro.notifBreakDone"));
      void emit("pomodoro://session-done", {
        title: t("pomodoro.overlayBreakTitle"),
        message: t("pomodoro.overlayBreakMsg"),
      });
      setMode("focus");
    }
  }

  return { mode, running, remainingMs, focusDone, display, minutes, start, pause, reset, setMode, setMinutes };
});
