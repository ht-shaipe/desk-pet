<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { FolderOpened, Loading } from "@element-plus/icons-vue";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { cancelDiskCleanup, deleteDiskCleanup, getDiskUsage, scanDiskCleanup } from "@/api/cleanup";
import type { CleanupCandidate, CleanupEvent, CleanupKind, DiskUsage } from "@/types";

const { t } = useI18n();

type Phase = "idle" | "scanning" | "scanned" | "deleting" | "done";

const KIND_ORDER: CleanupKind[] = ["logs", "target", "node_modules"];
/** 每个分类最多直接渲染的条目数，避免上千条 DOM 拖慢面板 */
const PREVIEW_LIMIT = 50;

const phase = ref<Phase>("idle");
const root = ref("");
const targets = ref<CleanupKind[]>(["logs", "target", "node_modules"]);
const logDays = ref(7);
const candidates = ref<CleanupCandidate[]>([]);
const deletedIds = ref(new Set<string>());
const failedIds = ref(new Set<string>());
const deletingId = ref("");

const stats = reactive({ count: 0, bytes: 0, errors: 0, truncated: false });
const progress = reactive({ dirs: 0, files: 0, bytes: 0, current: "" });
const removal = reactive({ removed: 0, failed: 0, bytes: 0 });

// 勾选待删除项（Set 响应式代理仅对方法调用生效，变更时整体替换以触发更新）
const selectedIds = ref(new Set<string>());

const activeJobId = ref("");
const scanJobId = ref("");
const stream = ref<EventSource | null>(null);

// 扫描目录所在卷的磁盘占用，进入工具/切换目录时异步刷新
const disk = ref<DiskUsage | null>(null);
const diskLoading = ref(false);
let diskSeq = 0;

// 手动输入路径（目录选择器看不到 /tmp 等隐藏目录时直接粘贴路径）
const editingPath = ref(false);
const pathDraft = ref("");
let homeDirCache = "";

const busy = computed(() => phase.value === "scanning" || phase.value === "deleting");
const statusVisible = computed(() => phase.value !== "idle");

const targetOptions = computed(() =>
  KIND_ORDER.map((kind) => ({
    value: kind,
    label: t(`toolbox.tools.cleanup.kinds.${kind}`),
    hint: t(`toolbox.tools.cleanup.hints.${kind}`),
  })),
);

const groups = computed(() =>
  KIND_ORDER.map((kind) => {
    const items = candidates.value.filter((item) => item.kind === kind);
    return {
      kind,
      label: t(`toolbox.tools.cleanup.kinds.${kind}`),
      items,
      bytes: items.reduce((sum, item) => sum + item.bytes, 0),
    };
  }).filter((group) => group.items.length > 0),
);

const totalBytes = computed(() =>
  phase.value === "done" ? removal.bytes : stats.bytes || progress.bytes,
);

const selectedBytes = computed(() =>
  candidates.value.reduce((sum, item) => (selectedIds.value.has(item.id) ? sum + item.bytes : sum), 0),
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

const allSelected = computed(
  () => candidates.value.length > 0 && selectedIds.value.size >= candidates.value.length,
);
const someSelected = computed(
  () => selectedIds.value.size > 0 && selectedIds.value.size < candidates.value.length,
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

function toggleItem(id: string, checked?: boolean) {
  const next = new Set(selectedIds.value);
  const want = checked ?? !next.has(id);
  if (want) {
    next.add(id);
  } else {
    next.delete(id);
  }
  selectedIds.value = next;
}

function toggleSelectAll(checked: boolean) {
  selectedIds.value = checked ? new Set(candidates.value.map((item) => item.id)) : new Set();
}

/** 组标题复选框：整组已选则取消，否则整组勾选。 */
function toggleGroup(group: { items: CleanupCandidate[] }) {
  const selected = group.items.some((item) => selectedIds.value.has(item.id));
  const next = new Set(selectedIds.value);
  for (const item of group.items) {
    if (selected) {
      next.delete(item.id);
    } else {
      next.add(item.id);
    }
  }
  selectedIds.value = next;
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
  deletedIds.value = new Set();
  failedIds.value = new Set();
  selectedIds.value = new Set();
  deletingId.value = "";
  Object.assign(stats, { count: 0, bytes: 0, errors: 0, truncated: false });
  Object.assign(progress, { dirs: 0, files: 0, bytes: 0, current: "" });
  Object.assign(removal, { removed: 0, failed: 0, bytes: 0 });
  scanJobId.value = "";
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
      // 扫描中新发现的项不默认勾选，扫描结束后统一全选
      break;
    case "scanDone":
      Object.assign(stats, {
        count: data.count,
        bytes: data.bytes,
        errors: data.errors,
        truncated: data.truncated,
      });
      progress.bytes = data.bytes;
      selectedIds.value = new Set(candidates.value.map((item) => item.id));
      phase.value = "scanned";
      closeStream();
      break;
    case "deleting":
      deletingId.value = data.id;
      progress.current = data.path;
      break;
    case "deleted":
      deletingId.value = "";
      if (data.ok) {
        deletedIds.value.add(data.id);
        removal.removed += 1;
        removal.bytes += data.bytes;
        // 每删一项刷新上方磁盘占用，让清理进度更直观
        scheduleDiskUsageRefresh();
        // 已删除项退出勾选集
        const next = new Set(selectedIds.value);
        next.delete(data.id);
        selectedIds.value = next;
      } else {
        failedIds.value.add(data.id);
        removal.failed += 1;
      }
      break;
    case "deleteDone":
      removal.removed = data.removed;
      removal.failed = data.failed;
      removal.bytes = data.bytes;
      deletingId.value = "";
      phase.value = "done";
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
            size: fmt(data.bytes),
          }),
        );
      }
      break;
    case "cancelled":
      closeStream();
      if (data.mode === "scan") {
        candidates.value = [];
        phase.value = "idle";
      } else {
        phase.value = "done";
      }
      ElMessage.info(t("toolbox.tools.cleanup.cancelled"));
      break;
  }
}

async function startScan() {
  if (!root.value) {
    ElMessage.warning(t("toolbox.tools.cleanup.rootInvalid"));
    return;
  }
  if (targets.value.length === 0) {
    ElMessage.warning(t("toolbox.tools.cleanup.noSelection"));
    return;
  }
  resetResults();
  phase.value = "scanning";
  try {
    const job = await scanDiskCleanup(root.value, targets.value, logDays.value);
    scanJobId.value = job.jobId;
    connect(job.jobId, job.url);
  } catch (e) {
    phase.value = "idle";
    ElMessage.error(t("toolbox.tools.cleanup.failed", { err: String(e) }));
  }
}

async function startClean() {
  if (!scanJobId.value || selectedIds.value.size === 0) {
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("toolbox.tools.cleanup.confirmBody", {
        count: selectedIds.value.size,
        size: fmt(selectedBytes.value),
      }),
      t("toolbox.tools.cleanup.confirmTitle"),
      {
        type: "warning",
        confirmButtonText: t("toolbox.tools.cleanup.clean"),
        cancelButtonText: t("common.cancel"),
      },
    );
  } catch {
    return;
  }

  phase.value = "deleting";
  Object.assign(removal, { removed: 0, failed: 0, bytes: 0 });
  try {
    const job = await deleteDiskCleanup(scanJobId.value, [...selectedIds.value]);
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
});

// 进入工具默认目录、选择目录或手动输入路径后刷新磁盘占用
watch(root, (value) => {
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
      </section>

      <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
        <div class="mb-1.5 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
          {{ t("toolbox.tools.cleanup.scope") }}
        </div>
        <el-checkbox-group
          v-model="targets"
          class="flex flex-col gap-0.5"
          :disabled="busy"
          @change="resetResults"
        >
          <el-checkbox
            v-for="option in targetOptions"
            :key="option.value"
            :value="option.value"
            class="mr-0 h-auto items-start py-0.5 [&_.el-checkbox\_\_input]:mt-0.5 [&_.el-checkbox\_\_label]:pl-2"
          >
            <span class="flex flex-col leading-[1.35]">
              <span class="text-xs text-dp-secondary">{{ option.label }}</span>
              <span class="text-[10px] text-dp-tertiary">{{ option.hint }}</span>
            </span>
          </el-checkbox>
        </el-checkbox-group>
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
        <div
          v-if="targets.includes('logs')"
          class="mt-1.5 text-[10px] leading-[1.45] text-dp-tertiary"
        >
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
            :title="t('toolbox.tools.cleanup.reclaimable')"
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
            {{ t("toolbox.tools.cleanup.removed") }} {{ removal.removed }} / {{ candidates.length }}
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
        <div
          v-if="stats.errors > 0 && !busy"
          class="mt-[5px] text-[10px] leading-[1.45] text-[#e6a23c]"
        >
          {{ t("toolbox.tools.cleanup.errors", { n: stats.errors }) }}
        </div>
      </section>

      <section v-if="groups.length > 0" class="mt-0.5">
        <div class="flex items-center gap-2 px-0.5 pb-1.5 pt-0.5">
          <el-checkbox
            class="shrink-0 text-[10.5px]"
            :model-value="allSelected"
            :indeterminate="someSelected"
            :disabled="busy || phase === 'done' || failedIds.size > 0"
            @change="(v: boolean) => toggleSelectAll(v)"
          >
            {{ t("toolbox.tools.cleanup.selectAll") }}
          </el-checkbox>
          <span class="mb-0 text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.results") }}
          </span>
          <span class="ml-auto text-[10.5px] text-dp-tertiary">
            {{ t("toolbox.tools.cleanup.selected") }} {{ selectedIds.size }} / {{ candidates.length }} ·
            {{ fmt(selectedBytes) }}
          </span>
        </div>
        <div
          v-for="group in groups"
          :key="group.kind"
          class="mb-2 overflow-hidden rounded-[10px] border border-dp-card-border bg-dp-card-bg"
        >
          <div class="flex items-center justify-between gap-2 bg-dp-btn-bg px-2.5 py-1.5 text-[10.5px] font-semibold text-dp-secondary">
            <span class="flex min-w-0 items-center gap-1.5">
              <el-checkbox
                class="[&_.el-checkbox\_\_label]:p-0 [&_.el-checkbox\_\_label]:text-current"
                :model-value="group.items.every((item) => selectedIds.has(item.id))"
                :indeterminate="
                  group.items.some((item) => selectedIds.has(item.id)) &&
                  !group.items.every((item) => selectedIds.has(item.id))
                "
                :disabled="busy"
                @change="() => toggleGroup(group)"
              />
              <span class="truncate">{{ group.label }}</span>
            </span>
            <span class="shrink-0">{{ group.items.length }} · {{ fmt(group.bytes) }}</span>
          </div>
          <div>
            <div
              v-for="item in group.items.slice(0, PREVIEW_LIMIT)"
              :key="item.id"
              class="flex cursor-pointer items-center gap-1.5 border-t border-dp-divider px-2.5 py-[5px]"
              @click="!busy && !deletedIds.has(item.id) && toggleItem(item.id)"
            >
              <el-checkbox
                class="shrink-0 [&_.el-checkbox\_\_label]:p-0"
                :model-value="selectedIds.has(item.id)"
                :disabled="busy || deletedIds.has(item.id)"
                @change="(v: boolean) => toggleItem(item.id, v)"
                @click.stop
              />
              <span
                class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[10px] leading-[1.4] text-dp-muted [&.failed]:text-[#f56c6c] [&.removed]:text-dp-tertiary [&.removed]:line-through [&.removed]:opacity-60"
                :class="{
                  removed: deletedIds.has(item.id),
                  failed: failedIds.has(item.id),
                }"
                :title="item.path"
              >{{ item.path }}</span>
              <el-icon v-if="deletingId === item.id" class="is-loading shrink-0 text-dp-accent" :size="11">
                <Loading />
              </el-icon>
              <span class="shrink-0 text-[10px] text-dp-tertiary">{{ fmt(item.bytes) }}</span>
            </div>
            <div
              v-if="group.items.length > PREVIEW_LIMIT"
              class="border-t border-dp-divider px-2.5 py-[5px] text-[10px] text-dp-tertiary"
            >
              {{ t("toolbox.tools.cleanup.more", { n: group.items.length - PREVIEW_LIMIT }) }}
            </div>
          </div>
        </div>
      </section>

      <div
        v-if="phase === 'scanned' && groups.length === 0"
        class="px-3 py-6 text-center text-[11px] text-dp-tertiary"
      >
        {{ t("toolbox.tools.cleanup.empty") }}
      </div>
    </el-scrollbar>

    <div class="flex shrink-0 flex-col gap-1.5 border-t border-dp-divider px-px pb-0.5 pt-[7px]">
      <p v-if="phase === 'scanned' || phase === 'deleting'" class="m-0 text-[10px] leading-[1.4] text-dp-tertiary">
        {{ t("toolbox.tools.cleanup.deleteWarn") }}
      </p>
      <div class="flex gap-2 [&_.el-button]:ml-0">
        <el-button
          v-if="phase === 'idle'"
          type="primary"
          class="flex-1"
          :disabled="!root || targets.length === 0"
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
            :disabled="candidates.length === 0 || selectedIds.size === 0"
            @click="startClean"
          >
            {{ t("toolbox.tools.cleanup.clean") }}
          </el-button>
        </template>
        <el-button v-else class="flex-1" @click="startScan">
          {{ t("toolbox.tools.cleanup.rescan") }}
        </el-button>
      </div>
    </div>
  </div>
</template>

