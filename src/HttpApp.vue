<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { Close, Document, Fold, Expand, Promotion, Star } from "@element-plus/icons-vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import {
  clearHttpHistory,
  deleteHttpGroup,
  deleteHttpSaved,
  httpRequest,
  listHttpHistory,
  listHttpSaved,
  renameHttpGroup,
  upsertHttpSaved,
} from "@/api/toolbox";
import { copyToClipboard } from "@/api/clipboard";
import { parseCurl, toCurl } from "@/lib/curlParser";
import { toFetch, toPython } from "@/lib/codeGen";
import RequestEditor from "@/components/http/RequestEditor.vue";
import ResponseViewer from "@/components/http/ResponseViewer.vue";
import SidebarPanel from "@/components/http/SidebarPanel.vue";
import type {
  HttpHistoryItem,
  HttpRequestSpec,
  HttpResponseData,
  HttpSavedItem,
} from "@/types";

const { t } = useI18n();

const HTTP_MIN = { width: 760, height: 560 };

/* ---------- 外观同步 ---------- */
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

function startResize(direction: ResizeDirection, event: PointerEvent) {
  startWindowResize(direction, event, HTTP_MIN);
}

onMounted(() => {
  void syncAppearance();
  void refreshSaved();
  void refreshHistory();
  window.addEventListener("focus", syncAppearance);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", syncAppearance);
});

/* ---------- 请求状态 ---------- */
function emptySpec(): HttpRequestSpec {
  return {
    url: "",
    method: "GET",
    params: [],
    headers: [],
    body: "",
    bodyType: "json",
    form: [],
    formFiles: [],
    auth: { type: "none", token: "", username: "", password: "", key: "", value: "", in: "header" },
    timeoutMs: undefined,
  };
}

const spec = ref<HttpRequestSpec>(emptySpec());
  const resp = ref<HttpResponseData | null>(null);
  const abortCtrl = ref<AbortController | null>(null);
  /** 响应历史请求键：方法 + 原始 URL（与 Rust 侧 spec_key 格式一致） */
  const specKey = computed(
    () => `${spec.value.method.trim().toUpperCase()} ${spec.value.url.trim()}`,
  );
const busy = ref(false);

const sidebarVisible = ref(true);
const activeSavedId = ref<string | null>(null);
const activeHistoryId = ref<string | null>(null);

function normalizeLoaded(raw: Partial<HttpRequestSpec> | null): HttpRequestSpec {
  return { ...emptySpec(), ...(raw ?? {}) };
}

  async function send() {
    if (busy.value) {
      return;
    }
    const target = spec.value.url.trim();
    if (!/^https?:\/\//i.test(target)) {
      ElMessage.warning(t("toolbox.tools.http.invalidUrl"));
      return;
    }
    spec.value.url = target;
    busy.value = true;
    const ctrl = new AbortController();
    abortCtrl.value = ctrl;
    try {
      resp.value = await httpRequest(spec.value, { signal: ctrl.signal });
    } catch (e) {
      // 用户主动取消时保留上一次响应，不提示错误
      if ((e as DOMException)?.name !== "AbortError") {
        resp.value = null;
        ElMessage.error(String(e));
      }
    } finally {
      busy.value = false;
      abortCtrl.value = null;
      // 成败均由 Rust 侧记入历史，发送后刷新
      void refreshHistory();
    }
  }

  /** 取消当前请求 */
  function stop() {
    abortCtrl.value?.abort();
  }

/* ---------- 收藏 ---------- */
const saved = ref<HttpSavedItem[]>([]);
const history = ref<HttpHistoryItem[]>([]);

async function refreshSaved() {
  try {
    saved.value = await listHttpSaved();
  } catch {
    saved.value = [];
  }
}

async function refreshHistory() {
  try {
    history.value = await listHttpHistory();
  } catch {
    history.value = [];
  }
}

function loadSaved(item: HttpSavedItem) {
  spec.value = normalizeLoaded(tryParseSpec(item.spec));
  activeSavedId.value = item.id;
  activeHistoryId.value = null;
  resp.value = null;
}

function loadHistory(item: HttpHistoryItem) {
  spec.value = normalizeLoaded(tryParseSpec(item.spec));
  activeHistoryId.value = item.id;
  activeSavedId.value = null;
  resp.value = null;
}

function tryParseSpec(json: string): Partial<HttpRequestSpec> | null {
  try {
    return JSON.parse(json || "{}") as Partial<HttpRequestSpec>;
  } catch {
    return null;
  }
}

/* ---------- 保存对话框 ---------- */
const saveDlg = ref(false);
const editingId = ref<string | null>(null);
const saveName = ref("");
const saveGroup = ref("");

const groupOptions = computed(() => [...new Set(saved.value.map((i) => i.group_name ?? ""))]);
const editingItem = computed(() => saved.value.find((i) => i.id === editingId.value) ?? null);

/** 新建时用地址路径段作为默认名 */
function guessName(): string {
  const url = spec.value.url.trim();
  try {
    const path = new URL(url).pathname;
    const seg = path.split("/").filter(Boolean).pop();
    return seg ?? "";
  } catch {
    return "";
  }
}

function openSaveDialog(item?: HttpSavedItem) {
  editingId.value = item?.id ?? null;
  saveName.value = item?.name ?? guessName();
  saveGroup.value = item?.group_name ?? "";
  saveDlg.value = true;
}

async function confirmSave() {
  const name = saveName.value.trim();
  if (!name) {
    ElMessage.warning(t("toolbox.tools.http.nameRequired"));
    return;
  }
  const existed = editingItem.value;
  // 新条目排在目标组末尾
  const siblings = saved.value.filter((i) => (i.group_name ?? "") === saveGroup.value);
  const maxSort = siblings.reduce((acc, i) => Math.max(acc, i.sort ?? 0), 0);
  const item: HttpSavedItem = {
    id: existed?.id ?? "",
    group_name: saveGroup.value.trim(),
    name,
    spec: JSON.stringify(spec.value),
    sort: existed ? existed.sort : maxSort + 1,
    pinned: existed?.pinned ?? false,
    create_time: existed?.create_time ?? "",
    update_time: "",
  };
  try {
    await upsertHttpSaved(item);
    saveDlg.value = false;
    ElMessage.success(t("toolbox.tools.http.savedTo", { group: item.group_name || t("toolbox.tools.http.groupDefault") }));
    void refreshSaved();
    if (item.id) {
      activeSavedId.value = item.id;
    }
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 删除 / 重命名 ---------- */
async function removeSaved(item: HttpSavedItem) {
  try {
    await ElMessageBox.confirm(
      t("toolbox.tools.http.deleteSavedConfirm", { name: item.name || savedUrlOf(item) }),
      t("common.delete"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  try {
    await deleteHttpSaved(item.id);
    if (activeSavedId.value === item.id) {
      activeSavedId.value = null;
    }
    void refreshSaved();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function savedUrlOf(item: HttpSavedItem): string {
  return tryParseSpec(item.spec)?.url ?? "";
}

async function removeGroup(group: string) {
  const willDrop = saved.value.filter((i) => (i.group_name ?? "") === group);
  try {
    await ElMessageBox.confirm(
      t("toolbox.tools.http.deleteGroupConfirm", { name: group || t("toolbox.tools.http.groupDefault") }),
      t("toolbox.tools.http.deleteGroup"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  try {
    await deleteHttpGroup(group);
    if (activeSavedId.value && willDrop.some((i) => i.id === activeSavedId.value)) {
      activeSavedId.value = null;
    }
    void refreshSaved();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function doRenameGroup(group: string) {
  let value: string;
  try {
    const result = await ElMessageBox.prompt(
      t("toolbox.tools.http.groupNameLabel"),
      t("toolbox.tools.http.renameGroup"),
      { inputValue: group, inputPattern: /\S/, inputErrorMessage: t("toolbox.tools.http.nameRequired") },
    );
    value = result.value.trim();
  } catch {
    return;
  }
  if (!value || value === group) {
    return;
  }
  try {
    await renameHttpGroup(group, value);
    void refreshSaved();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function clearHistoryAll() {
  try {
    await ElMessageBox.confirm(t("toolbox.tools.http.clearHistoryConfirm"), t("toolbox.tools.http.clearHistory"), {
      type: "warning",
    });
  } catch {
    return;
  }
  try {
    await clearHttpHistory();
    activeHistoryId.value = null;
    void refreshHistory();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- cURL 导入 / 导出 ---------- */
const curlDlg = ref(false);
const curlText = ref("");

function applyCurl() {
  try {
    const parsed = parseCurl(curlText.value);
    spec.value = normalizeLoaded(parsed);
    curlDlg.value = false;
    curlText.value = "";
    activeSavedId.value = null;
    activeHistoryId.value = null;
    ElMessage.success(t("toolbox.tools.http.curlOk"));
  } catch {
    ElMessage.error(t("toolbox.tools.http.curlFail"));
  }
}

async function copyCode(kind: "curl" | "fetch" | "python") {
  try {
    const code = kind === "curl" ? toCurl(spec.value) : kind === "fetch" ? toFetch(spec.value) : toPython(spec.value);
    await copyToClipboard(code);
    ElMessage.success(t("toolbox.tools.http.copied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** 置顶/取消置顶：翻转后走 upsert（store 侧幂等） */
async function togglePin(item: HttpSavedItem) {
  try {
    await upsertHttpSaved({ ...item, pinned: !item.pinned });
    void refreshSaved();
  } catch (e) {
    ElMessage.error(String(e));
  }
}
</script>

<template>
  <div class="w-screen h-screen bg-[var(--el-bg-color)] text-dp-primary overflow-hidden select-none flex flex-col">
    <!-- 顶栏(拖拽区) -->
    <div
      class="h-12 shrink-0 flex items-center gap-2 px-3 bg-dp-card-bg border-b border-dp-divider"
      data-tauri-drag-region
    >
      <el-button
        size="small"
        text
        :icon="sidebarVisible ? Fold : Expand"
        :title="t('toolbox.tools.http.toggleSidebar')"
        class="shrink-0"
        @click="sidebarVisible = !sidebarVisible"
      />
      <span class="text-sm font-semibold text-dp-primary shrink-0" data-tauri-drag-region>
        {{ t("toolbox.tools.http.title") }}
      </span>
      <span class="text-[11px] text-dp-tertiary truncate" data-tauri-drag-region>
        {{ t("toolbox.tools.http.proxyHint") }}
      </span>
      <el-button size="small" :icon="Star" class="ml-auto shrink-0" @click="openSaveDialog()">
        {{ t("toolbox.tools.http.saveReq") }}
      </el-button>
      <el-button size="small" :icon="Document" class="shrink-0" @click="curlDlg = true">
        {{ t("toolbox.tools.http.importCurl") }}
      </el-button>
      <el-button size="small" :icon="Close" circle text class="shrink-0" @click="closeWindow" />
    </div>

    <!-- 三栏主体:侧栏 / 请求 / 响应 -->
    <div class="flex-1 min-h-0 flex gap-3 px-3 pb-3 pt-2.5">
      <SidebarPanel
        v-if="sidebarVisible"
        :saved="saved"
        :history="history"
        :active-saved-id="activeSavedId"
        :active-history-id="activeHistoryId"
        @load-saved="loadSaved"
        @edit-saved="openSaveDialog"
        @delete-saved="removeSaved"
        @rename-group="doRenameGroup"
        @delete-group="removeGroup"
        @toggle-pin="togglePin"
        @load-history="loadHistory"
        @clear-history="clearHistoryAll"
      />

      <!-- 中:请求编辑(固定比例，与原双栏布局一致；侧栏收起后响应区变宽) -->
      <RequestEditor :spec="spec" :busy="busy" class="w-[42%] shrink-0" @send="send" @stop="stop" @copy-code="copyCode" />

      <!-- 右:响应(撑满剩余宽度) -->
      <ResponseViewer
        :resp="resp"
        :busy="busy"
        :spec-key="specKey"
        :sse-active="false"
        :sse-events="[]"
        :sse-stats="null"
        class="flex-1 min-w-0"
      />
    </div>

    <!-- 保存到收藏 -->
    <el-dialog
      v-model="saveDlg"
      :title="editingId ? t('toolbox.tools.http.editSaved') : t('toolbox.tools.http.saveReq')"
      width="320px"
      :append-to-body="false"
    >
      <div class="flex flex-col gap-3">
        <el-input
          v-model="saveName"
          :placeholder="t('toolbox.tools.http.namePh')"
          :maxlength="80"
          class="[&_.el-input\_\_inner]:text-[12px]"
        />
        <el-select
          v-model="saveGroup"
          filterable
          allow-create
          default-first-option
          :placeholder="t('toolbox.tools.http.groupSelectPh')"
          class="[&_.el-input\_\_inner]:text-[12px]"
        >
          <el-option
            v-for="g in groupOptions"
            :key="g"
            :label="g || t('toolbox.tools.http.groupDefault')"
            :value="g"
          />
        </el-select>
      </div>
      <template #footer>
        <el-button size="small" @click="saveDlg = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" @click="confirmSave">{{ t("common.confirm") }}</el-button>
      </template>
    </el-dialog>

    <!-- 导入 cURL -->
    <el-dialog
      v-model="curlDlg"
      :title="t('toolbox.tools.http.importCurl')"
      width="460px"
      :append-to-body="false"
    >
      <el-input
        v-model="curlText"
        type="textarea"
        resize="none"
        :rows="9"
        :placeholder="t('toolbox.tools.http.curlPh')"
        class="[&_.el-textarea\_\_inner]:text-[11.5px] [&_.el-textarea\_\_inner]:font-mono [&_.el-textarea\_\_inner]:select-text"
      />
      <template #footer>
        <el-button size="small" @click="curlDlg = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" :icon="Promotion" @click="applyCurl">
          {{ t("toolbox.tools.http.curlApply") }}
        </el-button>
      </template>
    </el-dialog>

    <!-- 无边框窗口的边缘/四角缩放手柄 -->
    <div class="pointer-events-none fixed inset-0 z-50" aria-hidden="true">
      <div class="rs-hint left-0 right-0 top-0 h-[5px] cursor-ns-resize" @pointerdown.prevent="startResize('North', $event)" />
      <div class="rs-hint left-0 right-0 bottom-0 h-[5px] cursor-ns-resize" @pointerdown.prevent="startResize('South', $event)" />
      <div class="rs-hint bottom-0 left-0 top-0 w-[5px] cursor-ew-resize" @pointerdown.prevent="startResize('West', $event)" />
      <div class="rs-hint bottom-0 right-0 top-0 w-[5px] cursor-ew-resize" @pointerdown.prevent="startResize('East', $event)" />
      <div class="rs-hint left-0 top-0 h-[18px] w-[18px] cursor-nwse-resize" @pointerdown.prevent="startResize('NorthWest', $event)" />
      <div class="rs-hint right-0 top-0 h-[18px] w-[18px] cursor-nesw-resize" @pointerdown.prevent="startResize('NorthEast', $event)" />
      <div class="rs-hint bottom-0 left-0 h-[18px] w-[18px] cursor-nesw-resize" @pointerdown.prevent="startResize('SouthWest', $event)" />
      <div class="rs-hint bottom-0 right-0 h-[18px] w-[18px] cursor-nwse-resize" @pointerdown.prevent="startResize('SouthEast', $event)" />
    </div>
  </div>
</template>
