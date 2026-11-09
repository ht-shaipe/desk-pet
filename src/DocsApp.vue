<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox, type TableInstance } from "element-plus";
import {
  Aim,
  Close,
  CopyDocument,
  Delete,
  Download,
  FolderAdd,
  FolderOpened,
  Loading,
  Refresh,
  Search,
} from "@element-plus/icons-vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import { copyToClipboard } from "@/api/clipboard";
import { documentConvert, saveDocumentMarkdown } from "@/api/toolbox";
import { imagesOpenPath, imagesRevealPath } from "@/api/images";
import {
  docsAddDir,
  docsDeleteItem,
  docsListDirs,
  docsListExts,
  docsQuery,
  docsRemoveDir,
  docsScanStart,
  docsTaskCancel,
} from "@/api/docs";
import type { DocsScanProgress, DocDirInfo, DocExtStat, DocItemInfo } from "@/types";

const { t, locale } = useI18n();

const DOCS_MIN = { width: 860, height: 600 };
const PAGE_SIZE = 200;

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
  startWindowResize(direction, event, DOCS_MIN);
}

/* ---------- 数据状态 ---------- */
const dirs = ref<DocDirInfo[]>([]);
const searchInput = ref("");
const search = ref("");
const items = ref<DocItemInfo[]>([]);
const hasMore = ref(false);
const total = ref(0);
const loading = ref(false);

/** 各目录扫描进行态(事件驱动) */
const scanState = reactive<Record<number, { scanned: number }>>({});
const scanChip = computed(() => {
  let scanned = 0;
  for (const s of Object.values(scanState)) {
    scanned += s.scanned;
  }
  return { scanned, active: Object.keys(scanState).length > 0 };
});

const dirFilter = ref<number | null>(null);
const exts = ref<DocExtStat[]>([]);
const extFilter = ref<string | null>(null);
/** 表头排序(服务端执行,分页顺序一致) */
const sortBy = ref("mtime");
const sortOrder = ref<"asc" | "desc">("desc");
const tableRef = ref<TableInstance>();
/** 回写默认排序箭头时拦截由此触发的 sort-change,避免递归 */
let restoringSort = false;

function onSortChange({ prop, order }: { prop: string; order: "ascending" | "descending" | null }) {
  if (restoringSort) {
    return;
  }
  if (!order) {
    // 点击表头取消排序:回退默认排序,并把箭头同步回默认列
    sortBy.value = "mtime";
    sortOrder.value = "desc";
    restoringSort = true;
    tableRef.value?.sort("mtime", "descending");
    void nextTick(() => {
      restoringSort = false;
    });
  } else {
    sortBy.value = prop;
    sortOrder.value = order === "ascending" ? "asc" : "desc";
  }
  void refresh();
}

let searchTimer: ReturnType<typeof setTimeout> | undefined;
watch(searchInput, (v) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    search.value = v.trim();
  }, 300);
});

async function loadDirs() {
  try {
    dirs.value = await docsListDirs();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function loadExts() {
  try {
    exts.value = await docsListExts();
    // 当前选中的类型已无文档时自动取消筛选
    if (extFilter.value && !exts.value.some((e) => e.ext === extFilter.value)) {
      extFilter.value = null;
    }
  } catch {
    // 类型列表属增强展示,失败静默
  }
}

async function loadMore() {
  if (loading.value) {
    return;
  }
  loading.value = true;
  try {
    const page = await docsQuery({
      dirId: dirFilter.value ?? undefined,
      search: search.value,
      ext: extFilter.value ?? undefined,
      sortBy: sortBy.value,
      sortOrder: sortOrder.value,
      limit: PAGE_SIZE,
      offset: items.value.length,
    });
    items.value.push(...page.items);
    hasMore.value = page.hasMore;
    total.value = page.total;
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    loading.value = false;
  }
}

async function refresh() {
  items.value = [];
  hasMore.value = false;
  await loadMore();
}

function onMainScroll(e: Event) {
  const el = e.target as HTMLElement;
  if (hasMore.value && !loading.value && el.scrollTop + el.clientHeight >= el.scrollHeight - 400) {
    void loadMore();
  }
}

watch([dirFilter, search, extFilter], () => {
  void refresh();
});

/* ---------- 目录管理 ---------- */
async function pickDir() {
  try {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string" || !picked) {
      return;
    }
    const dir = await docsAddDir(picked);
    await loadDirs();
    dirFilter.value = dir.id;
    void docsScanStart(dir.id).catch((e) => ElMessage.error(String(e)));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function rescan(dirId: number) {
  try {
    await docsScanStart(dirId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function removeDir(dir: DocDirInfo) {
  try {
    await ElMessageBox.confirm(
      t("docs.removeDirConfirm", { n: dir.item_count }),
      t("docs.removeDir"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  try {
    await docsRemoveDir(dir.id);
    if (dirFilter.value === dir.id) {
      dirFilter.value = null;
    }
    await loadDirs();
    await loadExts();
    await refresh();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function cancelAllScans() {
  for (const k of Object.keys(scanState)) {
    try {
      await docsTaskCancel(Number(k));
    } catch {
      // 忽略
    }
  }
}

/* ---------- 事件监听 ---------- */
let unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  await syncAppearance();
  await Promise.all([loadDirs(), loadExts(), refresh()]);
  unlisteners.push(
    await listen<DocsScanProgress>("docs://scan-progress", (e) => {
      const p = e.payload;
      if (p.done) {
        delete scanState[p.dirId];
        if (p.removed > 0 || p.added > 0 || p.updated > 0) {
          void refresh();
        }
        void loadDirs();
        void loadExts();
        // 无新增且无清理时不提示,避免进入页面自动重扫每次都弹结果
        if (p.added > 0 || p.removed > 0) {
          ElMessage.success(
            t("docs.scanDone", { n: p.scanned, a: p.added, u: p.updated, r: p.removed }),
          );
        }
      } else {
        scanState[p.dirId] = { scanned: p.scanned };
      }
    }),
  );
  window.addEventListener("focus", syncAppearance);

  // 进入时对所有已配置目录自动触发一轮增量扫描(静默,已在扫描中的会被忽略)
  for (const d of dirs.value) {
    void docsScanStart(d.id).catch(() => {});
  }
});

onBeforeUnmount(() => {
  unlisteners.forEach((u) => u());
  window.removeEventListener("focus", syncAppearance);
});

/* ---------- 展示工具 ---------- */
function fmtBytes(n: number): string {
  if (n < 1024) {
    return `${n} B`;
  }
  if (n < 1024 * 1024) {
    return `${(n / 1024).toFixed(1)} KB`;
  }
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function fmtDate(ts: number): string {
  if (!ts) {
    return "-";
  }
  return new Intl.DateTimeFormat(locale.value === "en" ? "en-US" : "zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ts * 1000));
}

/* ---------- 转换 / 打开 / 删除 ---------- */
const convertingKey = ref("");
const convertResult = reactive({
  visible: false,
  name: "",
  markdown: "",
  chars: 0,
  lines: 0,
});

async function convertRow(row: DocItemInfo) {
  convertingKey.value = row.path;
  try {
    const r = await documentConvert(row.path);
    if (r.needs_ocr) {
      ElMessage.warning(t("docs.needsOcr", { total: r.page_count }));
      return;
    }
    convertResult.name = row.name;
    convertResult.markdown = r.markdown;
    convertResult.chars = r.markdown.length;
    convertResult.lines = r.markdown ? r.markdown.split("\n").length : 0;
    convertResult.visible = true;
  } catch (e) {
    ElMessage.error(t("docs.convertFail", { err: String(e) }));
  } finally {
    convertingKey.value = "";
  }
}

async function copyMarkdown() {
  try {
    await copyToClipboard(convertResult.markdown);
    ElMessage.success(t("docs.copied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function downloadMarkdown() {
  try {
    const base = convertResult.name.replace(/\.[^.]*$/, "");
    const picked = await saveDialog({ defaultPath: `${base || "document"}.md` });
    if (!picked) {
      return;
    }
    await saveDocumentMarkdown(picked, convertResult.markdown);
    ElMessage.success(t("docs.saved", { path: picked }));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function deleteRow(row: DocItemInfo) {
  try {
    await ElMessageBox.confirm(row.name, t("docs.deleteConfirm"), { type: "warning" });
  } catch {
    return;
  }
  try {
    await docsDeleteItem(row.id);
    items.value = items.value.filter((i) => i.id !== row.id);
    await loadDirs();
    await loadExts();
    ElMessage.success(t("docs.deleteDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}
</script>

<template>
  <div class="w-screen h-screen bg-[var(--el-bg-color)] text-dp-primary overflow-hidden select-none">
    <!-- 顶栏(拖拽区) -->
    <div
      class="h-12 shrink-0 flex items-center gap-3 px-4 bg-dp-card-bg border-b border-dp-divider"
      data-tauri-drag-region
    >
      <span class="text-sm font-semibold text-dp-primary" data-tauri-drag-region>
        {{ t("docs.title") }}
      </span>
      <el-input
        v-model="searchInput"
        size="small"
        clearable
        :prefix-icon="Search"
        :placeholder="t('docs.searchPh')"
        class="w-56 ml-auto"
      />
      <span
        v-if="scanChip.active"
        class="flex items-center gap-1.5 text-[11px] text-dp-tertiary whitespace-nowrap"
      >
        <el-icon class="is-loading" :size="12"><Loading /></el-icon>
        <span>{{ t("docs.scanChipIndex", { s: scanChip.scanned }) }}</span>
        <el-icon
          class="cursor-pointer hover:text-dp-muted"
          :size="12"
          :title="t('docs.scanCancel')"
          @click="cancelAllScans"
        >
          <Close />
        </el-icon>
      </span>
      <el-button size="small" :icon="Close" circle text @click="closeWindow" />
    </div>

    <div class="flex h-[calc(100%-3rem)]">
      <!-- 侧边栏 -->
      <aside class="w-52 shrink-0 border-r border-dp-divider bg-dp-card-bg flex flex-col overflow-y-auto">
        <div class="p-2.5">
          <div class="flex items-center gap-1.5">
            <span class="text-[11px] font-medium text-dp-muted flex-1">
              {{ t("docs.libraries") }}
            </span>
            <el-tooltip :content="t('docs.addDir')">
              <el-button size="small" :icon="FolderAdd" circle text @click="pickDir" />
            </el-tooltip>
          </div>
          <div
            v-for="d in dirs"
            :key="d.id"
            class="group flex items-center gap-1 px-1.5 py-1 rounded-md cursor-pointer text-[11px] leading-[1.6] min-w-0"
            :class="dirFilter === d.id ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] text-dp-accent' : 'text-dp-muted hover:bg-dp-btn-bg'"
            @click="dirFilter = dirFilter === d.id ? null : d.id"
          >
            <el-icon
              v-if="scanState[d.id]"
              class="is-loading shrink-0"
              :size="11"
              :title="t('docs.scanning')"
              @click.stop="docsTaskCancel(d.id)"
            >
              <Loading />
            </el-icon>
            <span class="truncate flex-1 min-w-0" :title="d.path">{{ d.path }}</span>
            <span class="shrink-0 text-[10px] text-dp-tertiary">{{ d.item_count }}</span>
            <span class="hidden group-hover:flex items-center shrink-0">
              <el-icon :size="11" class="hover:text-dp-accent" :title="t('docs.rescan')" @click.stop="rescan(d.id)">
                <Refresh />
              </el-icon>
              <el-icon :size="11" class="hover:text-[var(--el-color-danger,#f56c6c)]" :title="t('docs.removeDir')" @click.stop="removeDir(d)">
                <Close />
              </el-icon>
            </span>
          </div>
        </div>
        <div v-if="exts.length" class="p-2.5 border-t border-dp-divider">
          <div class="text-[11px] font-medium text-dp-muted mb-1">
            {{ t("docs.types") }}
          </div>
          <div
            v-for="e in exts"
            :key="e.ext"
            class="flex items-center gap-1 px-1.5 py-1 rounded-md cursor-pointer text-[11px] leading-[1.6]"
            :class="extFilter === e.ext ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] text-dp-accent' : 'text-dp-muted hover:bg-dp-btn-bg'"
            @click="extFilter = extFilter === e.ext ? null : e.ext"
          >
            <span class="uppercase flex-1 min-w-0 truncate">{{ e.ext }}</span>
            <span class="shrink-0 text-[10px] text-dp-tertiary">{{ e.count }}</span>
          </div>
        </div>
        <div class="p-2.5 border-t border-dp-divider text-[10px] leading-[1.6] text-dp-tertiary">
          {{ t("docs.sidebarHint") }}
        </div>
      </aside>

      <!-- 主区:文档表格(始终挂载,避免刷新数据时卸载导致排序箭头丢失) -->
      <main class="flex-1 min-w-0 overflow-y-auto view-scroll" @scroll="onMainScroll">
        <div class="px-3 pt-3">
          <el-table
            ref="tableRef"
            :data="items"
            size="small"
            class="w-full"
            :default-sort="{ prop: 'mtime', order: 'descending' }"
            @sort-change="onSortChange"
          >
            <el-table-column prop="name" :label="t('docs.docName')" min-width="260" show-overflow-tooltip sortable="custom" />
            <el-table-column prop="ext" :label="t('docs.docType')" width="90" sortable="custom">
              <template #default="{ row }">
                <span class="text-[10.5px] px-1.5 py-0.5 rounded bg-dp-btn-bg text-dp-muted uppercase">
                  {{ row.ext }}
                </span>
              </template>
            </el-table-column>
            <el-table-column prop="size" :label="t('docs.docSize')" width="104" sortable="custom">
              <template #default="{ row }">{{ fmtBytes(row.size) }}</template>
            </el-table-column>
            <el-table-column prop="mtime" :label="t('docs.docDate')" width="150" sortable="custom">
              <template #default="{ row }">{{ fmtDate(row.mtime) }}</template>
            </el-table-column>
            <el-table-column :label="t('docs.docActions')" width="186" fixed="right">
              <template #default="{ row }">
                <!-- 去掉 EP 相邻按钮默认边距并压缩内边距,避免固定列溢出裁切 -->
                <div class="flex items-center gap-1 [&_.el-button+.el-button]:ml-0 [&_.el-button]:!px-1.5">
                  <el-button
                    v-if="row.ext !== 'md'"
                    size="small"
                    text
                    type="primary"
                    :loading="convertingKey === row.path"
                    @click="convertRow(row)"
                  >
                    {{ t("docs.convertMd") }}
                  </el-button>
                  <el-tooltip :content="t('docs.open')" placement="top">
                    <el-button size="small" text :icon="FolderOpened" @click="imagesOpenPath(row.path)" />
                  </el-tooltip>
                  <el-tooltip :content="t('docs.locate')" placement="top">
                    <el-button size="small" text :icon="Aim" @click="imagesRevealPath(row.path)" />
                  </el-tooltip>
                  <el-tooltip :content="t('docs.del')" placement="top">
                    <el-button size="small" text type="danger" :icon="Delete" @click="deleteRow(row)" />
                  </el-tooltip>
                </div>
              </template>
            </el-table-column>
            <template #empty>
              <el-empty v-if="!loading" :description="t('docs.emptyHint')" :image-size="72">
                <el-button type="primary" size="small" :icon="FolderAdd" @click="pickDir">
                  {{ t("docs.addDir") }}
                </el-button>
              </el-empty>
            </template>
          </el-table>
        </div>
        <div v-if="loading" class="py-6 flex justify-center text-dp-tertiary">
          <el-icon class="is-loading" :size="18"><Loading /></el-icon>
        </div>
      </main>
    </div>

    <!-- 转换结果 -->
    <el-dialog
      v-model="convertResult.visible"
      :title="convertResult.name"
      width="680px"
      :close-on-click-modal="true"
    >
      <div class="text-[11px] text-dp-tertiary mb-2">
        {{ t("docs.convertStats", { chars: convertResult.chars, lines: convertResult.lines }) }}
      </div>
      <el-scrollbar max-height="420px" class="dp-scroll">
        <pre class="m-0 p-3 rounded-lg bg-dp-panel-bg border border-dp-divider text-[12px] leading-[1.6] whitespace-pre-wrap break-words text-dp-primary">{{ convertResult.markdown }}</pre>
      </el-scrollbar>
      <template #footer>
        <el-button size="small" :icon="CopyDocument" @click="copyMarkdown">
          {{ t("docs.copy") }}
        </el-button>
        <el-button size="small" type="primary" :icon="Download" @click="downloadMarkdown">
          {{ t("docs.download") }}
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
