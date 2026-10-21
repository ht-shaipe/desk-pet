<script setup lang="ts">
import type { Component } from "vue";
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

const config = useConfigStore();

interface QuickItem {
  action: string;
  label: string;
  icon: Component;
  macOnly?: boolean;
}

const actions: QuickItem[] = [
  { action: "dark-mode", label: "深浅模式", icon: Moon, macOnly: true },
  { action: "mute", label: "静音切换", icon: Mute, macOnly: true },
  { action: "lock", label: "锁屏", icon: Lock },
  { action: "sleep", label: "睡眠", icon: MoonNight },
  { action: "open-settings:main", label: "系统设置", icon: Setting },
  { action: "open-settings:sound", label: "声音设置", icon: Bell },
  { action: "open-settings:display", label: "显示设置", icon: Monitor },
  { action: "open-settings:network", label: "网络设置", icon: Connection },
];

const visible = actions.filter((a) => !a.macOnly || config.platform === "macos");

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
      <span class="toolbar-title">快捷操作</span>
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
  background: #ffffff;
  border: 1px solid #eef0f3;
  border-radius: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.quick-item:hover {
  border-color: #c6e2ff;
  box-shadow: 0 4px 12px rgba(64, 158, 255, 0.12);
}

.quick-item:active {
  transform: scale(0.97);
}

.quick-icon {
  color: #409eff;
}

.quick-label {
  font-size: 12px;
  color: #374151;
}
</style>
