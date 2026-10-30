<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

import PetSprite from "@/components/pet/PetSprite.vue";
import PanelContainer from "@/components/layout/PanelContainer.vue";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { useClipboardStore } from "@/stores/clipboard";
import { useLauncherStore } from "@/stores/launcher";
import { useFavoritesStore } from "@/stores/favorites";
import { collapseWindow, expandWindow, rememberPanelSize, startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import { useUpdater } from "@/composables/useUpdater";
import { autoPullFavorites } from "@/composables/useStashCloud";

const ui = useUiStore();
const config = useConfigStore();
const win = getCurrentWindow();

let unlistenMoved: UnlistenFn | undefined;
let unlistenResized: UnlistenFn | undefined;

onMounted(() => {
  // 主窗口配置为 visible:false 启动：等首帧绘制完成再 show，避免透明窗首帧前的原生灰底闪现。
  // 兜底：macOS WKWebView 在窗口未显示时可能暂停 rAF（回调永不触发），超时后仍必须 show，
  // 否则主面板永远打不开。
  nextTick()
    .then(() => {
      const painted = new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      });
      const timeout = new Promise<void>((resolve) => setTimeout(resolve, 400));
      return Promise.race([painted, timeout]);
    })
    .then(() => win.show())
    .catch(() => {
      win.show();
    });
  void useClipboardStore().init();
  void useLauncherStore().init();
  void useFavoritesStore().init().catch((err) => console.error("[favorites] init failed:", err));
  // 云同步开启时启动静默自动拉取：必须等 config.init() 载入开关/token 后再触发
  // （与收藏页开页共用 5 分钟节流，不会重复拉）
  // 启动静默检查更新 + 周期性检查；发现新版本时自动展开面板，直接打开设置-关于页处理下载
  const updater = useUpdater();
  updater.startPeriodicCheck();
  void updater
    .checkOnStartup(false)
    .then((hasUpdate) => {
      if (hasUpdate) {
        ui.settingsTab = "about";
        ui.setActiveView("settings");
        ui.expand();
      }
    })
    .catch(() => undefined);
  const configInited = config.init();
  void configInited
    .then(() => win.onMoved((event) => config.onWindowMoved(event.payload)))
    .then((unlisten) => {
      unlistenMoved = unlisten;
    })
    .catch((err) => console.error("[desk-pet] 窗口位置初始化失败:", err));
  // 启动自动同步：设置载入完成后静默拉取云端收藏（云同步开启且已登录时才生效）
  void configInited.then(() => autoPullFavorites()).catch(() => undefined);
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
      // 就地收起可能改变了精灵位置：程序化 setPosition 不保证触发 onMoved，这里显式同步持久化
      const pos = await win.outerPosition();
      config.onWindowMoved({ x: pos.x, y: pos.y });
    }
  }
);

/** 边缘/四角手柄按下时交给窗口按对应方向进入 resize 拖拽。
 *  macOS 上原生 startResizeDragging 未实现（tao 返回 NotSupported 且错误被运行时吞掉），
 *  由 windowCtl 内部按平台分流：macOS 手动驱动，其余平台走原生。 */
function startResize(direction: ResizeDirection, event: PointerEvent) {
  startWindowResize(direction, event);
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
        class="rs-hint left-0 right-0 top-0 h-[6px] cursor-ns-resize"
        @pointerdown.prevent="startResize('North', $event)"
      />
      <div
        class="rs-hint left-0 right-0 bottom-0 h-[6px] cursor-ns-resize"
        @pointerdown.prevent="startResize('South', $event)"
      />
      <div
        class="rs-hint left-0 top-0 bottom-0 w-[6px] cursor-ew-resize"
        @pointerdown.prevent="startResize('West', $event)"
      />
      <div
        class="rs-hint right-0 top-0 bottom-0 w-[6px] cursor-ew-resize"
        @pointerdown.prevent="startResize('East', $event)"
      />
      <!-- 四角 -->
      <div
        class="rs-hint left-0 top-0 w-[20px] h-[20px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('NorthWest', $event)"
      />
      <div
        class="rs-hint right-0 top-0 w-[20px] h-[20px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('NorthEast', $event)"
      />
      <div
        class="rs-hint left-0 bottom-0 w-[20px] h-[20px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('SouthWest', $event)"
      />
      <div
        class="rs-hint right-0 bottom-0 w-[20px] h-[20px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('SouthEast', $event)"
      />
    </div>
  </div>
</template>
