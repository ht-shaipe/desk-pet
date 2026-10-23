<script setup lang="ts">
import { onMounted, onUnmounted, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

/** 与 @tauri-apps/api/window.js 内部定义一致（模块未导出） */
type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";
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

/** 边缘/四角手柄按下时交给原生窗口按对应方向进入 resize 拖拽。 */
function startResize(direction: ResizeDirection) {
  void win.startResizeDragging(direction).catch(() => undefined);
}
</script>

<template>
  <div class="fixed inset-0 overflow-hidden">
    <PanelContainer v-show="ui.expanded" />
    <PetSprite />

    <!-- 展开态窗口边缘缩放手柄：悬停时显示对应方向光标，按下拖动改变窗体大小。
         四周与四角均不做颜色高亮，悬停只切换对应方向光标。
         z-10 需低于 PetSprite（z-20）：宠物图标位于窗口左上角，必须保持可点击/可拖拽，
         否则左上角手柄会截胡点击宠物（并因 1px 位移误触发 NW resize）。 -->
    <div v-show="ui.expanded" class="fixed inset-0 z-10 pointer-events-none" aria-hidden="true">
      <!-- 四边 -->
      <div
        class="rs-hint left-0 right-0 top-1 h-[6px] cursor-ns-resize"
        @pointerdown.prevent="startResize('North')"
      />
      <div
        class="rs-hint left-0 right-0 bottom-0 h-[6px] cursor-ns-resize"
        @pointerdown.prevent="startResize('South')"
      />
      <div
        class="rs-hint left-0 top-0 bottom-0 w-[6px] cursor-ew-resize"
        @pointerdown.prevent="startResize('West')"
      />
      <div
        class="rs-hint right-0 top-0 bottom-0 w-[6px] cursor-ew-resize"
        @pointerdown.prevent="startResize('East')"
      />
      <!-- 四角 -->
      <div
        class="rs-hint left-0 top-0 w-[20px] h-[20px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('NorthWest')"
      />
      <div
        class="rs-hint right-0 top-0 w-[20px] h-[20px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('NorthEast')"
      />
      <div
        class="rs-hint left-0 bottom-0 w-[20px] h-[20px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('SouthWest')"
      />
      <div
        class="rs-hint right-0 bottom-0 w-[20px] h-[20px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('SouthEast')"
      />
    </div>
  </div>
</template>
