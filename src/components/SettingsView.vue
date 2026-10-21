<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useConfigStore } from "@/stores/config";

const { t } = useI18n();
const config = useConfigStore();
const autostart = ref(false);

onMounted(async () => {
  try {
    autostart.value = await isEnabled();
  } catch {
    autostart.value = false;
  }
});

async function onAlwaysOnTop(val: string | number | boolean) {
  await config.setAlwaysOnTop(Boolean(val));
}

async function onLocaleChange(val: string | number | boolean) {
  await config.setLocale(String(val));
}

async function onAutostart(val: string | number | boolean) {
  try {
    if (Boolean(val)) {
      await enable();
    } else {
      await disable();
    }
    autostart.value = await isEnabled();
  } catch (e) {
    ElMessage.error(t("settings.autostartFail", { err: String(e) }));
  }
}
</script>

<template>
  <div class="settings-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("settings.title") }}</span>
    </div>
    <el-scrollbar class="view-scroll">
      <div class="settings-list">
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.lang") }}</div>
            <div class="row-desc">{{ t("settings.langDesc") }}</div>
          </div>
          <el-select
            :model-value="config.settings.locale"
            size="small"
            style="width: 120px"
            @change="onLocaleChange"
          >
            <el-option label="中文" value="zh-CN" />
            <el-option label="English" value="en" />
          </el-select>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.alwaysOnTop") }}</div>
            <div class="row-desc">{{ t("settings.alwaysOnTopDesc") }}</div>
          </div>
          <el-switch
            :model-value="config.settings.always_on_top"
            @change="onAlwaysOnTop"
          />
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.autostart") }}</div>
            <div class="row-desc">{{ t("settings.autostartDesc") }}</div>
          </div>
          <el-switch v-model="autostart" @change="onAutostart" />
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.shortcut") }}</div>
            <div class="row-desc">
              {{ config.platform === "macos" ? "Command + Shift + P" : "Ctrl + Shift + P" }}
            </div>
          </div>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.tray") }}</div>
            <div class="row-desc">{{ t("settings.trayDesc") }}</div>
          </div>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.about") }}</div>
            <div class="row-desc">Desk Pet v0.1.0 · {{ config.platform || "..." }}</div>
          </div>
        </div>
      </div>
    </el-scrollbar>
  </div>
</template>

<style scoped>
.settings-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.settings-list {
  padding: 6px 2px;
}

.settings-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 10px;
  border-radius: 10px;
  transition: background 0.15s ease;
}

.settings-row:hover {
  background: #eef1f5;
}

.row-info {
  margin-right: auto;
}

.row-title {
  font-size: 13px;
  font-weight: 500;
  color: #1f2937;
}

.row-desc {
  font-size: 12px;
  color: #9ca3af;
  margin-top: 2px;
}
</style>
