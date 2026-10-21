<script setup lang="ts">
import type { Component } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { CopyDocument, Grid, MagicStick, Minus, Notebook, Setting, Suitcase, Timer, User } from "@element-plus/icons-vue";
import { useUiStore, type PanelView } from "@/stores/ui";
import LauncherGrid from "./LauncherGrid.vue";
import NotesView from "./NotesView.vue";
import PomodoroView from "./PomodoroView.vue";
import ClipboardView from "./ClipboardView.vue";
import ToolboxView from "./toolbox/ToolboxView.vue";
import QuickSettingsView from "./QuickSettingsView.vue";
import AccountView from "./AccountView.vue";
import SettingsView from "./SettingsView.vue";

const ui = useUiStore();
const { t } = useI18n();

const tabs = computed<Array<{ view: PanelView; label: string; icon: Component }>>(() => [
  { view: "launcher", label: t("tabs.launcher"), icon: Grid },
  { view: "notes", label: t("tabs.notes"), icon: Notebook },
  { view: "pomodoro", label: t("tabs.pomodoro"), icon: Timer },
  { view: "clipboard", label: t("tabs.clipboard"), icon: CopyDocument },
  { view: "toolbox", label: t("tabs.toolbox"), icon: Suitcase },
  { view: "quick", label: t("tabs.quick"), icon: MagicStick },
  { view: "account", label: t("tabs.account"), icon: User },
  { view: "settings", label: t("tabs.settings"), icon: Setting },
]);
</script>

<template>
  <div class="panel-card">
    <div class="panel-header">
      <span class="panel-title">Desk Pet</span>
      <el-tooltip :content="t('panel.collapse')" placement="bottom">
        <button class="icon-btn" @click="ui.collapse()">
          <el-icon :size="14"><Minus /></el-icon>
        </button>
      </el-tooltip>
    </div>
    <div class="panel-tabs">
      <el-tooltip v-for="tab in tabs" :key="tab.view" :content="tab.label" placement="bottom">
        <button
          class="icon-btn tab-btn"
          :class="{ active: ui.activeView === tab.view }"
          @click="ui.setActiveView(tab.view)"
        >
          <el-icon :size="16"><component :is="tab.icon" /></el-icon>
        </button>
      </el-tooltip>
    </div>
    <div class="panel-body">
      <LauncherGrid v-if="ui.activeView === 'launcher'" />
      <NotesView v-else-if="ui.activeView === 'notes'" />
      <PomodoroView v-else-if="ui.activeView === 'pomodoro'" />
      <ClipboardView v-else-if="ui.activeView === 'clipboard'" />
      <ToolboxView v-else-if="ui.activeView === 'toolbox'" />
      <QuickSettingsView v-else-if="ui.activeView === 'quick'" />
      <AccountView v-else-if="ui.activeView === 'account'" />
      <SettingsView v-else />
    </div>
  </div>
</template>

<style scoped>
.panel-card {
  position: absolute;
  left: 18px;
  right: 18px;
  top: 52px;
  bottom: 18px;
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
  gap: 8px;
  padding: 10px 12px 4px 64px;
  min-height: 32px;
}

.panel-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--dp-text-muted);
  margin-right: auto;
}

.panel-tabs {
  display: flex;
  gap: 4px;
  padding: 0 12px 8px 64px;
  border-bottom: 1px solid var(--dp-divider);
}

.icon-btn {
  border: none;
  background: var(--dp-btn-bg);
  color: var(--dp-text-muted);
  width: 26px;
  height: 26px;
  border-radius: 8px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  flex-shrink: 0;
}

.icon-btn:hover {
  background: var(--dp-btn-hover-bg);
}

.tab-btn {
  width: 28px;
  height: 28px;
}

.tab-btn.active {
  background: var(--dp-accent);
  color: #ffffff;
}

.panel-body {
  flex: 1;
  min-height: 0;
  padding: 8px 12px 12px;
  display: flex;
  flex-direction: column;
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
