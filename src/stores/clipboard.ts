import { defineStore } from "pinia";
import { ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listen } from "@tauri-apps/api/event";
import {
  clearClipboardHistory,
  copyToClipboard,
  deleteClipboardEntries,
  deleteClipboardEntry,
  getClipboardHistory,
} from "@/api/clipboard";
import type { ClipEntry } from "@/types";

export const useClipboardStore = defineStore("clipboard", () => {
  const entries = ref<ClipEntry[]>([]);
  const loaded = ref(false);
  let unlisten: UnlistenFn | undefined;

  async function refresh() {
    entries.value = await getClipboardHistory();
  }

  async function init() {
    if (!loaded.value) {
      await refresh();
      loaded.value = true;
    }
    if (!unlisten) {
      unlisten = await listen("clipboard://changed", () => {
        void refresh();
      });
    }
  }

  async function copy(text: string) {
    await copyToClipboard(text);
  }

  async function remove(id: string) {
    await deleteClipboardEntry(id);
    await refresh();
  }

  /** 批量删除（按类型/日期条件清空用），一次落库。 */
  async function removeMany(ids: string[]) {
    await deleteClipboardEntries(ids);
    await refresh();
  }

  async function clearAll() {
    await clearClipboardHistory();
    await refresh();
  }

  function dispose() {
    unlisten?.();
  }

  return { entries, init, refresh, copy, remove, removeMany, clearAll, dispose };
});
