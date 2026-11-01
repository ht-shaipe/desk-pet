<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { FolderOpened, Loading } from "@element-plus/icons-vue";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import {
  cancelDiskCleanup,
  deleteDiskCleanup,
  getDiskUsage,
  listDiskVolumes,
  openFullDiskAccess,
  planDiskCleanup,
  scanDiskCleanup,
} from "@/api/cleanup";
import { openDiskAnalyzer } from "@/api/disk";
import { getPlatform } from "@/api/system";
import type {
  CleanupEvent,
  CleanupRemovalMode,
  DiskCandidate,
  DiskUsage,
  DiskVolume,
  RemovalPlan,
} from "@/types";

const { t } = useI18n();

type Phase = "idle" | "scanning" | "scanned" | "deleting" | "done";
type ResultFilter = "all" | "cache" | "build" | "deps" | "logs" | "trash" | "downloads" | "other";

/** 每类最多直接渲染的条目数，避免上千条 DOM 拖慢面板 */
const PREVIEW_LIMIT = 60;
/** "值得看看"的最小字节阈值（64MB，与后端 insights 对齐） */
const INSIGHT_MIN_BYTES = 64 * 1024 * 1024;

const FILTERS: ResultFilter[] = ["all", "cache", "build", "deps", "logs", "trash", "downloads", "other"];

const phase = ref<Phase>("idle");
const root = ref("");
const logDays = ref(7);
const candidates = ref<DiskCandidate[]>([]);
const deletedPaths = ref(new Set<string>());
const failedPaths = ref(new Set<string>());
const deletingPath = ref("");

const stats = reactive({ count: 0, bytes: 0, errors: 0, truncated: false, complete: true });
const progress = reactive({ dirs: 0, files: 0, bytes: 0, current: "" });
const removal = reactive({
  removed: 0,
  failed: 0,
  bytes: 0,
  freedBytes: 0,
  freeBefore: 0,
  freeAfter: 0,
  failures: [] as { path: string; error: string }[],
});

// 勾选待清理项（key 为展示路径；Set 变更时整体替换以触发更新）
const selectedPaths = ref(new Set<string>());

// 复核清单：删除计划 + 模式（默认回收站，可恢复）
const reviewOpen = ref(false);
const planLoading = ref(false);
const plan = ref<RemovalPlan | null>(null);
const removalMode = ref<CleanupRemovalMode>("trash");
// 删除计划序号：选择变化或关闭复核后，旧请求的响应必须丢弃。
let planSeq = 0;

function invalidatePlan() {
  planSeq += 1;
  reviewOpen.value = false;
  planLoading.value = false;
  plan.value = null;
}

const resultFilter = ref<ResultFilter>("all");
const sortBy = ref<"bytes" | "modified">("bytes");
// 名称子串搜索（忽略大小写）
const nameFilter = ref("");

const activeJobId = ref("");
const scanJobId = ref("");
const stream = ref<EventSource | null>(null);

// 扫描目录所在卷的磁盘占用，进入工具/切换目录时异步刷新
const disk = ref<DiskUsage | null>(null);
const diskLoading = ref(false);
let diskSeq = 0;

// 可扫描卷（外接/网络卷；根分区不允许整卷扫描）
const volumes = ref<DiskVolume[]>([]);
// 当前选中的卷（与 root 同步；root 不在卷列表时回退空串显示占位符）
const selectedVolume = ref("");
// 实时预览（扫描中只读）
const scanningPreview = ref<{ name: string; bytes: number; files: number }[]>([]);
const isMac = ref(false);

// 手动输入路径（目录选择器看不到 /tmp 等隐藏目录时直接粘贴路径）
const editingPath = ref(false);
const pathDraft = ref("");
let homeDirCache = "";

const busy = computed(() => phase.value === "scanning" || phase.value === "deleting");
const statusVisible = computed(() => phase.value !== "idle");

/** 结果过滤：chips 与 reclaim/规则字段的映射。 */
function matchFilter(item: DiskCandidate, filter: ResultFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "cache":
      return (
        item.category === "cache" &&
        item.reclaim !== "trash" &&
        item.ruleId !== "log-files" &&
        item.ruleId !== "logs-upper" &&
        item.ruleId !== "logs-lower"
      );
    case "build":
      return item.reclaim === "buildOutput";
    case "deps":
      return item.reclaim === "packageStore" || item.reclaim === "reinstallable";
    case "logs":
      return (
        item.ruleId === "log-files" ||
        item.ruleId === "logs-upper" ||
        item.ruleId === "logs-lower"
      );
    case "trash":
      return item.reclaim === "trash";
    case "downloads":
      return item.ruleId === "downloads-old";
    case "other":
      return FILTERS.filter((key) => key !== "all" && key !== "other").every(
        (key) => !matchFilter(item, key),
      );
  }
}

const filteredCandidates = computed(() => {
  const needle = nameFilter.value.trim().toLowerCase();
  const items = candidates.value.filter(
    (item) =>
      matchFilter(item, resultFilter.value) &&
      (needle === "" || item.name.toLowerCase().includes(needle)),
  );
  return items.sort((a, b) =>
    sortBy.value === "bytes" ? b.bytes - a.bytes : b.modified - a.modified,
  );
});

/** 推荐清理 = 规则表标记默认勾选的安全项。 */
const recommended = computed(() =>
  candidates.value.filter((item) => item.defaultSelected && item.risk === "safe"),
);
const recommendedBytes = computed(() => recommended.value.reduce((sum, item) => sum + item.bytes, 0));

/** 值得看看：有可回收原因且 ≥64MB 的 Top 8（与后端口径一致）。 */
const insights = computed(() =>
  candidates.value
    .filter((item) => item.reclaim && item.bytes >= INSIGHT_MIN_BYTES)
    .sort((a, b) => b.bytes - a.bytes)
    .slice(0, 8),
);

const totalBytes = computed(() =>
  phase.value === "done" ? removal.bytes : stats.bytes || progress.bytes,
);

const selectedBytes = computed(() =>
  candidates.value.reduce(
    (sum, item) => (selectedPaths.value.has(item.path) ? sum + item.bytes : sum),
    0,
  ),
);

/** 实测释放与估算明显不符时（<90%）给出解释。 */
const freedExplainVisible = computed(
  () =>
    phase.value === "done" &&
    removalMode.value === "permanent" &&
    removal.freedBytes > 0 &&
    removal.freedBytes < removal.bytes * 0.9,
);

const diskPercent = computed(() => {
  if (!disk.value || disk.value.total <= 0) {
    return 0;
  }
  const used = disk.value.total - disk.value.available;
  return Math.min(100, (used / disk.value.total) * 100);
});

/** 用量条颜色：≥90% 红、≥75% 橙，其余走强调色渐变。 */
const diskBarClass = computed(() => {
  if (diskPercent.value >= 90) {
    return "bg-[#f56c6c]";
  }
  if (diskPercent.value >= 75) {
    return "bg-[#e6a23c]";
  }
  return "bg-[linear-gradient(90deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))]";
});

// 全选只覆盖 Safe 项：Risky 永不进入一键全选。
const selectableCandidates = computed(() =>
  filteredCandidates.value.filter((item) => item.risk === "safe"),
);
const allSelected = computed(
  () =>
    selectableCandidates.value.length > 0 &&
    selectableCandidates.value.every((item) => selectedPaths.value.has(item.path)),
);
const someSelected = computed(
  () =>
    filteredCandidates.value.some((item) => selectedPaths.value.has(item.path)) &&
    !allSelected.value,
);
/** 已选计数分母：过滤激活时按过滤后的列表，否则按全部候选 */
const listedCount = computed(() =>
  resultFilter.value === "all" ? candidates.value.length : filteredCandidates.value.length,
);

const phaseText = computed(() => {
  switch (phase.value) {
    case "scanning":
      return t("toolbox.tools.cleanup.scanning");
    case "deleting":
      return t("toolbox.tools.cleanup.deleting");
    case "scanned":
      return t("toolbox.tools.cleanup.scanned");
    case "done":
      return t("toolbox.tools.cleanup.done");
    default:
      return "";
  }
});

/** 状态点颜色：删除中/部分失败橙色，完成绿色，其余强调色。 */
const statusDotClass = computed(() => {
  if (phase.value === "deleting" || (phase.value === "done" && removal.failed > 0)) {
    return "bg-[#e6a23c]";
  }
  if (phase.value === "scanned" || phase.value === "done") {
    return "bg-[#67c23a]";
  }
  return "bg-dp-accent";
});

function riskDotClass(risk: DiskCandidate["risk"]): string {
  if (risk === "safe") {
    return "bg-[#67c23a]";
  }
  if (risk === "caution") {
    return "bg-[#e6a23c]";
  }
  return "bg-[#f56c6c]";
}

/** 候选行的说明：优先可回收原因，其次类别。 */
function itemLabel(item: DiskCandidate): string {
  return item.reclaim
    ? t(`toolbox.tools.cleanup.reclaim.${item.reclaim}`)
    : t(`toolbox.tools.cleanup.category.${item.category}`);
}

function rowTitle(item: DiskCandidate): string {
  const base = `${item.path} · ${t(`toolbox.tools.cleanup.category.${item.category}`)} · ${t(
    `toolbox.tools.cleanup.risk.${item.risk}`,
  )}`;
  return item.complete ? base : `${base} · ${t("toolbox.tools.cleanup.incomplete")}`;
}

function failureOf(path: string): string | undefined {
  return removal.failures?.find((failure) => failure.path === path)?.error;
}

/** 预览条宽度：按当前预览里的最大字节占比。 */
function previewPercent(bytes: number): number {
  const max = Math.max(...scanningPreview.value.map((item) => item.bytes), 1);
  return Math.max(4, Math.round((bytes / max) * 100));
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

/** 异步查询扫描目录所在卷的磁盘占用；序号防竞态，失败时静默收起用量条。 */
async function loadDiskUsage(path: string) {
  const seq = ++diskSeq;
  diskLoading.value = true;
  try {
    const usage = await getDiskUsage(path);
    if (seq === diskSeq) {
      disk.value = usage;
    }
  } catch {
    if (seq === diskSeq) {
      disk.value = null;
    }
  } finally {
    if (seq === diskSeq) {
      diskLoading.value = false;
    }
  }
}

async function pickRoot() {
  if (busy.value) {
    return;
  }
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      title: t("toolbox.tools.cleanup.rootTitle"),
    });
    if (typeof picked === "string" && picked) {
      root.value = picked;
      resetResults();
    }
  } catch (e) {
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

function startPathInput() {
  if (busy.value) {
    return;
  }
  pathDraft.value = root.value;
  editingPath.value = true;
}

async function confirmPathInput() {
  let value = pathDraft.value.trim();
  if (!value) {
    editingPath.value = false;
    return;
  }
  if (value === "~") {
    value = homeDirCache || (await homeDir());
  } else if (value.startsWith("~/")) {
    value = (homeDirCache || (await homeDir())) + value.slice(1);
  }
  if (!value.startsWith("/")) {
    ElMessage.warning(t("toolbox.tools.cleanup.pathInvalid"));
    return;
  }
  root.value = value;
  editingPath.value = false;
  resetResults();
}

function volumeLabel(volume: DiskVolume): string {
  return t("toolbox.tools.cleanup.volumeLabel", { path: volume.path, free: fmt(volume.available) });
}

/** 卷下拉切换：更新扫描根目录并丢弃旧扫描结果。 */
function onPickVolume(path: string) {
  if (!path || path === root.value) {
    return;
  }
  root.value = path;
  resetResults();
}

async function onOpenAnalyzer() {
  try {
    await openDiskAnalyzer();
  } catch (e) {
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

async function onOpenFullDiskAccess() {
  try {
    await openFullDiskAccess();
  } catch (e) {
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

function toggleItem(path: string, checked?: boolean) {
  if (deletedPaths.value.has(path) || reviewOpen.value || planLoading.value) {
    return;
  }
  invalidatePlan();
  const next = new Set(selectedPaths.value);
  const want = checked ?? !next.has(path);
  if (want) {
    next.add(path);
  } else {
    next.delete(path);
  }
  selectedPaths.value = next;
}

/** 点击筛选 chip 只切换视图，不改变选择；勾选必须由用户显式完成。 */
function applyFilterSelection(filter: ResultFilter) {
  if (reviewOpen.value || planLoading.value) {
    return;
  }
  resultFilter.value = filter;
}

/** 全选/取消全选：只作用于当前过滤后的列表中的 Safe 项。 */
function toggleSelectAll(checked: boolean) {
  if (reviewOpen.value || planLoading.value) {
    return;
  }
  invalidatePlan();
  const next = new Set(selectedPaths.value);
  for (const item of filteredCandidates.value) {
    if (deletedPaths.value.has(item.path)) {
      continue;
    }
    if (checked) {
      if (item.risk === "safe") {
        next.add(item.path);
      }
    } else {
      next.delete(item.path);
    }
  }
  selectedPaths.value = next;
}

/** 只勾选安全项（Risky 永远不进推荐与一键全选）。 */
function selectSafe() {
  if (reviewOpen.value || planLoading.value) {
    return;
  }
  invalidatePlan();
  selectedPaths.value = new Set(
    candidates.value.filter((item) => item.risk === "safe").map((item) => item.path),
  );
}

/** 只保留推荐清理（规则表默认勾选集）。 */
function selectRecommendedOnly() {
  if (reviewOpen.value || planLoading.value) {
    return;
  }
  invalidatePlan();
  selectedPaths.value = new Set(recommended.value.map((item) => item.path));
}

function closeStream() {
  stream.value?.close();
  stream.value = null;
}

function resetResults() {
  if (busy.value) {
    return;
  }
  closeStream();
  phase.value = "idle";
  candidates.value = [];
  deletedPaths.value = new Set();
  failedPaths.value = new Set();
  selectedPaths.value = new Set();
  deletingPath.value = "";
  planSeq += 1;
  reviewOpen.value = false;
  planLoading.value = false;
  plan.value = null;
  removalMode.value = "trash";
  resultFilter.value = "all";
  nameFilter.value = "";
  Object.assign(stats, { count: 0, bytes: 0, errors: 0, truncated: false, complete: true });
  Object.assign(progress, { dirs: 0, files: 0, bytes: 0, current: "" });
  scanningPreview.value = [];
  Object.assign(removal, {
    removed: 0,
    failed: 0,
    bytes: 0,
    freedBytes: 0,
    freeBefore: 0,
    freeAfter: 0,
    failures: [],
  });
  scanJobId.value = "";
  activeJobId.value = "";
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
    // 正常结束时前端会主动 close；走到这里说明连接意外中断
    if (phase.value === "scanning") {
      closeStream();
      candidates.value = [];
      phase.value = "idle";
      ElMessage.error(t("toolbox.tools.cleanup.connectFail"));
    } else if (phase.value === "deleting") {
      closeStream();
      const jobId = activeJobId.value;
      if (jobId) {
        void cancelDiskCleanup(jobId).catch(() => {});
      }
      phase.value = "done";
      ElMessage.error(t("toolbox.tools.cleanup.connectFail"));
    }
  };
}

/** 删除过程中磁盘统计的防抖刷新：每删一项都会逼近真实占用，但连续删除时合并请求。 */
let diskRefreshTimer: ReturnType<typeof setTimeout> | undefined;

function scheduleDiskUsageRefresh(): void {
  if (diskRefreshTimer !== undefined || !root.value) {
    return;
  }
  diskRefreshTimer = setTimeout(() => {
    diskRefreshTimer = undefined;
    // 回调里读实时 root.value：挂起期间用户切换了目录也不会刷错对象
    void loadDiskUsage(root.value ?? "");
  }, 220);
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
    case "found":
      candidates.value.push(...data.items);
      break;
    case "preview":
      scanningPreview.value = data.entries;
      break;
    case "scanDone":
      Object.assign(stats, {
        count: data.count,
        bytes: data.bytes,
        errors: data.errors,
        truncated: data.truncated,
        complete: data.complete,
      });
      progress.files = data.files;
      progress.bytes = data.bytes;
      // 默认只勾选规则表标记的安全项（Risky/Caution 需用户手动选）
      selectedPaths.value = new Set(
        candidates.value
          .filter((item) => item.defaultSelected && item.risk === "safe")
          .map((item) => item.path),
      );
      phase.value = "scanned";
      closeStream();
      break;
    case "deleting":
      deletingPath.value = data.path;
      progress.current = data.path;
      break;
    case "deleted":
      deletingPath.value = "";
      if (data.ok) {
        deletedPaths.value.add(data.path);
        removal.removed += 1;
        removal.bytes += data.bytes;
        // 每删一项刷新上方磁盘占用，让清理进度更直观
        scheduleDiskUsageRefresh();
        // 已删除项退出勾选集
        const next = new Set(selectedPaths.value);
        next.delete(data.path);
        selectedPaths.value = next;
      } else {
        failedPaths.value.add(data.path);
        removal.failed += 1;
      }
      break;
    case "deleteDone":
      removal.removed = data.removed;
      removal.failed = data.failed;
      removal.bytes = data.bytes;
      removal.freedBytes = data.freedBytes;
      removal.freeBefore = data.freeBefore;
      removal.freeAfter = data.freeAfter;
      removal.failures = data.failures;
      deletingPath.value = "";
      phase.value = "done";
      invalidatePlan();
      closeStream();
      // 收尾兜底：防抖窗口内的最后几条删除也要体现在磁盘统计里
      scheduleDiskUsageRefresh();
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
          t("toolbox.tools.cleanup.deleteDone", {
            count: data.removed,
            size: fmt(data.freedBytes || data.bytes),
          }),
        );
      }
      break;
    case "cancelled":
      closeStream();
      invalidatePlan();
      if (data.mode === "scan") {
        candidates.value = [];
        phase.value = "idle";
      } else {
        phase.value = "done";
      }
      ElMessage.info(t("toolbox.tools.cleanup.cancelled"));
      break;
    case "failed":
      closeStream();
      invalidatePlan();
      if (data.mode === "scan") {
        candidates.value = [];
        phase.value = "idle";
      } else {
        phase.value = "done";
      }
      ElMessage.error(data.error);
      break;
  }
}

async function startScan() {
  if (!root.value) {
    ElMessage.warning(t("toolbox.tools.cleanup.rootInvalid"));
    return;
  }
  resetResults();
  phase.value = "scanning";
  try {
    const job = await scanDiskCleanup(root.value, logDays.value);
    scanJobId.value = job.jobId;
    connect(job.jobId, job.url);
  } catch (e) {
    phase.value = "idle";
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

/** 打开复核清单：后端权威计算 targets/covered/blocked。 */
async function openReview() {
  if (!scanJobId.value || selectedPaths.value.size === 0) {
    return;
  }
  const seq = ++planSeq;
  const requested = [...selectedPaths.value];
  reviewOpen.value = true;
  planLoading.value = true;
  plan.value = null;
  try {
    const nextPlan = await planDiskCleanup(scanJobId.value, requested);
    if (
      seq !== planSeq ||
      selectedPaths.value.size !== requested.length ||
      !requested.every((path) => selectedPaths.value.has(path))
    ) {
      return;
    }
    plan.value = nextPlan;
  } catch (e) {
    if (seq === planSeq) {
      reviewOpen.value = false;
      plan.value = null;
      ElMessage.error(t("toolbox.tools.cleanup.planFailed", { err: String(e) }));
    }
  } finally {
    if (seq === planSeq) {
      planLoading.value = false;
    }
  }
}

/** 复核确认：只删除计划里的 targets（covered 已被吸收，blocked 已被拒绝）。 */
async function confirmReview() {
  const currentPlan = plan.value;
  if (!currentPlan || currentPlan.targets.length === 0) {
    return;
  }
  const paths = currentPlan.targets.map((target) => target.path);
  const permanent = removalMode.value === "permanent";
  try {
    await ElMessageBox.confirm(
      permanent
        ? t("toolbox.tools.cleanup.review.confirmPermanent", {
            count: paths.length,
            size: fmt(currentPlan.reclaim),
          })
        : t("toolbox.tools.cleanup.review.confirmTrash", {
            count: paths.length,
            size: fmt(currentPlan.reclaim),
          }),
      t("toolbox.tools.cleanup.confirmTitle"),
      {
        type: permanent ? "warning" : "info",
        confirmButtonText: permanent
          ? t("toolbox.tools.cleanup.deletePermanently")
          : t("toolbox.tools.cleanup.moveToTrash"),
        cancelButtonText: t("common.cancel"),
      },
    );
  } catch {
    return;
  }

  phase.value = "deleting";
  reviewOpen.value = false;
  Object.assign(removal, {
    removed: 0,
    failed: 0,
    bytes: 0,
    freedBytes: 0,
    freeBefore: 0,
    freeAfter: 0,
    failures: [],
  });
  try {
    const job = await deleteDiskCleanup(scanJobId.value, paths, removalMode.value);
    connect(job.jobId, job.url);
  } catch (e) {
    phase.value = "scanned";
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

async function stop() {
  const wasScanning = phase.value === "scanning";
  const jobId = activeJobId.value;
  closeStream();
  if (jobId) {
    try {
      await cancelDiskCleanup(jobId);
    } catch {
      // 任务可能已经结束，忽略取消失败
    }
  }
  invalidatePlan();
  if (wasScanning) {
    candidates.value = [];
    phase.value = "idle";
  } else if (phase.value === "deleting") {
    phase.value = "done";
  }
  ElMessage.info(t("toolbox.tools.cleanup.cancelled"));
}

onMounted(async () => {
  try {
    homeDirCache = await homeDir();
    root.value = homeDirCache;
  } catch {
    // 取不到用户目录时由用户手动选择
  }
  isMac.value = (await getPlatform()) === "macos";
  try {
    volumes.value = await listDiskVolumes();
    selectedVolume.value = volumes.value.find((v) => v.path === root.value)?.path ?? "";
  } catch {
    volumes.value = [];
  }
});

// 进入工具默认目录、选择目录或手动输入路径后刷新磁盘占用；同时同步卷下拉选中态
watch(root, (value) => {
  selectedVolume.value = volumes.value.some((v) => v.path === value) ? value : "";
  if (value) {
    void loadDiskUsage(value);
  }
});

onBeforeUnmount(() => {
  const jobId = activeJobId.value;
  const running = busy.value;
  closeStream();
  if (diskRefreshTimer !== undefined) {
    clearTimeout(diskRefreshTimer);
    diskRefreshTimer = undefined;
  }
  if (running && jobId) {
    void cancelDiskCleanup(jobId).catch(() => {});
  }
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- 磁盘占用传送到工具箱标题栏右侧显示 -->
    <Teleport to="#toolbox-toolbar-extra">
      <el-icon v-if="diskLoading" class="is-loading shrink-0 text-dp-accent" :size="12">
        <Loading />
      </el-icon>
      <template v-else-if="disk">
        <div class="h-1 w-14 shrink-0 overflow-hidden rounded-[99px] bg-dp-btn-bg" aria-hidden="true">
          <span
            class="block h-full rounded-[99px] transition-[width] duration-300 motion-reduce:transition-none motion-reduce:opacity-70"
            :class="diskBarClass"
            :style="{ width: `${diskPercent.toFixed(1)}%` }"
          />
        </div>
        <span
          class="min-w-0 truncate text-[10px] text-dp-tertiary"
          :title="t('toolbox.tools.cleanup.diskFree', { size: fmt(disk.available) })"
        >
          {{
            t("toolbox.tools.cleanup.diskUsed", {
              used: fmt(disk.total - disk.available),
              total: fmt(disk.total),
            })
          }}
        </span>
      </template>
    </Teleport>
    <el-scrollbar class="view-scroll">
      <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
        <div class="mb-1.5 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
          {{ t("toolbox.tools.cleanup.root") }}
        </div>
        <div v-if="!editingPath" class="flex min-w-0 items-center gap-1.5">
          <el-icon class="shrink-0 text-dp-accent" :size="14"><FolderOpened /></el-icon>
          <span
            class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[11.5px]"
            :class="root ? 'text-dp-secondary' : 'text-dp-tertiary'"
            :title="root"
          >
            {{ root || t("toolbox.tools.cleanup.rootEmpty") }}
          </span>
          <el-button size="small" text type="primary" :disabled="busy" @click="startPathInput">
            {{ t("toolbox.tools.cleanup.pathInput") }}
          </el-button>
          <el-button size="small" text type="primary" :disabled="busy" @click="pickRoot">
            {{ t("toolbox.tools.cleanup.choose") }}
          </el-button>
          <el-button size="small" text type="primary" :disabled="!root" @click="onOpenAnalyzer">
            {{ t("toolbox.tools.cleanup.openAnalyzer") }}
          </el-button>
        </div>
        <div v-else class="flex min-w-0 items-center gap-1.5">
          <el-input
            v-model="pathDraft"
            size="small"
            class="min-w-0 flex-1 [&_.el-input\_\_inner]:text-[11.5px] [&_.el-input\_\_wrapper]:px-2 [&_.el-input\_\_wrapper]:py-0"
            placeholder="/tmp ~ 或绝对路径"
            autofocus
            @keyup.enter="confirmPathInput"
            @keyup.esc="editingPath = false"
          />
          <el-button size="small" text type="primary" @click="confirmPathInput">
            {{ t("common.confirm") }}
          </el-button>
          <el-button size="small" text @click="editingPath = false">
            {{ t("common.cancel") }}
          </el-button>
        </div>
        <div
          v-if="volumes.length > 0"
          class="mt-2 flex items-center gap-1.5 border-t border-dashed border-dp-divider pt-2"
        >
          <span class="text-[11px] text-dp-muted">{{ t("toolbox.tools.cleanup.pickVolume") }}</span>
          <el-select
            v-model="selectedVolume"
            size="small"
            class="ml-auto flex-1"
            :placeholder="t('toolbox.tools.cleanup.pickVolume')"
            :disabled="busy"
            @change="onPickVolume"
          >
            <el-option
              v-for="volume in volumes"
              :key="volume.path"
              :value="volume.path"
              :label="volumeLabel(volume)"
            />
          </el-select>
        </div>
        <div class="mt-2 flex items-center gap-2 border-t border-dashed border-dp-divider pt-2">
          <span class="text-[11px] text-dp-muted">{{ t("toolbox.tools.cleanup.logAge") }}</span>
          <el-select
            v-model="logDays"
            size="small"
            class="ml-auto w-32"
            :disabled="busy"
            @change="resetResults"
          >
            <el-option :value="7" :label="t('toolbox.tools.cleanup.age.d7')" />
            <el-option :value="30" :label="t('toolbox.tools.cleanup.age.d30')" />
            <el-option :value="0" :label="t('toolbox.tools.cleanup.age.all')" />
          </el-select>
        </div>
        <div class="mt-1.5 text-[10px] leading-[1.45] text-dp-tertiary">
          {{ t("toolbox.tools.cleanup.ageHint") }}
        </div>
      </section>

      <section
        v-if="statusVisible"
        class="mb-2 rounded-[10px] border border-[color-mix(in_srgb,var(--dp-accent)_35%,var(--dp-card-border))] bg-dp-card-bg px-2.5 py-[9px]"
      >
        <div class="flex items-center gap-1.5">
          <span class="size-1.5 shrink-0 rounded-full" :class="statusDotClass" aria-hidden="true" />
          <span class="text-xs font-semibold text-dp-secondary">{{ phaseText }}</span>
          <el-icon v-if="busy" class="is-loading text-dp-accent" :size="12"><Loading /></el-icon>
          <span
            class="ml-auto text-[11px] font-semibold text-dp-accent"
            :title="t('toolbox.tools.cleanup.reclaimableHint')"
          >
            {{ fmt(totalBytes) }}
          </span>
        </div>
        <div class="mt-[5px] flex flex-wrap gap-x-2.5 gap-y-1 text-[10.5px] text-dp-muted">
          <span>
            {{ t("toolbox.tools.cleanup.found") }}
            {{ phase === "scanned" ? stats.count : candidates.length }}
          </span>
          <span v-if="phase === 'scanning'">
            {{ progress.dirs }} {{ t("toolbox.tools.cleanup.dirs") }} ·
            {{ progress.files }} {{ t("toolbox.tools.cleanup.files") }}
          </span>
          <span v-if="phase === 'done'">
            {{ t("toolbox.tools.cleanup.removed") }} {{ removal.removed }}
            <template v-if="removal.failed > 0"> · {{ t("toolbox.tools.cleanup.failedCount") }} {{ removal.failed }}</template>
          </span>
          <span v-if="phase === 'deleting'">
            {{ t("toolbox.tools.cleanup.removed") }} {{ removal.removed }} / {{ plan?.targets.length ?? candidates.length }}
          </span>
        </div>
        <!-- 实测释放：statvfs 前后差值，与估算不等时解释原因 -->
        <div
          v-if="phase === 'done' && removalMode === 'permanent' && removal.freedBytes > 0"
          class="mt-[5px] flex flex-wrap items-center gap-x-2 text-[10.5px]"
        >
          <span class="font-semibold text-[#67c23a]">
            {{ t("toolbox.tools.cleanup.freedActual", { size: fmt(removal.freedBytes) }) }}
          </span>
          <span v-if="freedExplainVisible" class="text-[10px] leading-[1.45] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.freedExplain") }}
          </span>
        </div>
        <div
          class="mt-[5px] overflow-hidden text-ellipsis whitespace-nowrap font-[ui-monospace,SFMono-Regular,Menlo,monospace] text-[10px] text-dp-tertiary"
          :title="progress.current"
        >
          {{ progress.current }}
        </div>
        <div v-if="busy" class="mt-2 h-[3px] overflow-hidden rounded-[99px] bg-dp-btn-bg" aria-hidden="true">
          <span
            class="block h-full w-[32%] rounded-[99px] bg-[linear-gradient(90deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))] animate-cleanup-slide motion-reduce:h-full motion-reduce:w-full motion-reduce:opacity-50 motion-reduce:animate-none"
          />
        </div>
        <div v-if="stats.truncated" class="mt-[5px] text-[10px] leading-[1.45] text-[#e6a23c]">
          {{ t("toolbox.tools.cleanup.truncated") }}
        </div>
        <!-- 扫描预算耗尽：总量是下界，必须显式标注 -->
        <div v-if="!stats.complete && phase === 'scanned'" class="mt-[5px] text-[10px] leading-[1.45] text-[#e6a23c]">
          {{ t("toolbox.tools.cleanup.incomplete") }}
        </div>
        <div
          v-if="stats.errors > 0 && !busy"
          class="mt-[5px] text-[10px] leading-[1.45] text-[#e6a23c]"
        >
          {{ t("toolbox.tools.cleanup.errors", { n: stats.errors }) }}
        </div>
        <!-- 实时预览：扫描中只读（完成后才可操作） -->
        <template v-if="phase === 'scanning' && scanningPreview.length > 0">
          <div class="mt-2 flex items-center gap-1.5 text-[10px] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.scanPreview") }}
          </div>
          <div class="mt-1 flex flex-col gap-[3px]">
            <div v-for="entry in scanningPreview.slice(0, 6)" :key="entry.name" class="flex items-center gap-1.5">
              <span class="min-w-0 flex-1 truncate text-[10px] text-dp-muted" :title="entry.name">
                {{ entry.name || t("toolbox.tools.cleanup.previewOverflow") }}
              </span>
              <div class="h-[3px] w-16 shrink-0 overflow-hidden rounded-[99px] bg-dp-btn-bg" aria-hidden="true">
                <div
                  class="h-full rounded-[99px] bg-dp-accent"
                  :style="{ width: `${previewPercent(entry.bytes)}%` }"
                />
              </div>
              <span class="w-14 shrink-0 text-right text-[10px] text-dp-tertiary">{{ fmt(entry.bytes) }}</span>
            </div>
          </div>
        </template>
      </section>

      <!-- Full Disk Access 引导：权限不足时错误计数偏高（仅 macOS） -->
      <section
        v-if="isMac && stats.errors >= 10 && !busy"
        class="mb-2 rounded-[10px] border border-[#e6a23c55] bg-dp-card-bg px-2.5 py-[9px]"
      >
        <div class="text-[10.5px] leading-[1.5] text-dp-muted">
          {{ t("toolbox.tools.cleanup.fdaHint") }}
        </div>
        <el-button size="small" text type="primary" class="mt-1" @click="onOpenFullDiskAccess">
          {{ t("toolbox.tools.cleanup.fdaOpen") }}
        </el-button>
      </section>

      <!-- 推荐清理：规则表默认勾选的安全项 -->
      <section
        v-if="phase === 'scanned' && recommended.length > 0"
        class="mb-2 rounded-[10px] border border-[color-mix(in_srgb,var(--dp-accent)_35%,var(--dp-card-border))] bg-dp-card-bg px-2.5 py-[9px]"
      >
        <div class="flex items-center gap-1.5">
          <span class="text-[11px] font-semibold text-dp-secondary">
            {{ t("toolbox.tools.cleanup.recommended") }}
          </span>
          <span class="text-[10px] text-dp-tertiary">
            {{ recommended.length }} · {{ fmt(recommendedBytes) }}
          </span>
          <el-button
            size="small"
            text
            type="primary"
            class="ml-auto"
            :disabled="busy || reviewOpen || planLoading"
            @click="selectSafe"
          >
            {{ t("toolbox.tools.cleanup.selectSafe") }}
          </el-button>
          <el-button
            size="small"
            text
            :disabled="busy || reviewOpen || planLoading"
            @click="selectRecommendedOnly"
          >
            {{ t("common.reset") }}
          </el-button>
        </div>
        <div class="mt-[3px] text-[10px] leading-[1.45] text-dp-tertiary">
          {{ t("toolbox.tools.cleanup.recommendedHint") }}
        </div>
      </section>

      <!-- 值得看看：Top N 可回收大项，点击直接勾选 -->
      <section v-if="phase === 'scanned' && insights.length > 0" class="mb-2">
        <div class="px-0.5 pb-1 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
          {{ t("toolbox.tools.cleanup.worthALook") }}
        </div>
        <div class="overflow-hidden rounded-[10px] border border-dp-card-border bg-dp-card-bg">
          <button
            v-for="item in insights"
            :key="item.path"
            type="button"
            class="flex w-full cursor-pointer items-center gap-1.5 border-t border-dp-divider px-2.5 py-[5px] text-left first:border-t-0 hover:bg-dp-hover-bg"
            :disabled="busy || deletedPaths.has(item.path)"
            @click="toggleItem(item.path)"
          >
            <span class="min-w-0 flex-1 truncate text-[10px] leading-[1.4] text-dp-muted" :title="item.path">
              {{ item.path }}
            </span>
            <span class="shrink-0 text-[9.5px] text-dp-tertiary">
              {{ t(`toolbox.tools.cleanup.reclaim.${item.reclaim}`) }}
            </span>
            <span class="shrink-0 text-[10px] text-dp-tertiary">{{ fmt(item.bytes) }}</span>
          </button>
        </div>
      </section>

      <section v-if="filteredCandidates.length > 0" class="mt-0.5">
        <div class="flex items-center gap-2 px-0.5 pb-1.5 pt-0.5">
          <el-checkbox
            class="shrink-0 text-[10.5px]"
            :model-value="allSelected"
            :indeterminate="someSelected"
            :disabled="busy || phase === 'done' || failedPaths.size > 0 || reviewOpen || planLoading"
            @change="(v: boolean) => toggleSelectAll(v)"
          >
            {{ t("toolbox.tools.cleanup.selectAllSafe") }}
          </el-checkbox>
          <span class="mb-0 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.results") }}
          </span>
          <el-input
            v-model="nameFilter"
            size="small"
            clearable
            class="ml-auto w-28 [&_.el-input\_\_inner]:text-[10.5px] [&_.el-input\_\_wrapper]:px-2 [&_.el-input\_\_wrapper]:py-0"
            :placeholder="t('toolbox.tools.cleanup.searchPlaceholder')"
          />
          <span class="shrink-0 text-[10.5px] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.selected") }} {{ selectedPaths.size }} / {{ listedCount }} ·
            {{ fmt(selectedBytes) }}
          </span>
        </div>
        <!-- 过滤 chips + 排序 -->
        <div class="mb-1.5 flex flex-wrap items-center gap-1 px-0.5">
          <button
            v-for="key in FILTERS"
            :key="key"
            type="button"
            class="cursor-pointer rounded-full border px-2 py-[2px] text-[10px] transition-[background,color] duration-150"
            :class="
              resultFilter === key
                ? 'border-transparent bg-dp-accent text-white'
                : 'border-dp-card-border bg-dp-card-bg text-dp-muted hover:text-dp-primary'
            "
            :disabled="reviewOpen || planLoading"
            @click="applyFilterSelection(key)"
          >
            {{ t(`toolbox.tools.cleanup.filter.${key}`) }}
          </button>
          <button
            type="button"
            class="ml-auto cursor-pointer rounded-full border border-dp-card-border bg-dp-card-bg px-2 py-[2px] text-[10px] text-dp-muted hover:text-dp-primary"
            @click="sortBy = sortBy === 'bytes' ? 'modified' : 'bytes'"
          >
            {{ sortBy === "bytes" ? t("toolbox.tools.cleanup.sortSize") : t("toolbox.tools.cleanup.sortTime") }}
          </button>
        </div>
        <div class="overflow-hidden rounded-[10px] border border-dp-card-border bg-dp-card-bg">
          <div
            v-for="item in filteredCandidates.slice(0, PREVIEW_LIMIT)"
            :key="item.path"
            class="cursor-pointer border-t border-dp-divider px-2.5 py-[5px] first:border-t-0"
            :class="{ 'hover:bg-dp-hover-bg': !busy && !deletedPaths.has(item.path) }"
            @click="!busy && !deletedPaths.has(item.path) && toggleItem(item.path)"
          >
            <div class="flex items-center gap-1.5">
              <el-checkbox
                class="shrink-0 [&_.el-checkbox\_\_label]:p-0"
                :model-value="selectedPaths.has(item.path)"
                :disabled="busy || deletedPaths.has(item.path) || reviewOpen || planLoading"
                @change="(v: boolean) => toggleItem(item.path, v)"
                @click.stop
              />
              <span
                class="size-1.5 shrink-0 rounded-full"
                :class="riskDotClass(item.risk)"
                :title="t(`toolbox.tools.cleanup.risk.${item.risk}`)"
                aria-hidden="true"
              />
              <span
                class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-[ui-monospace,SFMono-Regular,Menlo,monospace] text-[10px] leading-[1.4] text-dp-muted [&.failed]:text-[#f56c6c] [&.removed]:text-dp-tertiary [&.removed]:line-through [&.removed]:opacity-60"
                :class="{
                  removed: deletedPaths.has(item.path),
                  failed: failedPaths.has(item.path),
                }"
                :title="failedPaths.has(item.path) ? failureOf(item.path) ?? item.path : rowTitle(item)"
              >{{ item.path }}</span>
              <el-icon v-if="deletingPath === item.path" class="is-loading shrink-0 text-dp-accent" :size="11">
                <Loading />
              </el-icon>
              <span class="shrink-0 text-[10px] text-dp-tertiary">{{ fmt(item.bytes) }}</span>
            </div>
            <div class="mt-[1px] flex items-center gap-1.5 pl-[26px] text-[9.5px] leading-[1.4] text-dp-tertiary">
              <span>{{ t(`toolbox.tools.cleanup.category.${item.category}`) }}</span>
              <span v-if="item.reclaim" class="text-dp-muted">· {{ itemLabel(item) }}</span>
              <span v-if="!item.complete" class="text-[#e6a23c]">
                · {{ t("toolbox.tools.cleanup.incomplete") }}
              </span>
            </div>
          </div>
          <div
            v-if="filteredCandidates.length > PREVIEW_LIMIT"
            class="border-t border-dp-divider px-2.5 py-[5px] text-[10px] text-dp-tertiary"
          >
            {{ t("toolbox.tools.cleanup.more", { n: filteredCandidates.length - PREVIEW_LIMIT }) }}
          </div>
        </div>
      </section>

      <div
        v-if="phase === 'scanned' && candidates.length === 0"
        class="px-3 py-6 text-center text-[11px] text-dp-tertiary"
      >
        {{ t("toolbox.tools.cleanup.empty") }}
      </div>

      <!-- 复核清单：targets / covered / blocked + 删除方式 -->
      <section
        v-if="reviewOpen"
        class="mb-2 rounded-[10px] border border-[#e6a23c55] bg-dp-card-bg px-2.5 py-[9px]"
      >
        <div class="flex items-center gap-1.5">
          <span class="text-[11px] font-semibold text-dp-secondary">
            {{ t("toolbox.tools.cleanup.review.title") }}
          </span>
          <el-icon v-if="planLoading" class="is-loading text-dp-accent" :size="12"><Loading /></el-icon>
          <el-button size="small" text class="ml-auto" :disabled="planLoading" @click="invalidatePlan">
            {{ t("common.cancel") }}
          </el-button>
        </div>
        <template v-if="plan">
          <div class="mt-1.5 text-[10.5px] text-dp-secondary">
            {{ t("toolbox.tools.cleanup.review.count", { n: plan.targets.length }) }} ·
            {{ fmt(plan.reclaim) }}
          </div>
          <div class="mt-1 max-h-24 overflow-y-auto rounded-[8px] bg-dp-btn-bg px-2 py-1">
            <div
              v-for="target in plan.targets.slice(0, 50)"
              :key="target.path"
              class="truncate font-[ui-monospace,SFMono-Regular,Menlo,monospace] text-[9.5px] leading-[1.5] text-dp-muted"
              :title="target.path"
            >
              {{ target.path }}
            </div>
            <div v-if="plan.targets.length > 50" class="text-[9.5px] text-dp-tertiary">
              …
            </div>
          </div>
          <div v-if="plan.covered.length > 0" class="mt-1 text-[10px] leading-[1.45] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.review.covered", { n: plan.covered.length }) }}
          </div>
          <template v-if="plan.blocked.length > 0">
            <div class="mt-1 text-[10px] font-semibold text-[#e6a23c]">
              {{ t("toolbox.tools.cleanup.review.blocked", { n: plan.blocked.length }) }}
            </div>
            <div class="mt-0.5 max-h-20 overflow-y-auto">
              <div
                v-for="blocked in plan.blocked"
                :key="blocked.path"
                class="truncate text-[9.5px] leading-[1.5] text-dp-tertiary"
                :title="`${blocked.path}: ${blocked.error}`"
              >
                {{ blocked.path }} — {{ blocked.error }}
              </div>
            </div>
          </template>
          <div class="mt-2 border-t border-dashed border-dp-divider pt-1.5">
            <div class="text-[10.5px] font-semibold text-dp-tertiary">
              {{ t("toolbox.tools.cleanup.review.mode") }}
            </div>
            <el-radio-group v-model="removalMode" class="mt-1 flex flex-col gap-0.5">
              <el-radio value="trash" class="mr-0 h-auto items-start py-0.5 [&_.el-radio\_\_label]:pl-2">
                <span class="flex flex-col leading-[1.35]">
                  <span class="text-[11px] text-dp-secondary">{{ t("toolbox.tools.cleanup.moveToTrash") }}</span>
                  <span class="text-[10px] text-dp-tertiary">{{ t("toolbox.tools.cleanup.review.trashHint") }}</span>
                </span>
              </el-radio>
              <el-radio value="permanent" class="mr-0 h-auto items-start py-0.5 [&_.el-radio\_\_label]:pl-2">
                <span class="flex flex-col leading-[1.35]">
                  <span class="text-[11px] text-dp-secondary">{{ t("toolbox.tools.cleanup.deletePermanently") }}</span>
                  <span class="text-[10px] text-dp-tertiary">{{ t("toolbox.tools.cleanup.review.permanentHint") }}</span>
                </span>
              </el-radio>
            </el-radio-group>
            <el-button
              :type="removalMode === 'permanent' ? 'danger' : 'primary'"
              class="mt-2 w-full [&_.el-button]:ml-0"
              :disabled="plan.targets.length === 0"
              @click="confirmReview"
            >
              {{
                removalMode === "permanent"
                  ? t("toolbox.tools.cleanup.review.confirmPermanent", {
                      count: plan.targets.length,
                      size: fmt(plan.reclaim),
                    })
                  : t("toolbox.tools.cleanup.review.confirmTrash", {
                      count: plan.targets.length,
                      size: fmt(plan.reclaim),
                    })
              }}
            </el-button>
          </div>
        </template>
      </section>
    </el-scrollbar>

    <div class="flex shrink-0 flex-col gap-1.5 border-t border-dp-divider px-px pb-0.5 pt-[7px]">
      <div class="flex gap-2 [&_.el-button]:ml-0">
        <el-button
          v-if="phase === 'idle'"
          type="primary"
          class="flex-1"
          :disabled="!root"
          @click="startScan"
        >
          {{ t("toolbox.tools.cleanup.scan") }}
        </el-button>
        <el-button v-else-if="busy" plain type="danger" class="flex-1" @click="stop">
          {{ t("toolbox.tools.cleanup.stop") }}
        </el-button>
        <template v-else-if="phase === 'scanned'">
          <el-button class="flex-1" @click="startScan">
            {{ t("toolbox.tools.cleanup.rescan") }}
          </el-button>
          <el-button
            type="primary"
            class="flex-1"
            :disabled="selectedPaths.size === 0 || reviewOpen"
            @click="openReview"
          >
            {{ t("toolbox.tools.cleanup.review.title") }}
          </el-button>
        </template>
        <el-button v-else class="flex-1" @click="startScan">
          {{ t("toolbox.tools.cleanup.rescan") }}
        </el-button>
      </div>
    </div>
  </div>
</template>
