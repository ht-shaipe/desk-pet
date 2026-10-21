import { defineStore } from "pinia";
import { ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getPlatform, getSettings, saveSettings } from "@/api";
import type { AppSettings } from "@/types";
import { initWindowPosition } from "@/utils/windowCtl";
import { useUiStore } from "./ui";

const win = getCurrentWindow();

export const useConfigStore = defineStore("config", () => {
  const settings = ref<AppSettings>({ pet_x: null, pet_y: null, always_on_top: true });
  const platform = ref("");
  const windowMoving = ref(false);

  let persistTimer: ReturnType<typeof setTimeout> | undefined;
  let movingTimer: ReturnType<typeof setTimeout> | undefined;

  function schedulePersist() {
    if (persistTimer !== undefined) {
      clearTimeout(persistTimer);
    }
    persistTimer = setTimeout(() => {
      void saveSettings(settings.value);
    }, 500);
  }

  /** 窗口移动回调：防抖持久化位置（展开态的位移来自展开动画，不记录）。 */
  function onWindowMoved(payload: { x: number; y: number }) {
    windowMoving.value = true;
    if (movingTimer !== undefined) {
      clearTimeout(movingTimer);
    }
    movingTimer = setTimeout(() => {
      windowMoving.value = false;
    }, 200);

    if (useUiStore().expanded) {
      return;
    }
    settings.value.pet_x = payload.x;
    settings.value.pet_y = payload.y;
    schedulePersist();
  }

  async function setAlwaysOnTop(value: boolean) {
    settings.value.always_on_top = value;
    await win.setAlwaysOnTop(value);
    schedulePersist();
  }

  async function init() {
    platform.value = await getPlatform();
    const s = await getSettings();
    settings.value = {
      pet_x: s.pet_x,
      pet_y: s.pet_y,
      always_on_top: s.always_on_top,
    };
    await initWindowPosition(s.pet_x !== null && s.pet_y !== null, s.pet_x, s.pet_y);
    await win.setAlwaysOnTop(settings.value.always_on_top);
  }

  return { settings, platform, windowMoving, init, onWindowMoved, setAlwaysOnTop };
});
