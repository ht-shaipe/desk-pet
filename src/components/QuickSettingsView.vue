<script setup lang="ts">
import type { Component } from "vue";
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import {
  Bell,
  Connection,
  Lock,
  Monitor,
  Moon,
  MoonNight,
  Mute,
  Setting,
} from "@element-plus/icons-vue";
import { quickAction } from "@/api";
import { useConfigStore } from "@/stores/config";

const { t } = useI18n();
const config = useConfigStore();

interface QuickItem {
  action: string;
  label: string;
  icon: Component;
  macOnly?: boolean;
}

const actions = computed<QuickItem[]>(() => [
  { action: "dark-mode", label: t("quick.darkMode"), icon: Moon, macOnly: true },
  { action: "mute", label: t("quick.mute"), icon: Mute, macOnly: true },
  { action: "lock", label: t("quick.lock"), icon: Lock },
  { action: "sleep", label: t("quick.sleep"), icon: MoonNight },
  { action: "open-settings:main", label: t("quick.sysSettings"), icon: Setting },
  { action: "open-settings:sound", label: t("quick.sound"), icon: Bell },
  { action: "open-settings:display", label: t("quick.display"), icon: Monitor },
  { action: "open-settings:network", label: t("quick.network"), icon: Connection },
]);

const visible = computed(() =>
  actions.value.filter((a) => !a.macOnly || config.platform === "macos")
);

async function onAction(item: QuickItem) {
  try {
    await quickAction(item.action);
  } catch (e) {
    ElMessage.error(String(e));
  }
}
</script>

<template>
  <div class="quick-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("quick.title") }}</span>
    </div>
    <div class="quick-grid">
      <button v-for="item in visible" :key="item.action" class="quick-item" @click="onAction(item)">
        <span class="quick-icon">
          <el-icon :size="20"><component :is="item.icon" /></el-icon>
        </span>
        <span class="quick-label">{{ item.label }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.quick-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.quick-grid {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 10px;
  padding: 8px 2px 2px;
  align-content: start;
}

.quick-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 16px 6px;
  background: var(--dp-card-bg);
  border: 1px solid var(--dp-card-border);
  border-radius: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.quick-item:hover {
  border-color: var(--dp-card-hover-border);
  box-shadow: var(--dp-card-hover-shadow);
}

.quick-item:active {
  transform: scale(0.97);
}

.quick-icon {
  color: var(--dp-accent);
}

.quick-label {
  font-size: 12px;
  color: var(--dp-text-secondary);
}
</style>
