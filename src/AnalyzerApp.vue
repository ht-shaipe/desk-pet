<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { Close, Loading } from "@element-plus/icons-vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { homeDir } from "@tauri-apps/api/path";
import {
  cancelDiskCleanup,
  getDiskUsage,
} from "@/api/cleanup";
import {
  closeDiskAnalyze,
  deleteDiskAnalyze,
  insightsDiskAnalyze,
  layoutDiskAnalyze,
  marksDiskAnalyze,
  planDiskAnalyze,
  scanDiskAnalyze,
} from "@/api/disk";
import { getSettings } from "@/api/settings";
import DiskTreemap from "@/components/disk/DiskTreemap.vue";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import type {
  AnalyzerInsight,
  CleanupEvent,
  CleanupRemovalMode,
  DiskLayout,
  DiskMarkInfo,
  DiskMetric,
  DiskUsage,
  RemovalPlan,
} from "@/types";

const { t } = useI18n();

type Phase = "idle" | "scanning" | "ready" | "deleting";
type ReviewState = "closed" | "loading" | "open";

const DEPTHS = [1, 2, 3, 4];

const phase = ref<Phase>("idle");
const rootPath = ref("");
const scanId = ref("");
const viewPath = ref("");
const layout = ref<DiskLayout | null>(null);
const metric = ref<DiskMetric>("bytes");
const maxDepth = ref(2);
const filterText = ref("");
const selectedPath = ref("");

// 标记（绝对路径集合）+ 当前树中的大小
const markPaths = ref(new Set<string>());
const markInfos = ref<DiskMarkInfo[]>([]);

// 值得看看：树上顶层可回收大目录（扫描完成后刷新）
const insights = ref<AnalyzerInsight[]>([]);

let insightsSeq = 0;
async function refreshInsights() {
  const scan = scanId.value;
  const seq = ++insightsSeq;
  if (!scan) {
    insights.value = [];
    return;
  }
  try {
    const items = await insightsDiskAnalyze(scan);
    if (seq === insightsSeq && scanId.value === scan) {
      insights.value = items;
    }
  } catch {
    if (seq === insightsSeq) {
      insights.value = [];
    }
  }
}

const stats = reactive({
  dirs: 0,
  files: 0,
  bytes: 0,
  truncated: false,
  complete: true,
  durationMs: 0,
});
const progress = reactive({ dirs: 0, files: 0, bytes: 0, current: "" });
const removal = reactive({
  removed: 0,
  failed: 0,
  bytes: 0,
  freedBytes: 0,
  failures: [] as { path: string; error: string }[],
});

const removalMode = ref<CleanupRemovalMode>("trash");
const review = ref<ReviewState>("closed");
const reviewPlan = ref<RemovalPlan | null>(null);

const disk = ref<DiskUsage | null>(null);
const stream = ref<EventSource | null>(null);
const activeJobId = ref("");
const treemapRef = ref<InstanceType<typeof DiskTreemap> | null>(null);
let layoutSeq = 0;
let reviewSeq = 0;

function invalidateReview() {
  reviewSeq += 1;
  review.value = "closed";
  reviewPlan.value = null;
}

const busy = computed(() => phase.value === "scanning" || phase.value === "deleting");

const crumbs = computed(() => {
  if (!viewPath.value) {
    return [] as { name: string; path: string }[];
  }
  const segments = viewPath.value.split("/").filter(Boolean);
  const items: { name: string; path: string }[] = [];
  let acc = "";
  for (const segment of segments) {
    acc += `/${segment}`;
    items.push({ name: segment, path: acc });
  }
  return items;
});

const selectedInfo = computed(() => {
  const path = selectedPath.value;
  if (!path) {
    return null;
  }
  const node = markInfos.value.find((item) => item.path === path);
  const tile = layout.value?.tiles.find((item) => item.path === path);
  const bytes = node?.bytes ?? tile?.bytes ?? 0;
  const files = node?.files ?? tile?.files ?? 0;
  const total = layout.value?.totalBytes ?? 1;
  return {
    path,
    name: path.split("/").filter(Boolean).pop() ?? path,
    bytes,
    files,
    percent: total > 0 ? (bytes / total) * 100 : 0,
    reclaim: tile?.reclaim ?? null,
    category: tile?.category ?? null,
    isMarked: markPaths.value.has(path),
    drillable: tile?.hasChildren ?? false,
  };
});

// 标记可能重叠（父目录与子目录同时标记），合计时只计最外层，避免重复。
const markTotal = computed(() => {
  const infos = markInfos.value
    .filter((item) => item.exists && markPaths.value.has(item.path))
    .sort((a, b) => a.path.length - b.path.length);
  const roots: string[] = [];
  let total = 0;
  for (const info of infos) {
    if (roots.some((root) => info.path === root || info.path.startsWith(`${root}/`))) {
      continue;
    }
    roots.push(info.path);
    total += info.bytes;
  }
  return total;
});

const freeAfter = computed(() => (disk.value?.available ?? 0) + markTotal.value);

let usageSeq = 0;
async function refreshDiskUsage() {
  const root = rootPath.value;
  const seq = ++usageSeq;
  try {
    const value = await getDiskUsage(root);
    if (seq === usageSeq && rootPath.value === root) {
      disk.value = value;
    }
  } catch {
    if (seq === usageSeq) {
      disk.value = null;
    }
  }
}

// 标记刷新的序号：并发在途时只应用最新响应，避免旧响应覆盖新标记
let marksSeq = 0;

async function refreshMarks() {
  const scan = scanId.value;
  const seq = ++marksSeq;
  if (!scan || markPaths.value.size === 0) {
    markInfos.value = [];
    return;
  }
  const requested = [...markPaths.value];
  try {
    const infos = await marksDiskAnalyze(scan, requested);
    if (seq !== marksSeq || scanId.value !== scan) {
      return; // 已有更新的请求/扫描在途，丢弃过期响应
    }
    markInfos.value = infos;
    const alive = new Set(infos.filter((item) => item.exists).map((item) => item.path));
    // 合并语义：剔除已消失的标记，保留刷新期间本地新增/移除的变化
    markPaths.value = new Set(
      [...markPaths.value].filter((path) => !requested.includes(path) || alive.has(path)),
    );
  } catch (e) {
    if (seq === marksSeq) {
      // 树失效等场景：保留本地标记但明确提示（重扫后恢复）
      ElMessage.error(String(e));
    }
  }
}

async function refreshLayout() {
  if (!scanId.value || !viewPath.value) {
    return;
  }
  const seq = ++layoutSeq;
  try {
    const result = await layoutDiskAnalyze({
      scanId: scanId.value,
      path: viewPath.value,
      width: canvasWidth.value,
      height: canvasHeight.value,
      maxDepth: maxDepth.value,
      metric: metric.value,
      filter: filterText.value || undefined,
    });
    if (seq === layoutSeq) {
      layout.value = result;
    }
  } catch (e) {
    if (seq === layoutSeq) {
      layout.value = null;
      ElMessage.error(String(e));
    }
  }
}

// 画布尺寸：treemap 布局需要具体像素
const canvasWidth = ref(800);
const canvasHeight = ref(500);
const canvasWrap = ref<HTMLDivElement | null>(null);
let resizeObserver: ResizeObserver | null = null;

function observeCanvas() {
  const el = canvasWrap.value;
  if (!el || typeof ResizeObserver === "undefined") {
    return;
  }
  resizeObserver = new ResizeObserver(() => {
    canvasWidth.value = Math.max(160, el.clientWidth);
    canvasHeight.value = Math.max(160, el.clientHeight);
    void refreshLayout();
  });
  resizeObserver.observe(el);
}

function closeStream() {
  stream.value?.close();
  stream.value = null;
}

function resetResults() {
  layoutSeq += 1;
  marksSeq += 1;
  insightsSeq += 1;
  usageSeq += 1;
  reviewSeq += 1;
  layout.value = null;
  selectedPath.value = "";
  scanId.value = "";
  activeJobId.value = "";
  insights.value = [];
  disk.value = null;
  markPaths.value = new Set();
  markInfos.value = [];
  review.value = "closed";
  reviewPlan.value = null;
  removalMode.value = "trash";
  Object.assign(stats, {
    dirs: 0,
    files: 0,
    bytes: 0,
    truncated: false,
    complete: true,
    durationMs: 0,
  });
  Object.assign(progress, { dirs: 0, files: 0, bytes: 0, current: "" });
  Object.assign(removal, { removed: 0, failed: 0, bytes: 0, freedBytes: 0, failures: [] });
}

function connect(jobId: string, url: string) {
  closeStream();
  activeJobId.value = jobId;
  const source = new EventSource(url);
  stream.value = source;
  source.onmessage = (event) => {
    let data: CleanupEvent;
    try {
      data = JSON.parse(event.data) as CleanupEvent;
    } catch {
      return;
    }
    handleEvent(data);
  };
  source.onerror = () => {
    if (phase.value === "scanning") {
      closeStream();
      // 连接断开不代表后端任务已停止；显式取消，避免后台扫描继续占用资源。
      const jobId = activeJobId.value;
      if (jobId) {
        void cancelDiskCleanup(jobId).catch(() => {});
      }
      invalidateReview();
      phase.value = "idle";
      ElMessage.error(t("toolbox.tools.cleanup.connectFail"));
    } else if (phase.value === "deleting") {
      closeStream();
      // 关键：删除连接断开时必须停止后端删除。否则窗口以为已经结束，
      // 文件却仍在后台被删除；用户可能继续操作并造成状态错乱。
      const jobId = activeJobId.value;
      if (jobId) {
        void cancelDiskCleanup(jobId).catch(() => {});
      }
      invalidateReview();
      phase.value = "ready";
      ElMessage.error(t("toolbox.tools.cleanup.connectFail"));
    }
  };
}

function handleEvent(data: CleanupEvent) {
  switch (data.type) {
    case "started":
      progress.current = data.root;
      break;
    case "progress":
      progress.dirs = data.dirs;
      progress.files = data.files;
      progress.bytes = data.bytes;
      progress.current = data.current;
      break;
    case "scanDone":
      Object.assign(stats, {
        dirs: data.count,
        files: data.files,
        bytes: data.bytes,
        truncated: data.truncated,
        complete: data.complete,
        durationMs: data.durationMs,
      });
      scanId.value = activeJobId.value;
      phase.value = "ready";
      closeStream();
      viewPath.value = rootPath.value;
      void refreshLayout();
      void refreshMarks();
      void refreshInsights();
      void refreshDiskUsage();
      break;
    case "deleting":
      progress.current = data.path;
      break;
    case "deleted":
      if (data.ok) {
        removal.removed += 1;
        removal.bytes += data.bytes;
      } else {
        removal.failed += 1;
      }
      break;
    case "deleteDone":
      removal.removed = data.removed;
      removal.failed = data.failed;
      removal.bytes = data.bytes;
      removal.freedBytes = data.freedBytes;
      removal.failures = data.failures;
      phase.value = "ready";
      invalidateReview();
      closeStream();
      if (data.failed > 0) {
        ElMessage.warning(
          t("toolbox.tools.cleanup.deletePartial", {
            count: data.removed,
            failed: data.failed,
            size: fmt(data.bytes),
          }),
        );
      } else {
        ElMessage.success(
          t("toolbox.tools.cleanup.freedActual", { size: fmt(data.freedBytes || data.bytes) }),
        );
      }
      if (
        removalMode.value === "permanent" &&
        data.freedBytes > 0 &&
        data.freedBytes < data.bytes * 0.9
      ) {
        ElMessage.info(t("toolbox.tools.cleanup.freedExplain"));
      }
      // 删除后自动重扫，树与免费空间同步刷新
      void startScan();
      break;
    case "failed":
      closeStream();
      invalidateReview();
      phase.value = data.mode === "scan" ? "idle" : "ready";
      ElMessage.error(data.error);
      break;
    case "cancelled":
      closeStream();
      invalidateReview();
      if (data.mode === "scan") {
        phase.value = "idle";
      } else {
        phase.value = "ready";
      }
      ElMessage.info(t("toolbox.tools.cleanup.cancelled"));
      break;
  }
}

async function startScan() {
  if (!rootPath.value) {
    return;
  }
  closeStream();
  // 重扫保留标记（与提示文案一致）：先暂存，resetResults 清空后恢复；
  // 新树出来后 refreshMarks 会剔除已消失的路径。
  const keptMarks = markPaths.value;
  resetResults();
  markPaths.value = new Set(keptMarks);
  phase.value = "scanning";
  try {
    const job = await scanDiskAnalyze(rootPath.value);
    activeJobId.value = job.jobId;
    connect(job.jobId, job.url);
  } catch (e) {
    phase.value = "idle";
    ElMessage.error(String(e));
  }
}

function onSelect(path: string) {
  selectedPath.value = path;
  // 复核打开时切换选择会失效，直接收起
  if (review.value !== "closed") {
    invalidateReview();
  }
}

function onDrill(path: string) {
  const scanRoot = layout.value?.root || rootPath.value;
  const displayRoot = rootPath.value;
  const inScanRoot =
    path === scanRoot ||
    path.startsWith(`${scanRoot}/`) ||
    path === displayRoot ||
    path.startsWith(`${displayRoot}/`);
  if (!path || busy.value || !inScanRoot) {
    return;
  }
  selectedPath.value = "";
  viewPath.value = path;
  treemapRef.value?.resetView();
  void refreshLayout();
}

function drillUp() {
  const scanRoot = layout.value?.root || rootPath.value;
  const parent = viewPath.value.split("/").slice(0, -1).join("/");
  const parentInScanRoot =
    !!parent && (parent === scanRoot || parent.startsWith(`${scanRoot}/`));
  if (parentInScanRoot) {
    viewPath.value = parent;
    selectedPath.value = "";
    treemapRef.value?.resetView();
    void refreshLayout();
  } else if (viewPath.value !== rootPath.value) {
    viewPath.value = rootPath.value;
    selectedPath.value = "";
    treemapRef.value?.resetView();
    void refreshLayout();
  }
}

/** 一键标记所有已知的"安全"级可回收目录：当前视图安全色块 ∪ "值得看看"大项。
 *  Risky/Caution 永远不会自动选中；目录标记吸收内部内容。 */
function markSafeInView() {
  if (busy.value) {
    return;
  }
  const safeTiles = (layout.value?.tiles ?? [])
    .filter((tile) => tile.kind === "dir" && tile.reclaim && tile.risk === "safe")
    .map((tile) => tile.path);
  // 只自动标记 Safe 项；Caution/Risky 即使带 reclaim 原因也不能进入推荐。
  const safeInsights = insights.value
    .filter((item) => item.risk === "safe")
    .map((item) => item.path);
  const merged = new Set([...markPaths.value, ...safeTiles, ...safeInsights]);
  if (merged.size === markPaths.value.size) {
    ElMessage.info(t("analyzer.noSafeItems"));
    return;
  }
  invalidateReview();
  markPaths.value = merged;
  void refreshMarks();
}

function toggleMark(path: string) {
  if (busy.value) {
    return;
  }
  // "文件"色块的 path 即所在目录：直接标记会连带整个目录，先提示改标目录本身。
  const tile = layout.value?.tiles.find((item) => item.path === path);
  if (tile?.kind === "files") {
    ElMessage.info(t("analyzer.markFilesTileHint"));
    return;
  }
  invalidateReview();
  const next = new Set(markPaths.value);
  if (next.has(path)) {
    next.delete(path);
  } else {
    next.add(path);
  }
  markPaths.value = next;
  void refreshMarks();
}

async function openReview() {
  if (markPaths.value.size === 0) {
    return;
  }
  const seq = ++reviewSeq;
  const scan = scanId.value;
  const requested = [...markPaths.value];
  review.value = "loading";
  try {
    const plan = await planDiskAnalyze(scan, requested);
    const selectionUnchanged =
      markPaths.value.size === requested.length &&
      requested.every((path) => markPaths.value.has(path));
    if (seq !== reviewSeq || scanId.value !== scan || !selectionUnchanged) {
      if (seq === reviewSeq) {
        review.value = "closed";
        reviewPlan.value = null;
      }
      return;
    }
    reviewPlan.value = plan;
    review.value = "open";
  } catch (e) {
    if (seq === reviewSeq) {
      review.value = "closed";
      reviewPlan.value = null;
      ElMessage.error(String(e));
    }
  }
}

async function confirmReview() {
  const currentPlan = reviewPlan.value;
  if (!currentPlan || currentPlan.targets.length === 0) {
    return;
  }
  const paths = currentPlan.targets.map((target) => target.path);
  const permanent = removalMode.value === "permanent";
  try {
    await ElMessageBox.confirm(
      permanent
        ? t("analyzer.review.confirmPermanent", { count: paths.length, size: fmt(currentPlan.reclaim) })
        : t("analyzer.review.confirmTrash", { count: paths.length, size: fmt(currentPlan.reclaim) }),
      t("toolbox.tools.cleanup.confirmTitle"),
      {
        type: permanent ? "warning" : "info",
        confirmButtonText: permanent
          ? t("analyzer.deletePermanently")
          : t("analyzer.moveToTrash"),
        cancelButtonText: t("common.cancel"),
      },
    );
  } catch {
    return;
  }

  // 确认删除后先关闭复核视图，避免删除进行中用户还能操作旧清单。
  invalidateReview();
  phase.value = "deleting";
  Object.assign(removal, { removed: 0, failed: 0, bytes: 0, freedBytes: 0, failures: [] });
  try {
    const job = await deleteDiskAnalyze(scanId.value, paths, removalMode.value);
    connect(job.jobId, job.url);
  } catch (e) {
    phase.value = "ready";
    ElMessage.error(String(e));
  }
}

async function stop() {
  const jobId = activeJobId.value;
  closeStream();
  if (jobId) {
    try {
      await cancelDiskCleanup(jobId);
    } catch {
      // 任务可能已结束
    }
  }
  invalidateReview();
  if (phase.value === "scanning") {
    phase.value = "idle";
  } else if (phase.value === "deleting") {
    phase.value = "ready";
  }
  ElMessage.info(t("toolbox.tools.cleanup.cancelled"));
}

async function closeWindow() {
  const jobId = activeJobId.value;
  const running = busy.value;
  if (jobId && running) {
    void cancelDiskCleanup(jobId).catch(() => {});
  }
  if (scanId.value) {
    void closeDiskAnalyze(scanId.value).catch(() => {});
  }
  // 无边框窗口没有系统关闭按钮；window.close() 对非脚本打开的窗口无效，
  // 必须走 Tauri 的 close 命令
  try {
    await getCurrentWindow().close();
  } catch {
    // 窗口可能已被销毁
  }
}

function onKeydown(event: KeyboardEvent) {
  // Element Plus 弹层（确认框/对话框）打开时禁用全部快捷键，避免对底层视图误操作
  if (document.querySelector(".el-overlay")) {
    return;
  }
  const target = event.target as HTMLElement | null;
  if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) {
    if (event.key === "Escape") {
      (target as HTMLInputElement).blur();
    }
    return;
  }
  // 焦点在按钮上时保留 Enter/Space 的原生激活行为，避免与快捷键双触发
  if (target?.tagName === "BUTTON" && (event.key === "Enter" || event.key === " ")) {
    return;
  }
  switch (event.key) {
    case "Backspace":
      event.preventDefault();
      drillUp();
      break;
    case "Enter":
      if (selectedInfo.value?.drillable) {
        onDrill(selectedInfo.value.path);
      }
      break;
    case " ":
    case "x":
    case "X":
      if (selectedPath.value) {
        event.preventDefault();
        toggleMark(selectedPath.value);
      }
      break;
    case "Escape":
      if (review.value !== "closed") {
        invalidateReview();
      } else {
        drillUp();
      }
      break;
    case "/":
      event.preventDefault();
      document.getElementById("analyzer-filter")?.focus();
      break;
    case "t":
    case "T":
      if (!busy.value) {
        metric.value = metric.value === "bytes" ? "files" : "bytes";
        void refreshLayout();
      }
      break;
    case "r":
    case "R":
      if (!busy.value && rootPath.value) {
        void startScan();
      }
      break;
    case "c":
    case "C":
      if (markPaths.value.size > 0 && !busy.value) {
        void openReview();
      }
      break;
    case "+":
    case "=":
      treemapRef.value?.zoomIn();
      break;
    case "-":
    case "_":
      treemapRef.value?.zoomOut();
      break;
    case "0":
      treemapRef.value?.resetView();
      break;
    case "w":
      if (event.metaKey) {
        event.preventDefault();
        void closeWindow();
      }
      break;
  }
}

/** 无边框窗口：顶栏拖拽移动，边缘/四角手柄驱动缩放（最小 720×520）。 */
const ANALYZER_MIN = { width: 720, height: 520 };

function startResize(direction: ResizeDirection, event: PointerEvent) {
  startWindowResize(direction, event, ANALYZER_MIN);
}

function fmt(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

/** 分析窗口不初始化宠物/剪贴板等逻辑，仅同步主题与主题色（与主窗口同一套规则）。 */
async function syncTheme() {
  try {
    const settings = await getSettings();
    applyTheme(settings.theme);
    applyAccentColor(settings.accent_color);
  } catch {
    // 主题读取失败时保持默认
  }
}

onMounted(async () => {
  void syncTheme();
  // 用户在主窗口改主题后切回分析窗口时重新同步（auto 跟随系统已由 theme 模块监听）
  window.addEventListener("focus", syncTheme);
  observeCanvas();
  window.addEventListener("keydown", onKeydown);
  try {
    rootPath.value = await homeDir();
  } catch {
    rootPath.value = "/";
  }
  await startScan();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("focus", syncTheme);
  const jobId = activeJobId.value;
  const running = busy.value;
  closeStream();
  resizeObserver?.disconnect();
  if (jobId && running) {
    void cancelDiskCleanup(jobId).catch(() => {});
  }
  if (scanId.value) {
    void closeDiskAnalyze(scanId.value).catch(() => {});
  }
});
</script>

<template>
  <div class="flex h-screen min-h-0 flex-col bg-[var(--dp-panel-bg)] text-dp-secondary">
    <!-- 顶栏：面包屑 + 控制 -->
    <div
      class="flex shrink-0 items-center gap-2 border-b border-dp-divider px-3 py-2"
      data-tauri-drag-region
    >
      <div
        class="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden"
        data-tauri-drag-region
      >
        <span class="shrink-0 text-[13px] font-bold tracking-[1px] text-dp-primary">
          {{ t("analyzer.title") }}
        </span>
        <span class="mx-1.5 shrink-0 text-dp-tertiary">·</span>
        <template v-for="(crumb, index) in crumbs" :key="crumb.path">
          <button
            type="button"
            class="max-w-[160px] cursor-pointer truncate rounded px-1 py-0.5 text-[11.5px] hover:bg-dp-hover-bg"
            :class="index === crumbs.length - 1 ? 'font-semibold text-dp-primary' : 'text-dp-muted'"
            :title="crumb.path"
            @click="onDrill(crumb.path)"
          >
            {{ crumb.name }}
          </button>
          <span v-if="index < crumbs.length - 1" class="shrink-0 text-dp-tertiary">/</span>
        </template>
      </div>
      <el-radio-group v-model="metric" size="small" :disabled="busy" @change="refreshLayout">
        <el-radio-button value="bytes">{{ t("analyzer.metricSize") }}</el-radio-button>
        <el-radio-button value="files">{{ t("analyzer.metricFiles") }}</el-radio-button>
      </el-radio-group>
      <el-select
        v-model="maxDepth"
        size="small"
        class="w-[92px]"
        :disabled="busy"
        @change="refreshLayout"
      >
        <el-option v-for="depth in DEPTHS" :key="depth" :value="depth" :label="`${t('analyzer.depth')} ${depth}`" />
      </el-select>
      <el-input
        id="analyzer-filter"
        v-model="filterText"
        size="small"
        class="w-44"
        clearable
        :placeholder="t('analyzer.filterPlaceholder')"
        :disabled="busy"
        @input="refreshLayout"
      />
      <el-button size="small" :disabled="busy || !rootPath" @click="startScan">
        {{ t("analyzer.rescan") }}
      </el-button>
      <el-button size="small" text @click="closeWindow">{{ t("analyzer.close") }}</el-button>
    </div>

    <div class="flex min-h-0 flex-1">
      <!-- treemap 画布 -->
      <div class="relative min-w-0 flex-1">
        <div ref="canvasWrap" class="absolute inset-0">
          <DiskTreemap
            ref="treemapRef"
            :tiles="layout?.tiles ?? []"
            :selected-path="selectedPath"
            :files-label="t('analyzer.filesTile')"
            :others-label="(count: number) => t('analyzer.othersTile', { n: count })"
            :size-label="(bytes: number) => fmt(bytes)"
            @select="onSelect"
            @drill="onDrill"
          />
        </div>
        <!-- 扫描中遮罩 -->
        <div
          v-if="phase === 'scanning'"
          class="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-[color-mix(in_srgb,var(--dp-panel-bg)_72%,transparent)]"
        >
          <el-icon class="is-loading text-dp-accent" :size="22"><Loading /></el-icon>
          <div class="text-[12px] text-dp-secondary">{{ t("analyzer.scanning") }}</div>
          <div class="font-[ui-monospace,Menlo,monospace] text-[10.5px] text-dp-tertiary">
            {{ progress.dirs }} {{ t("analyzer.dirs") }} · {{ progress.files }}
            {{ t("analyzer.files") }} · {{ fmt(progress.bytes) }}
          </div>
          <div class="max-w-[60%] truncate text-[10px] text-dp-tertiary" :title="progress.current">
            {{ progress.current }}
          </div>
          <el-button size="small" plain type="danger" @click="stop">
            {{ t("toolbox.tools.cleanup.stop") }}
          </el-button>
        </div>
        <!-- 底部状态条 -->
        <div
          v-if="phase === 'ready'"
          class="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-dp-divider bg-[color-mix(in_srgb,var(--dp-panel-bg)_88%,transparent)] px-3 py-1 text-[10.5px] text-dp-muted"
        >
          <span>{{ t("toolbox.tools.cleanup.found") }} {{ stats.dirs }} {{ t("analyzer.dirs") }}</span>
          <span>{{ fmt(stats.bytes) }}</span>
          <span v-if="disk">
            {{ t("toolbox.tools.cleanup.diskUsed", { used: fmt(disk.total - disk.available), total: fmt(disk.total) }) }}
          </span>
          <span v-if="stats.truncated" class="text-[#e6a23c]">{{ t("analyzer.truncated") }}</span>
          <span v-if="!stats.complete" class="text-[#e6a23c]">
            {{ t("toolbox.tools.cleanup.incomplete") }}
          </span>
          <span class="ml-auto text-dp-tertiary">{{ t("analyzer.shortcutHint") }}</span>
        </div>
      </div>

      <!-- 侧栏 -->
      <div class="flex w-[264px] shrink-0 flex-col border-l border-dp-divider">
        <el-scrollbar class="min-h-0 flex-1">
          <div class="p-3">
            <div class="mb-1 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("analyzer.selection") }}
            </div>
            <template v-if="selectedInfo">
              <div class="truncate text-[12px] font-semibold text-dp-primary" :title="selectedInfo.path">
                {{ selectedInfo.name }}
              </div>
              <div class="mt-0.5 truncate text-[10px] text-dp-tertiary" :title="selectedInfo.path">
                {{ selectedInfo.path }}
              </div>
              <div class="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5 text-[11px] text-dp-secondary">
                <span class="font-semibold">{{ fmt(selectedInfo.bytes) }}</span>
                <span>{{ selectedInfo.percent.toFixed(1) }}%</span>
                <span>{{ selectedInfo.files }} {{ t("analyzer.files") }}</span>
              </div>
              <div
                v-if="selectedInfo.category"
                class="mt-1 text-[10px] text-dp-tertiary"
              >
                {{ t(`disk.category.${selectedInfo.category}`) }}
                <template v-if="selectedInfo.reclaim">
                  · {{ t(`disk.reclaim.${selectedInfo.reclaim}`) }}
                </template>
              </div>
              <div class="mt-2 flex gap-2">
                <el-button size="small" type="primary" plain class="flex-1" :disabled="busy" @click="toggleMark(selectedInfo.path)">
                  {{
                    selectedInfo.isMarked
                      ? t("analyzer.unmark")
                      : t("analyzer.mark")
                  }}
                </el-button>
                <el-button
                  size="small"
                  class="flex-1"
                  :disabled="!selectedInfo.drillable"
                  @click="onDrill(selectedInfo.path)"
                >
                  {{ t("analyzer.drill") }}
                </el-button>
              </div>
            </template>
            <div v-else class="py-2 text-[11px] leading-[1.6] text-dp-tertiary">
              {{ t("analyzer.noSelection") }}
            </div>

            <!-- 值得看看：顶层可回收大目录，勾选直接进删除清单；点击行跳转查看 -->
            <template v-if="insights.length > 0">
              <div class="mb-1 mt-4 flex items-center gap-1.5">
                <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
                  {{ t("analyzer.worthALook") }}
                </span>
              </div>
              <div
                v-for="item in insights"
                :key="item.path"
                class="mb-1 flex cursor-pointer items-center gap-1.5 rounded-[8px] border px-2 py-1 transition-[background] hover:bg-dp-hover-bg"
                :class="
                  markPaths.has(item.path)
                    ? 'border-dp-accent bg-[color-mix(in_srgb,var(--dp-accent)_10%,var(--dp-card-bg))]'
                    : 'border-dp-card-border bg-dp-card-bg'
                "
                :title="item.path"
                @click="onDrill(item.path)"
              >
                <el-checkbox
                  class="shrink-0 [&_.el-checkbox\_\_label]:p-0"
                  :model-value="markPaths.has(item.path)"
                  :disabled="busy"
                  @change="toggleMark(item.path)"
                  @click.stop
                />
                <span class="min-w-0 flex-1 truncate text-[10.5px] text-dp-muted">
                  {{ item.name }}
                </span>
                <span class="shrink-0 text-[9.5px] text-dp-tertiary">
                  {{ t(`disk.reclaim.${item.reclaim}`) }}
                </span>
                <span class="shrink-0 text-[10.5px] text-dp-secondary">{{ fmt(item.bytes) }}</span>
              </div>
            </template>

            <!-- 已标记 -->
            <div class="mb-1 mt-4 flex items-center gap-1.5">
              <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
                {{ t("analyzer.marks") }}
              </span>
              <span class="text-[10px] text-dp-tertiary">
                {{ markInfos.filter((item) => item.exists).length }} · {{ fmt(markTotal) }}
              </span>
              <el-button
                size="small"
                text
                type="primary"
                class="ml-auto"
                :disabled="busy || (layout?.tiles.length ?? 0) === 0"
                @click="markSafeInView"
              >
                {{ t("analyzer.markSafe") }}
              </el-button>
            </div>
            <div
              v-if="markInfos.length === 0"
              class="rounded-[8px] border border-dashed border-dp-card-border px-2 py-2 text-[10.5px] leading-[1.5] text-dp-tertiary"
            >
              {{ t("analyzer.markEmpty") }}
            </div>
            <div
              v-for="item in markInfos"
              :key="item.path"
              class="group mb-1 flex items-center gap-1.5 rounded-[8px] border border-dp-card-border bg-dp-card-bg px-2 py-1"
              :class="{ 'opacity-50': !item.exists }"
            >
              <span
                class="min-w-0 flex-1 cursor-pointer truncate text-[10px] text-dp-muted hover:text-dp-primary"
                :title="item.path"
                @click="onDrill(item.path)"
              >
                {{ item.path }}
              </span>
              <span class="shrink-0 text-[10px] text-dp-tertiary">{{ fmt(item.bytes) }}</span>
              <el-icon
                class="shrink-0 cursor-pointer text-dp-tertiary hover:text-dp-primary"
                :size="11"
                @click="toggleMark(item.path)"
              >
                <Close />
              </el-icon>
            </div>

            <!-- 免费空间投影 -->
            <div class="mb-1 mt-4 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("analyzer.freeAfter") }}
            </div>
            <div class="flex items-baseline gap-2">
              <span class="text-[15px] font-bold text-dp-accent">{{ fmt(freeAfter) }}</span>
              <span v-if="disk" class="text-[10px] text-dp-tertiary">
                {{ t("analyzer.freeNow", { size: fmt(disk.available) }) }}
              </span>
            </div>
          </div>
        </el-scrollbar>

        <!-- 复核 -->
        <div class="shrink-0 border-t border-dp-divider p-3">
          <template v-if="review === 'closed'">
            <el-button
              type="primary"
              class="w-full [&_.el-button]:ml-0"
              :disabled="markPaths.size === 0 || busy"
              @click="openReview"
            >
              {{ t("analyzer.reviewBtn") }}
            </el-button>
          </template>
          <template v-else-if="review === 'loading'">
            <div class="flex items-center gap-2 text-[11px] text-dp-muted">
              <el-icon class="is-loading text-dp-accent" :size="12"><Loading /></el-icon>
              {{ t("analyzer.reviewLoading") }}
            </div>
          </template>
          <template v-else-if="reviewPlan">
            <div class="mb-1 text-[10.5px] font-semibold text-dp-secondary">
              {{ t("analyzer.reviewTitle") }} ·
              {{ t("analyzer.reviewCount", { n: reviewPlan.targets.length }) }} ·
              {{ fmt(reviewPlan.reclaim) }}
            </div>
            <div class="max-h-28 overflow-y-auto rounded-[8px] bg-dp-btn-bg px-2 py-1">
              <div
                v-for="target in reviewPlan.targets"
                :key="target.path"
                class="flex items-center gap-1.5"
              >
                <span
                  class="min-w-0 flex-1 truncate font-[ui-monospace,Menlo,monospace] text-[9.5px] leading-[1.5] text-dp-muted"
                  :title="target.path"
                >
                  {{ target.path }}
                </span>
                <span
                  v-if="target.gitRepos"
                  class="shrink-0 text-[9px] text-[#e6a23c]"
                  :title="t('analyzer.review.gitWarn', { n: target.gitRepos })"
                >
                  ⚠ {{ target.gitRepos }} git
                </span>
                <span
                  v-else-if="(target.nonReclaimableBytes ?? 0) > 0"
                  class="shrink-0 text-[9px] text-[#e6a23c]"
                  :title="t('analyzer.review.nonReclaimWarn', { size: fmt(target.nonReclaimableBytes ?? 0) })"
                >
                  ⚠ {{ fmt(target.nonReclaimableBytes ?? 0) }}
                </span>
              </div>
            </div>
            <div v-if="reviewPlan.covered.length > 0" class="mt-1 text-[10px] text-dp-tertiary">
              {{ t("analyzer.review.covered", { n: reviewPlan.covered.length }) }}
            </div>
            <template v-if="reviewPlan.blocked.length > 0">
              <div class="mt-1 text-[10px] font-semibold text-[#e6a23c]">
                {{ t("analyzer.review.blocked", { n: reviewPlan.blocked.length }) }}
              </div>
              <div class="max-h-16 overflow-y-auto">
                <div
                  v-for="blocked in reviewPlan.blocked"
                  :key="blocked.path"
                  class="truncate text-[9.5px] leading-[1.5] text-dp-tertiary"
                  :title="`${blocked.path}: ${blocked.error}`"
                >
                  {{ blocked.path }} — {{ blocked.error }}
                </div>
              </div>
            </template>
            <el-radio-group v-model="removalMode" class="mt-2 flex flex-col gap-0.5">
              <el-radio value="trash" class="mr-0 h-auto py-0.5 [&_.el-radio\_\_label]:pl-2">
                <span class="text-[11px]">{{ t("analyzer.moveToTrash") }} — {{ t("analyzer.review.trashHint") }}</span>
              </el-radio>
              <el-radio value="permanent" class="mr-0 h-auto py-0.5 [&_.el-radio\_\_label]:pl-2">
                <span class="text-[11px]">{{ t("analyzer.deletePermanently") }} — {{ t("analyzer.review.permanentHint") }}</span>
              </el-radio>
            </el-radio-group>
            <div class="mt-2 flex gap-2 [&_.el-button]:ml-0">
              <el-button class="flex-1" size="small" @click="review = 'closed'">
                {{ t("common.cancel") }}
              </el-button>
              <el-button
                :type="removalMode === 'permanent' ? 'danger' : 'primary'"
                class="flex-1"
                size="small"
                :disabled="reviewPlan.targets.length === 0"
                @click="confirmReview"
              >
                {{
                  removalMode === "permanent"
                    ? t("analyzer.review.confirmPermanent", { count: reviewPlan.targets.length, size: fmt(reviewPlan.reclaim) })
                    : t("analyzer.review.confirmTrash", { count: reviewPlan.targets.length, size: fmt(reviewPlan.reclaim) })
                }}
              </el-button>
            </div>
          </template>
        </div>
      </div>
    </div>

    <!-- 无边框窗口的边缘/四角缩放手柄（复用面板同款轮询式 resize） -->
    <div class="pointer-events-none fixed inset-0 z-50" aria-hidden="true">
      <div
        class="rs-hint left-0 right-0 top-0 h-[5px] cursor-ns-resize"
        @pointerdown.prevent="startResize('North', $event)"
      />
      <div
        class="rs-hint left-0 right-0 bottom-0 h-[5px] cursor-ns-resize"
        @pointerdown.prevent="startResize('South', $event)"
      />
      <div
        class="rs-hint bottom-0 left-0 top-0 w-[5px] cursor-ew-resize"
        @pointerdown.prevent="startResize('West', $event)"
      />
      <div
        class="rs-hint bottom-0 right-0 top-0 w-[5px] cursor-ew-resize"
        @pointerdown.prevent="startResize('East', $event)"
      />
      <div
        class="rs-hint left-0 top-0 h-[18px] w-[18px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('NorthWest', $event)"
      />
      <div
        class="rs-hint right-0 top-0 h-[18px] w-[18px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('NorthEast', $event)"
      />
      <div
        class="rs-hint bottom-0 left-0 h-[18px] w-[18px] cursor-nesw-resize"
        @pointerdown.prevent="startResize('SouthWest', $event)"
      />
      <div
        class="rs-hint bottom-0 right-0 h-[18px] w-[18px] cursor-nwse-resize"
        @pointerdown.prevent="startResize('SouthEast', $event)"
      />
    </div>

  </div>
</template>
