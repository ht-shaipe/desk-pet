<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ElMessage } from "element-plus";
import { Close, Loading } from "@element-plus/icons-vue";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize } from "@/lib/windowCtl";
import { useUpdater } from "@/composables/useUpdater";

const { t } = useI18n();
const updater = useUpdater();

const phase = computed(() => updater.status.value);
const downloadProgress = computed(() => updater.downloadProgress.value);

const SKIP_KEY = "updater.skippedVersion";

async function syncAppearance() {
  try {
    const settings = await getSettings();
    applyTheme(settings.theme);
    applyAccentColor(settings.accent_color);
    if (settings.locale === "zh-CN" || settings.locale === "en") {
      setAppLocale(settings.locale);
    }
  } catch {
    // 读取失败时保持默认外观
  }
}

async function closeWindow() {
  try {
    await getCurrentWindow().close();
  } catch {
    // 窗口可能已被销毁
  }
}

/** 忽略此版本：记录版本号（主窗口据此不再弹窗），可在设置-关于里手动更新。
 *  下载进行中不允许忽略：此时关窗会让用户错过重启安装的提示。 */
function skipVersion() {
  if (updater.status.value === "downloading") {
    return;
  }
  const version = updater.updateInfo.value?.version;
  if (version) {
    localStorage.setItem(SKIP_KEY, version);
  }
  void closeWindow();
}

async function startUpdate() {
  await updater.downloadAndInstall();
  if (updater.status.value === "ready") {
    // 下载完成后自动重启安装
    await updater.relaunchApp();
  } else if (updater.status.value === "error") {
    ElMessage.error(updater.errorMsg.value || t("updaterWin.error"));
  }
}

async function retry() {
  await updater.checkForUpdate(true);
}

function onKeydown(event: KeyboardEvent) {
  // 无边框窗口：拖拽区之外的键盘退出路径（Escape = 忽略）
  if (event.key === "Escape") {
    skipVersion();
  }
}

onMounted(async () => {
  await syncAppearance();
  window.addEventListener("keydown", onKeydown);
  await updater.checkForUpdate(true);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
});

const versionLabel = computed(() => `v${updater.updateInfo.value?.version ?? ""}`);
const notesLines = computed(() =>
  (updater.updateInfo.value?.body ?? "").split(/\r?\n/).filter((line) => line.trim().length > 0),
);
</script>

<template>
  <div class="relative flex h-screen min-h-0 flex-col text-dp-secondary">
    <!-- 不透明底：card-bg 实色打底 + panel-bg 同色覆盖（panel-bg 暗色是 0.97 透明，
         直接做弹窗底会透出后面窗口的内容） -->
    <div class="absolute inset-0 bg-dp-card-bg" aria-hidden="true" />
    <div class="absolute inset-0 bg-[var(--dp-panel-bg)]" aria-hidden="true" />

    <!-- 顶栏：拖拽区 + 关闭 -->
    <div
      class="relative flex shrink-0 items-center gap-2 px-3.5 pb-1.5 pt-3"
      data-tauri-drag-region
    >
      <span class="size-1.5 shrink-0 rounded-full bg-dp-accent" aria-hidden="true" />
      <span class="text-[12.5px] font-bold uppercase tracking-[1.5px] text-dp-muted">
        {{ t("updaterWin.title") }}
      </span>
      <el-icon
        class="ml-auto cursor-pointer text-dp-tertiary hover:text-dp-primary"
        :size="13"
        @click="skipVersion"
      >
        <Close />
      </el-icon>
    </div>

    <div class="relative flex min-h-0 flex-1 flex-col px-3.5 pb-3">
      <!-- 检查中 -->
      <div v-if="phase === 'checking'" class="flex flex-1 items-center justify-center gap-2">
        <el-icon class="is-loading text-dp-accent" :size="14"><Loading /></el-icon>
        <span class="text-[12.5px] text-dp-muted">{{ t("updaterWin.checking") }}</span>
      </div>

      <!-- 已是最新（竞态兜底） -->
      <div v-else-if="phase === 'up-to-date'" class="flex flex-1 flex-col items-center justify-center gap-2">
        <span class="text-[13px] text-dp-secondary">{{ t("updaterWin.upToDate") }}</span>
        <el-button size="small" @click="closeWindow">{{ t("updaterWin.close") }}</el-button>
      </div>

      <!-- 检查失败 -->
      <div v-else-if="phase === 'error' && !updater.updateInfo.value" class="flex flex-1 flex-col items-center justify-center gap-2">
        <span class="px-2 text-center text-[12px] leading-[1.5] text-[#e6a23c]">
          {{ updater.errorMsg.value || t("updaterWin.error") }}
        </span>
        <el-button size="small" @click="retry">{{ t("updaterWin.retry") }}</el-button>
      </div>

      <!-- 发现新版本 -->
      <template v-else-if="updater.updateInfo.value">
        <div class="mt-0.5 flex items-baseline gap-2">
          <span class="text-[19px] font-bold text-dp-primary">{{ versionLabel }}</span>
          <span class="text-[12px] text-dp-tertiary">
            {{ t("updaterWin.current", { version: `v${updater.currentVersion.value}` }) }}
          </span>
        </div>
        <div
          v-if="notesLines.length > 0"
          class="mt-2.5 max-h-16 overflow-y-auto rounded-[8px] bg-dp-btn-bg px-2.5 py-1.5"
        >
          <p
            v-for="(line, index) in notesLines"
            :key="index"
            class="m-0 whitespace-pre-line break-words text-[12px] leading-[1.55] text-dp-muted"
          >
            {{ line }}
          </p>
        </div>

        <!-- 下载进度 -->
        <div v-if="phase === 'downloading'" class="mt-3">
          <div class="mb-1 flex items-center gap-1.5 text-[12px] text-dp-muted">
            <el-icon class="is-loading text-dp-accent" :size="11"><Loading /></el-icon>
            {{ t("updaterWin.downloading") }} {{ downloadProgress }}%
          </div>
          <div class="h-[4px] overflow-hidden rounded-[99px] bg-dp-btn-bg">
            <div
              class="h-full rounded-[99px] bg-[linear-gradient(90deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))] transition-[width] duration-200"
              :style="{ width: `${downloadProgress}%` }"
            />
          </div>
        </div>
        <div v-else-if="phase === 'ready'" class="mt-3 text-[12px] text-[#67c23a]">
          {{ t("updaterWin.ready") }}
        </div>

        <!-- 操作区：沉底布局，说明再长也保持按钮贴底 -->
        <div class="mt-auto flex gap-2 pt-3 [&_.el-button]:ml-0">
          <el-button class="flex-1" :disabled="phase === 'downloading'" @click="skipVersion">
            {{ t("updaterWin.later") }}
          </el-button>
          <el-button
            type="primary"
            class="flex-1"
            :loading="phase === 'downloading'"
            @click="startUpdate"
          >
            {{ phase === 'ready' ? t("updaterWin.restart") : t("updaterWin.updateNow") }}
          </el-button>
        </div>
        <div class="mt-1.5 text-[10.5px] leading-[1.45] text-dp-tertiary">
          {{ t("updaterWin.skipHint") }}
        </div>
      </template>
    </div>

    <!-- 无边框窗口边缘缩放手柄（resizable=false 时基本用不到，保留四角以防系统调整） -->
    <div class="pointer-events-none fixed inset-0 z-50" aria-hidden="true">
      <div
        class="rs-hint right-0 bottom-0 h-[14px] w-[14px] cursor-nwse-resize"
        @pointerdown.prevent="startWindowResize('SouthEast', $event, { width: 340, height: 260 })"
      />
    </div>
  </div>
</template>
