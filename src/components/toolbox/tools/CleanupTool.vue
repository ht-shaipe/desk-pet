<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { FolderOpened, Loading } from "@element-plus/icons-vue";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { cancelDiskCleanup, deleteDiskCleanup, scanDiskCleanup } from "@/api/cleanup";
import type { CleanupCandidate, CleanupEvent, CleanupKind } from "@/types";

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

const activeJobId = ref("");
const scanJobId = ref("");
const stream = ref<EventSource | null>(null);

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
    case "scanDone":
      Object.assign(stats, {
        count: data.count,
        bytes: data.bytes,
        errors: data.errors,
        truncated: data.truncated,
      });
      progress.bytes = data.bytes;
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
  if (!scanJobId.value || candidates.value.length === 0) {
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("toolbox.tools.cleanup.confirmBody", {
        count: candidates.value.length,
        size: fmt(stats.bytes || totalBytes.value),
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
    const job = await deleteDiskCleanup(scanJobId.value);
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
    root.value = await homeDir();
  } catch {
    // 取不到用户目录时由用户手动选择
  }
});

onBeforeUnmount(() => {
  const jobId = activeJobId.value;
  const running = busy.value;
  closeStream();
  if (running && jobId) {
    void cancelDiskCleanup(jobId).catch(() => {});
  }
});
</script>

<template>
  <div class="cleanup-tool">
    <el-scrollbar class="view-scroll">
      <p class="intro-hint">{{ t("toolbox.tools.cleanup.rootHint") }}</p>

      <section class="setting-card">
        <div class="card-label">{{ t("toolbox.tools.cleanup.root") }}</div>
        <div class="root-row">
          <el-icon class="root-icon" :size="14"><FolderOpened /></el-icon>
          <span class="root-path" :class="{ empty: !root }" :title="root">
            {{ root || t("toolbox.tools.cleanup.rootEmpty") }}
          </span>
          <el-button size="small" text type="primary" :disabled="busy" @click="pickRoot">
            {{ t("toolbox.tools.cleanup.choose") }}
          </el-button>
        </div>
      </section>

      <section class="setting-card">
        <div class="card-label">{{ t("toolbox.tools.cleanup.scope") }}</div>
        <el-checkbox-group
          v-model="targets"
          class="target-list"
          :disabled="busy"
          @change="resetResults"
        >
          <el-checkbox
            v-for="option in targetOptions"
            :key="option.value"
            :value="option.value"
            class="target-check"
          >
            <span class="target-text">
              <span class="target-name">{{ option.label }}</span>
              <span class="target-hint">{{ option.hint }}</span>
            </span>
          </el-checkbox>
        </el-checkbox-group>
        <div v-if="targets.includes('logs')" class="age-row">
          <span class="age-label">{{ t("toolbox.tools.cleanup.logAge") }}</span>
          <el-select
            v-model="logDays"
            size="small"
            class="age-select"
            :disabled="busy"
            @change="resetResults"
          >
            <el-option :value="7" :label="t('toolbox.tools.cleanup.age.d7')" />
            <el-option :value="30" :label="t('toolbox.tools.cleanup.age.d30')" />
            <el-option :value="0" :label="t('toolbox.tools.cleanup.age.all')" />
          </el-select>
        </div>
        <div class="age-hint" v-if="targets.includes('logs')">
          {{ t("toolbox.tools.cleanup.ageHint") }}
        </div>
      </section>

      <section v-if="statusVisible" class="setting-card status-card">
        <div class="status-head">
          <span class="status-dot" :class="[phase, { partial: phase === 'done' && removal.failed > 0 }]" aria-hidden="true" />
          <span class="status-title">{{ phaseText }}</span>
          <el-icon v-if="busy" class="is-loading status-spin" :size="12"><Loading /></el-icon>
          <span class="status-size" :title="t('toolbox.tools.cleanup.reclaimable')">
            {{ fmt(totalBytes) }}
          </span>
        </div>
        <div class="status-metrics">
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
        <div class="current-path" :title="progress.current">{{ progress.current }}</div>
        <div v-if="busy" class="progress-track" aria-hidden="true">
          <span class="progress-bar" />
        </div>
        <div v-if="stats.truncated" class="warn-line">
          {{ t("toolbox.tools.cleanup.truncated") }}
        </div>
        <div v-if="stats.errors > 0 && !busy" class="warn-line">
          {{ t("toolbox.tools.cleanup.errors", { n: stats.errors }) }}
        </div>
      </section>

      <section v-if="groups.length > 0" class="results">
        <div class="results-head">
          <span class="card-label">{{ t("toolbox.tools.cleanup.results") }}</span>
          <span class="results-total">{{ candidates.length }} · {{ fmt(stats.bytes || progress.bytes) }}</span>
        </div>
        <div v-for="group in groups" :key="group.kind" class="result-group">
          <div class="group-head">
            <span>{{ group.label }}</span>
            <span>{{ group.items.length }} · {{ fmt(group.bytes) }}</span>
          </div>
          <div class="result-list">
            <div
              v-for="item in group.items.slice(0, PREVIEW_LIMIT)"
              :key="item.id"
              class="result-row"
              :class="{ removed: deletedIds.has(item.id), failed: failedIds.has(item.id) }"
            >
              <span class="row-path" :title="item.path">{{ item.path }}</span>
              <el-icon v-if="deletingId === item.id" class="is-loading row-spin" :size="11">
                <Loading />
              </el-icon>
              <span class="row-size">{{ fmt(item.bytes) }}</span>
            </div>
            <div v-if="group.items.length > PREVIEW_LIMIT" class="more">
              {{ t("toolbox.tools.cleanup.more", { n: group.items.length - PREVIEW_LIMIT }) }}
            </div>
          </div>
        </div>
      </section>

      <div v-if="phase === 'scanned' && groups.length === 0" class="empty-result">
        {{ t("toolbox.tools.cleanup.empty") }}
      </div>
    </el-scrollbar>

    <div class="action-bar">
      <p v-if="phase === 'scanned' || phase === 'deleting'" class="delete-warn">
        {{ t("toolbox.tools.cleanup.deleteWarn") }}
      </p>
      <div class="actions">
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
            :disabled="candidates.length === 0"
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

<style scoped>
.cleanup-tool {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.intro-hint {
  margin: 2px 2px 8px;
  font-size: 10.5px;
  line-height: 1.5;
  color: var(--dp-text-tertiary);
}

.setting-card {
  padding: 9px 10px;
  margin-bottom: 8px;
  border: 1px solid var(--dp-card-border);
  border-radius: 10px;
  background: var(--dp-card-bg);
}

.card-label {
  margin-bottom: 6px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.03em;
  color: var(--dp-text-tertiary);
}

.root-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.root-icon {
  flex-shrink: 0;
  color: var(--dp-accent);
}

.root-path {
  flex: 1;
  min-width: 0;
  font-size: 11.5px;
  color: var(--dp-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.root-path.empty {
  color: var(--dp-text-tertiary);
}

.target-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.target-check {
  height: auto;
  margin-right: 0;
  padding: 2px 0;
  align-items: flex-start;
}

.target-check :deep(.el-checkbox__input) {
  margin-top: 2px;
}

.target-check :deep(.el-checkbox__label) {
  padding-left: 8px;
}

.target-text {
  display: flex;
  flex-direction: column;
  line-height: 1.35;
}

.target-name {
  font-size: 12px;
  color: var(--dp-text-secondary);
}

.target-hint {
  font-size: 10px;
  color: var(--dp-text-tertiary);
}

.age-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px dashed var(--dp-divider);
}

.age-label {
  font-size: 11px;
  color: var(--dp-text-muted);
}

.age-select {
  width: 128px;
  margin-left: auto;
}

.age-hint {
  margin-top: 6px;
  font-size: 10px;
  line-height: 1.45;
  color: var(--dp-text-tertiary);
}

.status-card {
  border-color: color-mix(in srgb, var(--dp-accent) 35%, var(--dp-card-border));
}

.status-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--dp-accent);
}

.status-dot.scanned,
.status-dot.done {
  background: #67c23a;
}

.status-dot.deleting {
  background: #e6a23c;
}

.status-dot.partial {
  background: #e6a23c;
}

.status-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--dp-text-secondary);
}

.status-spin {
  color: var(--dp-accent);
}

.status-size {
  margin-left: auto;
  font-size: 11px;
  font-weight: 600;
  color: var(--dp-accent);
}

.status-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  margin-top: 5px;
  font-size: 10.5px;
  color: var(--dp-text-muted);
}

.current-path {
  margin-top: 5px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  color: var(--dp-text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.progress-track {
  height: 3px;
  margin-top: 8px;
  border-radius: 99px;
  background: var(--dp-btn-bg);
  overflow: hidden;
}

.progress-bar {
  display: block;
  width: 32%;
  height: 100%;
  border-radius: 99px;
  background: linear-gradient(90deg, var(--dp-accent), var(--dp-accent-deep, #2f7de1));
  animation: cleanup-slide 1.1s ease-in-out infinite;
}

@keyframes cleanup-slide {
  from {
    transform: translateX(-110%);
  }
  to {
    transform: translateX(340%);
  }
}

.warn-line {
  margin-top: 5px;
  font-size: 10px;
  line-height: 1.45;
  color: #e6a23c;
}

.results {
  margin-top: 2px;
}

.results-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 2px 6px;
}

.results-head .card-label {
  margin-bottom: 0;
}

.results-total {
  margin-left: auto;
  font-size: 10.5px;
  color: var(--dp-text-tertiary);
}

.result-group {
  margin-bottom: 8px;
  border: 1px solid var(--dp-card-border);
  border-radius: 10px;
  background: var(--dp-card-bg);
  overflow: hidden;
}

.group-head {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  background: var(--dp-btn-bg);
  font-size: 10.5px;
  font-weight: 600;
  color: var(--dp-text-secondary);
}

.result-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  border-top: 1px solid var(--dp-divider);
}

.result-row.removed .row-path {
  color: var(--dp-text-tertiary);
  text-decoration: line-through;
  opacity: 0.6;
}

.result-row.failed .row-path {
  color: #f56c6c;
}

.row-path {
  flex: 1;
  min-width: 0;
  font-size: 10px;
  line-height: 1.4;
  color: var(--dp-text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-spin {
  flex-shrink: 0;
  color: var(--dp-accent);
}

.row-size {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--dp-text-tertiary);
}

.more {
  padding: 5px 10px;
  border-top: 1px solid var(--dp-divider);
  font-size: 10px;
  color: var(--dp-text-tertiary);
}

.empty-result {
  padding: 24px 12px;
  text-align: center;
  font-size: 11px;
  color: var(--dp-text-tertiary);
}

.action-bar {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 7px 1px 2px;
  border-top: 1px solid var(--dp-divider);
}

.delete-warn {
  margin: 0;
  font-size: 10px;
  line-height: 1.4;
  color: var(--dp-text-tertiary);
}

.actions {
  display: flex;
  gap: 8px;
}

.actions .el-button {
  margin-left: 0;
}

.flex-1 {
  flex: 1;
}

@media (prefers-reduced-motion: reduce) {
  .progress-bar {
    animation: none;
    width: 100%;
    opacity: 0.5;
  }
}
</style>
