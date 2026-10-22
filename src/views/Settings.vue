<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfigStore } from "@/stores/config";
import { useUpdater } from "@/composables/useUpdater";

const { t } = useI18n();
const config = useConfigStore();
const activeTab = ref("general");
const autostart = ref(false);
const presetColors = ["#3d8bfd", "#7265e6", "#67c23a", "#e6a23c", "#f56c6c", "#00a2ae", "#eb2f96"];

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
  get: () => config.settings.accent_color ?? "#3d8bfd",
  set: (val: string | null) => void config.setAccentColor(val),
});

onMounted(async () => {
  try {
    autostart.value = await isEnabled();
  } catch {
    autostart.value = false;
  }
  appNameModel.value = config.settings.app_name ?? "";
  try {
    const { getVersion } = await import("@tauri-apps/api/app");
    version.value = await getVersion();
  } catch {
    version.value = "?";
  }
});

async function onAlwaysOnTop(val: string | number | boolean) {
  await config.setAlwaysOnTop(Boolean(val));
}

async function onLocaleChange(val: string | number | boolean) {
  await config.setLocale(String(val));
}

async function onAppNameChange() {
  const name = (appNameModel.value ?? "").trim();
  if (!name || name === config.settings.app_name) {
    appNameModel.value = config.settings.app_name ?? "";
    return;
  }
  await config.setAppName(name);
  ElMessage.success(t("settings.appNameOk"));
}

const appNameModel = ref("");

const updater = useUpdater();
const version = ref("...");
const updateBtnDisabled = computed(
  () => updater.status.value === "checking" || updater.status.value === "downloading"
);

/** 检查更新按钮文案状态 */
const updateBtnText = computed(() => {
  switch (updater.status.value) {
    case "checking":
      return t("settings.updateChecking");
    case "downloading":
      return `${updater.downloadProgress.value}%`;
    case "ready":
      return t("settings.updateRestart");
    default:
      return t("settings.checkUpdate");
  }
});

async function onCheckUpdate() {
  if (updater.status.value === "ready") {
    await updater.relaunchApp();
    return;
  }
  if (updater.status.value === "checking" || updater.status.value === "downloading") {
    return;
  }
  const found = await updater.checkForUpdate();
  if (!found) {
    if (updater.status.value === "up-to-date") {
      ElMessage.success(t("settings.upToDate"));
    } else {
      ElMessage.error(t("settings.updateFail", { err: updater.errorMsg.value }));
    }
  } else {
    await updater.startUpdateFromNotification();
  }
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
  <div class="view-root">
    <!-- <div class="view-toolbar">
      <span class="toolbar-title">{{ t("settings.title") }}</span>
    </div> -->
    <el-tabs
      v-model="activeTab"
      class="flex-1 min-h-0 flex flex-col [&_.el-tabs\_\_header]:mx-2 [&_.el-tabs\_\_header]:mb-2 [&_.el-tabs\_\_content]:flex-1 [&_.el-tabs\_\_content]:min-h-0 [&_.el-tab-pane]:h-full"
    >
      <el-tab-pane :label="t('settings.groupGeneral')" name="general">
        <el-scrollbar class="view-scroll">
          <div class="py-1.5 px-0.5">
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.appName") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.appNameDesc") }}</div>
              </div>
              <el-input
                v-model="appNameModel"
                size="small"
                maxlength="24"
                class="w-[150px]"
                :placeholder="config.settings.app_name ?? 'Desk Pet'"
                @change="onAppNameChange"
              />
            </div>
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.lang") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.langDesc") }}</div>
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
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.alwaysOnTop") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.alwaysOnTopDesc") }}</div>
              </div>
              <el-switch
                :model-value="config.settings.always_on_top"
                @change="onAlwaysOnTop"
              />
            </div>
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.autostart") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.autostartDesc") }}</div>
              </div>
              <el-switch v-model="autostart" @change="onAutostart" />
            </div>
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.shortcut") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">
                  {{ config.platform === "macos" ? "Command + Shift + P" : "Ctrl + Shift + P" }}
                </div>
              </div>
            </div>
            <div
              v-if="config.platform === 'macos'"
              class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg"
            >
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.dock") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.dockDesc") }}</div>
              </div>
              <el-switch
                :model-value="config.settings.show_dock ?? true"
                @change="(v: string | number | boolean) => config.setShowDock(Boolean(v))"
              />
            </div>
          </div>
        </el-scrollbar>
      </el-tab-pane>
      <el-tab-pane :label="t('settings.groupAppearance')" name="appearance">
        <el-scrollbar class="view-scroll">
          <div class="py-1.5 px-0.5">
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.theme") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.themeDesc") }}</div>
              </div>
              <el-select v-model="themeModel" size="small" style="width: 120px">
                <el-option :label="t('settings.themeAuto')" value="auto" />
                <el-option :label="t('settings.themeLight')" value="light" />
                <el-option :label="t('settings.themeDark')" value="dark" />
              </el-select>
            </div>
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.avatar") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.avatarDesc") }}</div>
              </div>
              <div class="flex items-center gap-2">
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
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.accent") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.accentDesc") }}</div>
              </div>
              <div class="flex items-center gap-2">
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
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.spriteSize") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">{{ t("settings.spriteSizeDesc") }}</div>
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
          </div>
        </el-scrollbar>
      </el-tab-pane>
      <el-tab-pane :label="t('settings.groupAbout')" name="about">
        <el-scrollbar class="view-scroll">
          <div class="py-1.5 px-0.5">
            <div class="flex items-center gap-3 py-3 px-2.5 rounded-[10px] transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg">
              <div class="mr-auto">
                <div class="text-[13px] font-medium text-dp-primary">{{ t("settings.about") }}</div>
                <div class="text-xs text-dp-tertiary mt-0.5">Desk Pet v{{ version }} · {{ config.platform || "..." }}</div>
              </div>
              <el-button size="small" :disabled="updateBtnDisabled" @click="onCheckUpdate">
                {{ updateBtnText }}
              </el-button>
            </div>
          </div>
        </el-scrollbar>
      </el-tab-pane>
    </el-tabs>
  </div>
</template>
