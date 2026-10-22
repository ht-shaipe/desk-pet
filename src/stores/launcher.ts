import { defineStore } from "pinia";
import { ref } from "vue";
import { getLauncher, saveLauncher } from "@/api/launcher";
import type { LauncherItem } from "@/types";
import { i18n } from "@/i18n";
import { useConfigStore } from "./config";

const PALETTE = ["#409eff", "#67c23a", "#e6a23c", "#f56c6c", "#7265e6", "#00a2ae", "#eb2f96", "#909399"];

/** 首次运行时按平台预置几个常用启动项（名称跟随界面语言）。 */
function seedItems(platform: string): LauncherItem[] {
  const t = i18n.global.t;
  if (platform === "windows") {
    return [
      { id: "seed-cmd", name: t("launcher.seeds.cmd"), icon: "C", item_type: "command", value: "cmd", color: "#409eff" },
      { id: "seed-notepad", name: t("launcher.seeds.notepad"), icon: "N", item_type: "command", value: "notepad", color: "#67c23a" },
      { id: "seed-calc", name: t("launcher.seeds.calc"), icon: "Cal", item_type: "command", value: "calc", color: "#e6a23c" },
      { id: "seed-github", name: "GitHub", icon: "G", item_type: "url", value: "https://github.com", color: "#909399" },
    ];
  }
  return [
    { id: "seed-safari", name: "Safari", icon: "S", item_type: "app", value: "Safari", color: "#409eff" },
    { id: "seed-terminal", name: t("launcher.seeds.terminal"), icon: "T", item_type: "app", value: "Terminal", color: "#374151" },
    { id: "seed-notes", name: t("launcher.seeds.notes"), icon: "N", item_type: "app", value: "Notes", color: "#e6a23c" },
    { id: "seed-github", name: "GitHub", icon: "G", item_type: "url", value: "https://github.com", color: "#909399" },
  ];
}

export const useLauncherStore = defineStore("launcher", () => {
  const items = ref<LauncherItem[]>([]);
  const loaded = ref(false);

  async function persist() {
    await saveLauncher({ items: items.value });
  }

  async function init() {
    if (loaded.value) {
      return;
    }
    const data = await getLauncher();
    if (data.items.length > 0) {
      items.value = data.items;
    } else {
      items.value = seedItems(useConfigStore().platform);
      void persist();
    }
    loaded.value = true;
  }

  function addItem(item: LauncherItem) {
    items.value.unshift(item);
    void persist();
  }

  function updateItem(item: LauncherItem) {
    const idx = items.value.findIndex((i) => i.id === item.id);
    if (idx >= 0) {
      items.value[idx] = { ...item };
      void persist();
    }
  }

  function removeItem(id: string) {
    items.value = items.value.filter((i) => i.id !== id);
    void persist();
  }

  function nextColor(): string {
    return PALETTE[items.value.length % PALETTE.length];
  }

  return { items, init, addItem, updateItem, removeItem, nextColor };
});
