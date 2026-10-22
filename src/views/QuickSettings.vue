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
import { quickAction } from "@/api/system";
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
  <div class="view-root">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("quick.title") }}</span>
    </div>
    <div class="flex-1 grid grid-cols-2 gap-2.5 pt-2 px-0.5 pb-0.5 content-start">
      <button
        v-for="item in visible"
        :key="item.action"
        class="flex flex-col items-center gap-2 py-4 px-1.5 bg-dp-card-bg border border-dp-card-border rounded-xl cursor-pointer transition-all duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover active:scale-[0.97]"
        @click="onAction(item)"
      >
        <span class="text-dp-accent">
          <el-icon :size="20"><component :is="item.icon" /></el-icon>
        </span>
        <span class="text-xs text-dp-secondary">{{ item.label }}</span>
      </button>
    </div>
  </div>
</template>
