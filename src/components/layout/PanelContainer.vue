<script setup lang="ts">
import type { Component } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChatDotRound, CopyDocument, Grid, MagicStick, Minus, Notebook, Setting, Suitcase, Timer, User } from "@element-plus/icons-vue";
import { useUiStore, type PanelView } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePomodoroStore } from "@/stores/pomodoro";
import Launcher from "@/views/Launcher.vue";
import Notes from "@/views/Notes.vue";
import Pomodoro from "@/views/Pomodoro.vue";
import Clipboard from "@/views/Clipboard.vue";
import Toolbox from "@/views/Toolbox.vue";
import Chat from "@/views/Chat.vue";
import QuickSettings from "@/views/QuickSettings.vue";
import Account from "@/views/Account.vue";
import Settings from "@/views/Settings.vue";

const ui = useUiStore();
const config = useConfigStore();
const pomodoro = usePomodoroStore();
const { t } = useI18n();

const tabs = computed<Array<{ view: PanelView; label: string; icon: Component }>>(() => [
  { view: "chat", label: t("tabs.chat"), icon: ChatDotRound },
  { view: "launcher", label: t("tabs.launcher"), icon: Grid },
  { view: "notes", label: t("tabs.notes"), icon: Notebook },
  { view: "pomodoro", label: t("tabs.pomodoro"), icon: Timer },
  { view: "clipboard", label: t("tabs.clipboard"), icon: CopyDocument },
  { view: "toolbox", label: t("tabs.toolbox"), icon: Suitcase },
  { view: "quick", label: t("tabs.quick"), icon: MagicStick },
  { view: "account", label: t("tabs.account"), icon: User },
  { view: "settings", label: t("tabs.settings"), icon: Setting },
]);

const activeIndex = computed(() => Math.max(0, tabs.value.findIndex((tab) => tab.view === ui.activeView)));

const win = getCurrentWindow();

/** 头部空白区按下即拖动窗口；折叠按钮及其内部元素不触发。 */
function onHeaderPointerdown(e: PointerEvent) {
  if (e.button !== 0) {
    return;
  }
  const target = e.target as HTMLElement;
  if (target.closest(".collapse-btn")) {
    return;
  }
  void win.startDragging();
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
  <div class="panel-card">
    <div class="panel-header" @pointerdown="onHeaderPointerdown">
      <div class="panel-brand">
        <span class="brand-dot" aria-hidden="true" />
        <span class="brand-name">{{ config.settings.app_name ?? "Desk Pet" }}</span>
      </div>
      <el-tooltip :content="t('panel.collapse')" placement="bottom" :show-after="300">
        <button class="collapse-btn" :aria-label="t('panel.collapse')" @click="ui.collapse()">
          <el-icon :size="13"><Minus /></el-icon>
        </button>
      </el-tooltip>
    </div>
    <nav class="panel-tabs" :style="{ gridTemplateColumns: `repeat(${tabs.length}, minmax(0, 1fr))` }">
      <span
        class="tab-glider"
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
      >
        <button
          class="tab-btn"
          :class="{ active: ui.activeView === tab.view }"
          :aria-label="tab.label"
          :aria-current="ui.activeView === tab.view ? 'page' : undefined"
          @click="ui.setActiveView(tab.view)"
        >
          <el-icon :size="16"><component :is="tab.icon" /></el-icon>
          <span v-if="tab.view === 'pomodoro' && pomodoro.running" class="tab-pip" />
        </button>
      </el-tooltip>
    </nav>
    <div class="panel-body" @pointerdown="onBodyPointerdown">
      <Transition name="view-fade" mode="out-in">
        <Launcher v-if="ui.activeView === 'launcher'" />
        <Notes v-else-if="ui.activeView === 'notes'" />
        <Pomodoro v-else-if="ui.activeView === 'pomodoro'" />
        <Clipboard v-else-if="ui.activeView === 'clipboard'" />
        <Toolbox v-else-if="ui.activeView === 'toolbox'" />
        <Chat v-else-if="ui.activeView === 'chat'" />
        <QuickSettings v-else-if="ui.activeView === 'quick'" />
        <Account v-else-if="ui.activeView === 'account'" />
        <Settings v-else />
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.panel-card {
  position: absolute;
  left: 8px;
  right: 8px;
  top: 42px;
  bottom: 8px;
  background: var(--dp-panel-bg);
  border: 1px solid var(--dp-panel-border);
  border-radius: 16px;
  box-shadow: var(--dp-panel-shadow);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: panel-in 0.18s ease-out;
}

.panel-header {
  display: flex;
  align-items: center;
  padding: 12px 14px 10px 64px;
  flex-shrink: 0;
}

.panel-brand {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-right: auto;
}

.brand-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--dp-accent), var(--dp-accent-deep));
  box-shadow: 0 0 0 3px var(--dp-tag-soft);
}

.brand-name {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 1.5px;
  text-transform: uppercase;
  color: var(--dp-text-muted);
}

.collapse-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  padding: 0;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--dp-text-muted);
  cursor: pointer;
  transition: background 0.18s ease, color 0.18s ease;
}

.collapse-btn:hover {
  background: var(--dp-btn-bg);
  color: var(--dp-text-primary);
}

.collapse-btn:focus-visible,
.tab-btn:focus-visible {
  outline: 2px solid var(--dp-accent);
  outline-offset: 1px;
}

/* ---------- 分段式导航条 ---------- */

.panel-tabs {
  position: relative;
  display: grid;
  margin: 0 8px 8px 8px;
  padding: 4px;
  border: 1px solid var(--dp-tabbar-border);
  border-radius: 13px;
  background: var(--dp-tabbar-bg);
  flex-shrink: 0;
}

.tab-glider {
  position: absolute;
  top: 4px;
  left: 4px;
  height: 32px;
  border-radius: 9px;
  background: var(--dp-accent);
  background-image: linear-gradient(180deg, rgba(255, 255, 255, 0.18), rgba(255, 255, 255, 0) 60%);
  box-shadow: 0 2px 6px -1px rgba(0, 0, 0, 0.25);
  box-shadow: 0 3px 9px -2px color-mix(in srgb, var(--dp-accent) 60%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.22);
  transition: transform 0.3s cubic-bezier(0.34, 1.15, 0.64, 1);
  pointer-events: none;
}

.tab-btn {
  position: relative;
  z-index: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 32px;
  padding: 0;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--dp-text-muted);
  cursor: pointer;
  transition: background 0.18s ease, color 0.18s ease;
}

.tab-btn :deep(.el-icon) {
  transition: transform 0.18s ease;
}

.tab-btn:active :deep(.el-icon) {
  transform: scale(0.86);
}

.tab-btn:not(.active):hover {
  color: var(--dp-text-primary);
  background: var(--dp-tab-hover-bg);
}

.tab-btn.active {
  color: #ffffff;
}

/* 番茄钟运行中的小圆点 */
.tab-pip {
  position: absolute;
  top: 6px;
  right: 7px;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--dp-accent);
  box-shadow: 0 0 0 2px var(--dp-tabbar-bg);
  animation: pip-pulse 1.8s ease-in-out infinite;
}

.tab-btn.active .tab-pip {
  background: #ffffff;
  box-shadow: none;
}

@keyframes pip-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.4;
  }
}

.panel-body {
  flex: 1;
  min-height: 0;
  padding: 0 6px 6px;
  display: flex;
  flex-direction: column;
}

/* 视图切换过渡 */
.view-fade-enter-active,
.view-fade-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.view-fade-enter-from {
  opacity: 0;
  transform: translateY(4px);
}

.view-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@keyframes panel-in {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
</style>
