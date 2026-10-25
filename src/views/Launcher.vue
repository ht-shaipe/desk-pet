<script setup lang="ts">
import type { Component } from "vue";
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import {
  Bell, Connection, Delete, EditPen, Lock, Monitor, Moon, MoonNight, Mute, Plus, Setting,
} from "@element-plus/icons-vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getAppIcon, getAppIconPath, launchItem, listApps } from "@/api/launcher";
import { quickAction } from "@/api/system";
import { useLauncherStore } from "@/stores/launcher";
import { useConfigStore } from "@/stores/config";
import type { AppEntry, LaunchType, LauncherItem } from "@/types";

const { t } = useI18n();
const store = useLauncherStore();
const config = useConfigStore();
const editMode = ref(false);
const dialogVisible = ref(false);
const editingId = ref<string | null>(null);

// ---------- 快捷操作（原 QuickSettings 视图并入，子 tab 切换） ----------

type LaunchTab = "launch" | "quick";
const tab = ref<LaunchTab>("launch");

/** 子 tab 选中态：品牌色胶囊 vs 静默灰字。 */
function subTabCls(name: LaunchTab): string {
  return tab.value === name
    ? "bg-dp-accent text-white shadow-[0_2px_8px_-2px_color-mix(in_srgb,var(--dp-accent)_55%,transparent)]"
    : "text-dp-tertiary hover:text-dp-secondary";
}

interface QuickItem {
  action: string;
  label: string;
  icon: Component;
  macOnly?: boolean;
}

const quickActions = computed<QuickItem[]>(() => [
  { action: "dark-mode", label: t("quick.darkMode"), icon: Moon, macOnly: true },
  { action: "mute", label: t("quick.mute"), icon: Mute, macOnly: true },
  { action: "lock", label: t("quick.lock"), icon: Lock },
  { action: "sleep", label: t("quick.sleep"), icon: MoonNight },
  { action: "open-settings:main", label: t("quick.sysSettings"), icon: Setting },
  { action: "open-settings:sound", label: t("quick.sound"), icon: Bell },
  { action: "open-settings:display", label: t("quick.display"), icon: Monitor },
  { action: "open-settings:network", label: t("quick.network"), icon: Connection },
]);

const quickVisible = computed(() =>
  quickActions.value.filter((a) => !a.macOnly || config.platform === "macos")
);

async function onQuickAction(item: QuickItem) {
  try {
    await quickAction(item.action);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function emptyForm(): LauncherItem {
  return {
    id: "",
    name: "",
    icon: "",
    item_type: "app",
    value: "",
    color: store.nextColor(),
  };
}

const form = reactive<LauncherItem>(emptyForm());

const typeOptions = computed<Array<{ label: string; value: LaunchType }>>(() => [
  { label: t("launcher.typeApp"), value: "app" },
  { label: t("launcher.typeUrl"), value: "url" },
  { label: t("launcher.typeCmd"), value: "command" },
]);

/** 按类型分组；管理态保留空分类，方便直接往该分类里添加。 */
const groups = computed(() =>
  typeOptions.value
    .map((opt) => ({
      ...opt,
      items: store.items.filter((item) => item.item_type === opt.value),
    }))
    .filter((group) => group.items.length > 0 || editMode.value),
);

const valuePlaceholder = computed(() => {
  const mac = config.platform !== "windows";
  switch (form.item_type) {
    case "url":
      return t("launcher.ph.url");
    case "command":
      return mac ? t("launcher.ph.cmdMac") : t("launcher.ph.cmdWin");
    default:
      return mac ? t("launcher.ph.appMac") : t("launcher.ph.appWin");
  }
});

async function onLaunch(item: LauncherItem) {
  if (editMode.value) {
    return;
  }
  try {
    await launchItem(item);
  } catch (e) {
    ElMessage.error(t("launcher.launchFail", { err: String(e) }));
  }
}

/** 系统应用图标（asset 协议地址），提取失败回退为文字图标。 */
const iconUrls = ref<Record<string, string>>({});

async function loadIcons() {
  for (const item of store.items) {
    if (item.item_type !== "app" || iconUrls.value[item.id]) {
      continue;
    }
    try {
      const path = await getAppIcon(item);
      if (path) {
        iconUrls.value[item.id] = convertFileSrc(path);
      }
    } catch {
      // 无法提取时保持文字图标
    }
  }
}

watch(() => store.items.map((i) => `${i.id}:${i.item_type}:${i.value}`).join("|"), loadIcons, {
  immediate: true,
});

function openAdd(type: LaunchType = "app") {
  editingId.value = null;
  Object.assign(form, emptyForm(), { item_type: type });
  if (type === "app") {
    void ensureApps();
  }
  dialogVisible.value = true;
}

function openEdit(item: LauncherItem) {
  editingId.value = item.id;
  Object.assign(form, item);
  void ensureApps();
  dialogVisible.value = true;
}

/** 系统应用列表（懒加载 + 缓存）。 */
const apps = ref<AppEntry[]>([]);
let appsLoaded = false;

/** 应用下拉列表图标：key 为 app.value，"" 表示已尝试但无图标。 */
const appIconUrls = ref<Record<string, string>>({});
let appIconsStarted = false;

/** 后台按有限并发提取全部应用图标，图标随加载逐条出现在下拉列表中。 */
async function loadAppIcons() {
  if (appIconsStarted) {
    return;
  }
  appIconsStarted = true;
  const values = apps.value.map((a) => a.value);
  let idx = 0;
  const WORKERS = 4;
  const worker = async () => {
    while (idx < values.length) {
      const value = values[idx++];
      if (appIconUrls.value[value] !== undefined) {
        continue;
      }
      try {
        const path = await getAppIconPath(value);
        if (path) {
          appIconUrls.value[value] = convertFileSrc(path);
        } else {
          appIconUrls.value[value] = "";
        }
      } catch {
        appIconUrls.value[value] = "";
      }
    }
  };
  await Promise.all(Array.from({ length: WORKERS }, worker));
}

async function ensureApps() {
  if (appsLoaded) {
    return;
  }
  appsLoaded = true;
  try {
    apps.value = await listApps();
    void loadAppIcons();
  } catch {
    appsLoaded = false;
  }
}

/** 选择应用时自动补全名称。 */
function onAppPicked(value: string) {
  const app = apps.value.find((a) => a.value === value);
  if (app && !form.name.trim()) {
    form.name = app.name;
  }
}

/** 切换为应用类型时确保应用列表已加载。 */
function onTypeChange() {
  if (form.item_type === "app") {
    void ensureApps();
  }
}

function confirmDialog() {
  if (!form.name.trim() || !form.value.trim()) {
    ElMessage.warning(t("launcher.emptyWarn"));
    return;
  }
  if (editingId.value) {
    store.updateItem({ ...form, id: editingId.value });
    ElMessage.success(t("common.saved"));
  } else {
    store.addItem({
      ...form,
      id: `l-${Date.now()}-${Math.floor(Math.random() * 1000)}`,
      name: form.name.trim(),
      value: form.value.trim(),
    });
    ElMessage.success(t("common.added"));
  }
  dialogVisible.value = false;
}
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <!-- 子 tab：快速启动 / 快捷操作（macOS 分段胶囊：全圆角容器 + 选中品牌色） -->
      <div class="mr-auto inline-flex items-center rounded-full bg-dp-btn-bg p-[3px]">
        <button
          class="px-3 h-[22px] rounded-full text-[11.5px] leading-none font-medium cursor-pointer select-none transition-colors duration-150 ease-[ease]"
          :class="subTabCls('launch')"
          @click="tab = 'launch'"
        >
          {{ t("tabs.launcher") }}
        </button>
        <button
          class="px-3 h-[22px] rounded-full text-[11.5px] leading-none font-medium cursor-pointer select-none transition-colors duration-150 ease-[ease]"
          :class="subTabCls('quick')"
          @click="tab = 'quick'"
        >
          {{ t("tabs.quick") }}
        </button>
      </div>
      <el-button v-if="tab === 'launch' && !editMode" size="small" text :icon="Setting" @click="editMode = true">
        {{ t("launcher.manage") }}
      </el-button>
      <el-button v-else-if="tab === 'launch'" size="small" text type="primary" @click="editMode = false">
        {{ t("launcher.done") }}
      </el-button>
    </div>
    <el-scrollbar v-if="tab === 'launch'" class="view-scroll">
      <div v-if="store.items.length > 0 || editMode" class="flex flex-col gap-3.5 pt-2 px-0.5 pb-1">
        <section v-for="group in groups" :key="group.value">
          <div class="flex items-center gap-1.5 px-0.5 pb-0.5">
            <span class="text-xs font-semibold tracking-[0.02em] text-dp-secondary">{{ group.label }}</span>
            <span class="min-w-4 h-4 px-[5px] rounded-lg bg-dp-btn-bg text-dp-tertiary text-[10px] leading-4 text-center">{{ group.items.length }}</span>
          </div>
          <div class="grid grid-cols-4 gap-2.5">
            <div
              v-for="item in group.items"
              :key="item.id"
              class="relative flex flex-col items-center gap-1.5 min-w-0 py-2 px-0.5 rounded-[10px] cursor-pointer transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg"
              @click="editMode ? openEdit(item) : onLaunch(item)"
            >
              <span
                v-if="editMode"
                class="absolute top-0.5 right-1.5 inline-flex items-center justify-center w-4 h-4 rounded-full bg-[#f56c6c] text-white cursor-pointer"
                @click.stop="store.removeItem(item.id)"
              >
                <el-icon :size="8"><Delete /></el-icon>
              </span>
              <span
                class="inline-flex items-center justify-center w-[42px] h-[42px] rounded-full text-white text-[17px] font-semibold overflow-hidden"
                :style="iconUrls[item.id] ? {} : { background: item.color || '#909399' }"
              >
                <img v-if="iconUrls[item.id]" class="w-full h-full object-contain" :src="iconUrls[item.id]" alt="" />
                <template v-else>{{ item.icon || item.name.slice(0, 1) }}</template>
              </span>
              <span class="max-w-full text-xs text-dp-secondary truncate" :title="item.name">{{ item.name }}</span>
            </div>
            <div
              v-if="editMode"
              class="relative flex flex-col items-center gap-1.5 min-w-0 py-2 px-0.5 rounded-[10px] cursor-pointer transition-[background] duration-150 ease-[ease] hover:bg-dp-hover-bg"
              @click="openAdd(group.value)"
            >
              <span class="inline-flex items-center justify-center w-[42px] h-[42px] rounded-full overflow-hidden bg-dp-btn-hover-bg text-dp-muted">
                <el-icon :size="18"><Plus /></el-icon>
              </span>
              <span class="max-w-full text-xs text-dp-secondary truncate">{{ t("launcher.add") }}</span>
            </div>
          </div>
        </section>
      </div>
      <el-empty
        v-if="store.items.length === 0 && !editMode"
        :description="t('launcher.empty')"
        :image-size="60"
      />
      <div v-if="editMode" class="flex items-center gap-1 mt-2.5 text-dp-tertiary text-xs">
        <el-icon :size="12"><EditPen /></el-icon>
        {{ t("launcher.editHint") }}
      </div>
    </el-scrollbar>
    <!-- 快捷操作动作网格（原快捷操作视图）：el-scrollbar 统一滚动条样式 -->
    <el-scrollbar v-else class="view-scroll">
      <div class="grid grid-cols-2 gap-2.5 pt-2 px-0.5 pb-0.5 content-start">
        <button
          v-for="item in quickVisible"
          :key="item.action"
          class="flex flex-col items-center gap-2 py-4 px-1.5 bg-dp-card-bg border border-dp-card-border rounded-xl cursor-pointer transition-all duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover active:scale-[0.97]"
          @click="onQuickAction(item)"
        >
          <span class="text-dp-accent">
            <el-icon :size="20"><component :is="item.icon" /></el-icon>
          </span>
          <span class="text-[11.5px] text-dp-secondary">{{ item.label }}</span>
        </button>
      </div>
    </el-scrollbar>

    <el-dialog
      v-model="dialogVisible"
      :title="editingId ? t('launcher.editTitle') : t('launcher.addTitle')"
      width="300"
      append-to-body
    >
      <el-form label-position="top" size="small">
        <el-form-item :label="t('launcher.name')">
          <el-input v-model="form.name" :placeholder="t('launcher.namePh')" maxlength="20" />
        </el-form-item>
        <el-form-item :label="t('launcher.type')">
          <el-select v-model="form.item_type" @change="onTypeChange">
            <el-option v-for="opt in typeOptions" :key="opt.value" :label="opt.label" :value="opt.value" />
          </el-select>
        </el-form-item>
        <el-form-item :label="t('launcher.value')">
          <el-select
            v-if="form.item_type === 'app'"
            v-model="form.value"
            filterable
            allow-create
            default-first-option
            :placeholder="valuePlaceholder"
            @change="onAppPicked"
          >
            <el-option v-for="app in apps" :key="app.value" :label="app.name" :value="app.value">
              <div class="flex items-center gap-1.5 min-w-0">
                <span class="inline-flex shrink-0 items-center justify-center w-[18px] h-[18px] rounded-[4px] overflow-hidden bg-dp-btn-bg">
                  <img
                    v-if="appIconUrls[app.value]"
                    class="w-full h-full object-contain"
                    :src="appIconUrls[app.value]"
                    alt=""
                  />
                </span>
                <span class="truncate">{{ app.name }}</span>
              </div>
            </el-option>
          </el-select>
          <el-input v-else v-model="form.value" :placeholder="valuePlaceholder" />
        </el-form-item>
        <el-form-item :label="t('launcher.iconLabel')">
          <el-input v-model="form.icon" maxlength="2" :placeholder="t('launcher.iconPh')" />
        </el-form-item>
        <el-form-item :label="t('launcher.color')">
          <el-color-picker v-model="form.color" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button size="small" @click="dialogVisible = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" @click="confirmDialog">{{ t("common.confirm") }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>
