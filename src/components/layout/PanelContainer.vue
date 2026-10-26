<script setup lang="ts">
import type { Component } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChatDotRound, CopyDocument, Grid, Minus, Setting, Star, Suitcase, Timer, User } from "@element-plus/icons-vue";
import { useUiStore, type PanelView } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePomodoroStore } from "@/stores/pomodoro";
import Launcher from "@/views/Launcher.vue";
import Pomodoro from "@/views/Pomodoro.vue";
import Clipboard from "@/views/Clipboard.vue";
import Favorites from "@/views/Favorites.vue";
import Toolbox from "@/views/Toolbox.vue";
import Chat from "@/views/Chat.vue";
import Account from "@/views/Account.vue";
import Settings from "@/views/Settings.vue";

const ui = useUiStore();
const config = useConfigStore();
const pomodoro = usePomodoroStore();
const { t } = useI18n();

const tabs = computed<Array<{ view: PanelView; label: string; icon: Component }>>(() => [
  { view: "chat", label: t("tabs.chat"), icon: ChatDotRound },
  { view: "clipboard", label: t("tabs.clipboard"), icon: CopyDocument },
  { view: "stash", label: t("tabs.stash"), icon: Star },
  { view: "toolbox", label: t("tabs.toolbox"), icon: Suitcase },
  { view: "launcher", label: t("tabs.launcher"), icon: Grid },
  { view: "pomodoro", label: t("tabs.pomodoro"), icon: Timer },
  { view: "account", label: t("tabs.account"), icon: User },
  { view: "settings", label: t("tabs.settings"), icon: Setting },
]);

const activeIndex = computed(() => Math.max(0, tabs.value.findIndex((tab) => tab.view === ui.activeView)));

const win = getCurrentWindow();

/** 头部空白区拖动窗口：位移超过阈值才交给原生拖拽；折叠按钮及其内部元素不触发。
 *  macOS 陷阱（勿回退成 pointerdown 立即拖拽）：accessory 应用 + acceptFirstMouse 下，
 *  点击激活窗口与 startDragging 同时触发，原生拖拽事件会携带错误坐标，
 *  面板瞬间飞到光标处（点击位置变成窗口左上角）。改为 4px 位移阈值后，
 *  普通点击不再触发拖拽，真实拖动才走原生逻辑。 */
const HEADER_DRAG_THRESHOLD = 4;
let headerDragOrigin: { x: number; y: number } | null = null;

function onHeaderPointerdown(e: PointerEvent) {
  if (e.button !== 0) {
    headerDragOrigin = null;
    return;
  }
  const target = e.target as HTMLElement;
  if (target.closest(".collapse-btn")) {
    headerDragOrigin = null;
    return;
  }
  headerDragOrigin = { x: e.clientX, y: e.clientY };
}

function onHeaderPointermove(e: PointerEvent) {
  if (!headerDragOrigin) {
    return;
  }
  const dx = e.clientX - headerDragOrigin.x;
  const dy = e.clientY - headerDragOrigin.y;
  if (Math.sqrt(dx * dx + dy * dy) > HEADER_DRAG_THRESHOLD) {
    headerDragOrigin = null;
    void win.startDragging();
  }
}

function onHeaderPointerend() {
  headerDragOrigin = null;
}

/** 番茄钟计时中时，在标签提示里附带剩余时间。 */
function tabTip(label: string, view: PanelView): string {
  return view === "pomodoro" && pomodoro.running ? `${label} · ${pomodoro.display}` : label;
}

/** 隐藏 Dock（accessory）模式下应用点击不会自动激活，键盘输入失效；点击面板内容时主动取焦点。 */
function onBodyPointerdown() {
  void win.setFocus();
}
</script>

<template>
  <div
    class="absolute inset-x-2 top-[42px] bottom-2 bg-dp-panel-bg border border-dp-panel-border rounded-2xl shadow-dp-panel flex flex-col overflow-hidden animate-panel-in"
  >
    <div
      class="flex items-center pt-3 pr-3.5 pb-2.5 pl-16 shrink-0"
      @pointerdown="onHeaderPointerdown"
      @pointermove="onHeaderPointermove"
      @pointerup="onHeaderPointerend"
      @pointercancel="onHeaderPointerend"
      @pointerleave="onHeaderPointerend"
    >
      <div class="inline-flex items-center gap-2 mr-auto">
        <span
          class="w-[7px] h-[7px] rounded-full bg-[linear-gradient(135deg,var(--dp-accent),var(--dp-accent-deep))] shadow-[0_0_0_3px_var(--dp-tag-soft)]"
          aria-hidden="true"
        />
        <span class="text-[11px] font-bold tracking-[1.5px] uppercase text-dp-muted">{{ config.settings.app_name ?? "Desk Pet" }}</span>
      </div>
      <!-- teleported=false：跟随面板隐藏，避免收起时 popper 锚点归零、闪现在宠物小窗位置 -->
      <el-tooltip
        :content="t('panel.collapse')"
        placement="bottom"
        :show-after="300"
        :teleported="false"
        effect="light"
        popper-class="dp-tip"
      >
        <button
          class="collapse-btn inline-flex items-center justify-center w-[26px] h-[26px] p-0 rounded-[9px] bg-transparent text-dp-muted cursor-pointer transition-[background,color] duration-180 ease-[ease] hover:bg-dp-btn-bg hover:text-dp-primary focus-visible:[outline:2px_solid_var(--dp-accent)] focus-visible:outline-offset-1"
          :aria-label="t('panel.collapse')"
          @click="ui.collapse()"
        >
          <el-icon :size="13"><Minus /></el-icon>
        </button>
      </el-tooltip>
    </div>
    <nav
      class="relative grid mx-2 mb-2 p-1 border border-dp-tabbar-border rounded-[13px] bg-dp-tabbar-bg shrink-0"
      :style="{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }"
    >
      <span
        class="absolute top-1 left-1 h-8 rounded-[9px] bg-dp-accent bg-[linear-gradient(180deg,rgba(255,255,255,0.18),rgba(255,255,255,0)_60%)] shadow-[0_3px_9px_-2px_color-mix(in_srgb,var(--dp-accent)_60%,transparent),inset_0_1px_0_rgba(255,255,255,0.22)] transition-transform duration-300 ease-[cubic-bezier(0.34,1.15,0.64,1)] pointer-events-none"
        aria-hidden="true"
        :style="{
          width: `calc((100% - 8px) / ${tabs.length})`,
          transform: `translateX(${activeIndex * 100}%)`,
        }"
      />
      <el-tooltip
        v-for="tab in tabs"
        :key="tab.view"
        :content="tabTip(tab.label, tab.view)"
        placement="bottom"
        :show-after="250"
        :teleported="false"
        effect="light"
        popper-class="dp-tip"
      >
        <button
          class="relative z-1 inline-flex items-center justify-center w-full h-8 p-0 rounded-[9px] bg-transparent text-dp-muted cursor-pointer transition-[background,color] duration-180 ease-[ease] not-[.active]:hover:text-dp-primary not-[.active]:hover:bg-dp-tab-hover-bg focus-visible:[outline:2px_solid_var(--dp-accent)] focus-visible:outline-offset-1 [&_.el-icon]:transition-transform [&_.el-icon]:duration-180 [&_.el-icon]:ease-[ease] [&:active_.el-icon]:scale-[0.86]"
          :class="ui.activeView === tab.view ? 'active text-white' : ''"
          :aria-label="tab.label"
          :aria-current="ui.activeView === tab.view ? 'page' : undefined"
          @click="ui.setActiveView(tab.view)"
        >
          <el-icon :size="16"><component :is="tab.icon" /></el-icon>
          <span
            v-if="tab.view === 'pomodoro' && pomodoro.running"
            class="absolute top-1.5 right-[7px] w-[5px] h-[5px] rounded-full animate-pip-pulse pointer-events-none"
            :class="ui.activeView === tab.view ? 'bg-white shadow-none' : 'bg-dp-accent shadow-[0_0_0_2px_var(--dp-tabbar-bg)]'"
          />
        </button>
      </el-tooltip>
    </nav>
    <div class="flex-1 min-h-0 px-1.5 pb-1.5 flex flex-col" @pointerdown="onBodyPointerdown">
      <Transition
        mode="out-in"
        enter-active-class="transition-[opacity,transform] duration-160 ease-[ease]"
        leave-active-class="transition-[opacity,transform] duration-160 ease-[ease]"
        enter-from-class="opacity-0 translate-y-1"
        leave-to-class="opacity-0 -translate-y-1"
      >
        <Launcher v-if="ui.activeView === 'launcher'" />
        <Pomodoro v-else-if="ui.activeView === 'pomodoro'" />
        <Clipboard v-else-if="ui.activeView === 'clipboard'" />
        <Favorites v-else-if="ui.activeView === 'stash'" />
        <Toolbox v-else-if="ui.activeView === 'toolbox'" />
        <Chat v-else-if="ui.activeView === 'chat'" />
        <Account v-else-if="ui.activeView === 'account'" />
        <Settings v-else />
      </Transition>
    </div>
  </div>
</template>
