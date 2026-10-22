<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfigStore } from "@/stores/config";

const { t } = useI18n();
const config = useConfigStore();
const autostart = ref(false);
const presetColors = ["#409eff", "#7265e6", "#67c23a", "#e6a23c", "#f56c6c", "#00a2ae", "#eb2f96"];

async function onChangeAvatar() {
  const picked = await open({
    multiple: false,
    filters: [{ name: t("settings.avatarFilter"), extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
  });
  if (typeof picked !== "string" || !picked) {
    return;
  }
  try {
    await config.changePetAvatar(picked);
    ElMessage.success(t("settings.avatarChanged"));
  } catch (e) {
    ElMessage.error(t("settings.avatarFail", { err: String(e) }));
  }
}

async function onResetAvatar() {
  await config.resetPetAvatarConfig();
  ElMessage.success(t("settings.avatarResetOk"));
}

const themeModel = computed({
  get: () => config.settings.theme ?? "auto",
  set: (val: string) => void config.setTheme(val),
});

const spriteSizeModel = computed({
  get: () => config.settings.sprite_size ?? 48,
  set: (val: number | number[]) => void config.setSpriteSize(Array.isArray(val) ? val[0] : val),
});

const accentModel = computed({
  get: () => config.settings.accent_color ?? "#409eff",
  set: (val: string | null) => void config.setAccentColor(val),
});

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
            <div class="row-title">{{ t("settings.theme") }}</div>
            <div class="row-desc">{{ t("settings.themeDesc") }}</div>
          </div>
          <el-select v-model="themeModel" size="small" style="width: 120px">
            <el-option :label="t('settings.themeAuto')" value="auto" />
            <el-option :label="t('settings.themeLight')" value="light" />
            <el-option :label="t('settings.themeDark')" value="dark" />
          </el-select>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.avatar") }}</div>
            <div class="row-desc">{{ t("settings.avatarDesc") }}</div>
          </div>
          <div class="accent-picker">
            <el-button size="small" @click="onChangeAvatar">{{ t("settings.avatarChange") }}</el-button>
            <el-button
              v-if="config.settings.pet_avatar"
              size="small"
              text
              @click="onResetAvatar"
            >
              {{ t("settings.avatarReset") }}
            </el-button>
          </div>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.accent") }}</div>
            <div class="row-desc">{{ t("settings.accentDesc") }}</div>
          </div>
          <div class="accent-picker">
            <el-color-picker v-model="accentModel" size="small" :predefine="presetColors" />
            <el-button
              v-if="config.settings.accent_color"
              size="small"
              text
              @click="accentModel = null"
            >
              {{ t("settings.accentReset") }}
            </el-button>
          </div>
        </div>
        <div class="settings-row">
          <div class="row-info">
            <div class="row-title">{{ t("settings.spriteSize") }}</div>
            <div class="row-desc">{{ t("settings.spriteSizeDesc") }}</div>
          </div>
          <el-slider
            v-model="spriteSizeModel"
            :min="32"
            :max="64"
            :step="2"
            size="small"
            style="width: 130px; --el-slider-height: 16px"
          />
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
  background: var(--dp-hover-bg);
}

.row-info {
  margin-right: auto;
}

.accent-picker {
  display: flex;
  align-items: center;
  gap: 8px;
}

.row-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--dp-text-primary);
}

.row-desc {
  font-size: 12px;
  color: var(--dp-text-tertiary);
  margin-top: 2px;
}
</style>
