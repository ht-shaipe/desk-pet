<script setup lang="ts">
import { onMounted, onUnmounted, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import PetSprite from "@/components/pet/PetSprite.vue";
import PanelContainer from "@/components/layout/PanelContainer.vue";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { useClipboardStore } from "@/stores/clipboard";
import { useLauncherStore } from "@/stores/launcher";
import { useNotesStore } from "@/stores/notes";
import { collapseWindow, expandWindow, rememberPanelSize } from "@/lib/windowCtl";
import { useUpdater } from "@/composables/useUpdater";

const ui = useUiStore();
const config = useConfigStore();
const win = getCurrentWindow();

let unlistenMoved: UnlistenFn | undefined;
let unlistenResized: UnlistenFn | undefined;

onMounted(() => {
  void useClipboardStore().init();
  void useLauncherStore().init();
  void useNotesStore().init();
  // 启动静默检查更新 + 周期性检查
  const updater = useUpdater();
  void updater.checkOnStartup();
  updater.startPeriodicCheck();
  void config
    .init()
    .then(() => win.onMoved((event) => config.onWindowMoved(event.payload)))
    .then((unlisten) => {
      unlistenMoved = unlisten;
    })
    .catch((err) => console.error("[desk-pet] 窗口位置初始化失败:", err));
  win
    .onResized((event) => {
      if (ui.expanded) {
        rememberPanelSize({ width: event.payload.width, height: event.payload.height });
      }
    })
    .then((unlisten) => {
      unlistenResized = unlisten;
    })
    .catch((err) => console.error("[desk-pet] resize 监听注册失败:", err));
});

onUnmounted(() => {
  unlistenMoved?.();
  unlistenResized?.();
});

watch(
  () => ui.expanded,
  async (expanded) => {
    if (expanded) {
      await expandWindow();
    } else {
      await collapseWindow();
    }
  }
);
</script>

<template>
  <div class="fixed inset-0 overflow-hidden">
    <PanelContainer v-show="ui.expanded" />
    <PetSprite />
  </div>
</template>
