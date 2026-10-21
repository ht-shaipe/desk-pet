<script setup lang="ts">
import { onMounted, onUnmounted, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import PetSprite from "@/components/PetSprite.vue";
import PanelContainer from "@/components/PanelContainer.vue";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { useClipboardStore } from "@/stores/clipboard";
import { useLauncherStore } from "@/stores/launcher";
import { useNotesStore } from "@/stores/notes";
import { collapseWindow, expandWindow } from "@/utils/windowCtl";

const ui = useUiStore();
const config = useConfigStore();
const win = getCurrentWindow();

let unlistenMoved: UnlistenFn | undefined;

onMounted(async () => {
  await config.init();
  unlistenMoved = await win.onMoved((event) => config.onWindowMoved(event.payload));
  void useClipboardStore().init();
  void useLauncherStore().init();
  void useNotesStore().init();
});

onUnmounted(() => {
  unlistenMoved?.();
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
  <div class="app-root" :class="{ expanded: ui.expanded }">
    <PanelContainer v-show="ui.expanded" />
    <PetSprite />
  </div>
</template>
