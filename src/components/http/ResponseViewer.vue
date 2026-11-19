<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Clock, DocumentCopy, Hide, Loading, VideoPause, View } from "@element-plus/icons-vue";
import { copyToClipboard } from "@/api/clipboard";
import { textDiff } from "@/api/toolbox";
import { listHttpResponses, clearHttpResponses } from "@/api/toolbox";
import type {
  DiffRow,
  HttpHeader,
  HttpRespRecordItem,
  HttpSseEvent,
  HttpSseStats,
  HttpResponseData,
} from "@/types";
import { evaluateJsonPath } from "@/lib/jsonPath";
import JsonTree from "@/components/http/JsonTree.vue";

const { t } = useI18n();

const props = defineProps<{
  resp: HttpResponseData | null;
  busy: boolean;
  /** 响应历史的请求键（method + 原始 URL） */
  specKey: string;
  /** SSE 流状态：进行中或已有事件时优先展示事件流 */
  sseActive: boolean;
  sseEvents: HttpSseEvent[];
  sseStats: HttpSseStats | null;
}>();

const emit = defineEmits<{
  "stop-sse": [];
}>();

const respTab = ref<"body" | "response" | "request">("body");

/** JSON 自动缩进展示，其余按原文 */
const prettyBody = computed(() => {
  const text = props.resp?.body ?? "";
  if (!text) {
    return "";
  }
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
});

/* ---------- JSON 三视图 + JSONPath ---------- */
const bodyMode = ref<"pretty" | "tree" | "raw">("raw");
const parsedJson = computed<unknown | null>(() => {
  if (props.resp?.binary) {
    return null;
  }
  try {
    const value = JSON.parse(props.resp?.body ?? "");
    return value !== null && typeof value === "object" ? value : null;
  } catch {
    return null;
  }
});
const jsonPathText = ref("");
const jsonPathResult = computed<unknown[] | null>(() => {
  if (!parsedJson.value || !jsonPathText.value.trim()) {
    return null;
  }
  return evaluateJsonPath(parsedJson.value, jsonPathText.value);
});
const shownBody = computed(() => {
  if (bodyMode.value === "raw") {
    return props.resp?.body ?? "";
  }
  if (jsonPathResult.value) {
    try {
      return JSON.stringify(jsonPathResult.value, null, 2);
    } catch {
      return prettyBody.value;
    }
  }
  return prettyBody.value;
});
const jsonPathCount = computed(() => jsonPathResult.value?.length ?? 0);

const shownHeaders = computed<HttpHeader[]>(() => {
  if (!props.resp) {
    return [];
  }
  return respTab.value === "request" ? props.resp.requestHeaders : props.resp.responseHeaders;
});

/* ---------- 密钥打码 ---------- */
const SECRET_NAME_RE = /(authorization|token|secret|password|passwd|api[-_]?key|^key$|cookie|session|credential)/i;
const revealSecrets = ref(false);
const isSecret = (h: HttpHeader): boolean => SECRET_NAME_RE.test(h.name);
const displayValue = (h: HttpHeader): string =>
  !revealSecrets.value && isSecret(h) ? "••••••" : h.value;

/* ---------- 图片预览 ---------- */
const imgInfo = ref("");
function onImgLoad(event: Event) {
  const img = event.target as HTMLImageElement;
  imgInfo.value = `${img.naturalWidth}×${img.naturalHeight} px`;
}

function statusClass(status: number): string {
  if (status >= 200 && status < 300) {
    return "bg-[color-mix(in_srgb,var(--el-color-success,#67c23a)_16%,transparent)] text-[var(--el-color-success,#67c23a)]";
  }
  if (status < 400) {
    return "bg-dp-tag-soft text-dp-accent";
  }
  return "bg-dp-danger-soft text-[var(--el-color-danger,#f56c6c)]";
}

function fmt(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

async function copyBody() {
  if (!shownBody.value) {
    return;
  }
  await copyToClipboard(shownBody.value);
  ElMessage.success(t("toolbox.tools.http.copied"));
}

/* ---------- SSE 事件流 ---------- */
const sseListEl = ref<HTMLElement | null>(null);
watch(
  () => props.sseEvents.length,
  async () => {
    await nextTick();
    sseListEl.value?.scrollTo({ top: sseListEl.value.scrollHeight });
  },
);

function sseTime(index: number): string {
  // 事件序号即时间顺序；此处仅展示序号
  return String(index + 1);
}

/* ---------- 响应历史 + 对比 diff ---------- */
const historyVisible = ref(false);
const historyLoading = ref(false);
const historyItems = ref<HttpRespRecordItem[]>([]);
const selectedIds = ref<string[]>([]);
const diffResult = ref<{ rows: DiffRow[]; added: number; removed: number } | null>(null);
const diffRunning = ref(false);

async function openHistory() {
  historyVisible.value = true;
  historyLoading.value = true;
  selectedIds.value = [];
  diffResult.value = null;
  try {
    historyItems.value = await listHttpResponses(props.specKey);
  } catch {
    historyItems.value = [];
  } finally {
    historyLoading.value = false;
  }
}

function toggleSelect(id: string) {
  const list = selectedIds.value;
  const index = list.indexOf(id);
  if (index >= 0) {
    list.splice(index, 1);
    return;
  }
  if (list.length >= 2) {
    list.shift();
  }
  list.push(id);
}

async function runDiff() {
  const [a, b] = selectedIds.value
    .map((id) => historyItems.value.find((item) => item.id === id))
    .filter(Boolean);
  if (!a || !b) {
    return;
  }
  diffRunning.value = true;
  try {
    // 新记录在右侧（作为"新版本"）
    const [older, newer] = a.create_time <= b.create_time ? [a, b] : [b, a];
    diffResult.value = await textDiff(older.body, newer.body, null);
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    diffRunning.value = false;
  }
}

async function clearHistory() {
  try {
    await clearHttpResponses(props.specKey);
    historyItems.value = [];
    selectedIds.value = [];
    diffResult.value = null;
  } catch (e) {
    ElMessage.error(String(e));
  }
}

const rowText = (row: DiffRow): string => row.segments.map((segment) => segment.text).join("");
const rowClass = (row: DiffRow): string =>
  row.kind === "insert"
    ? "bg-[color-mix(in_srgb,var(--el-color-success,#67c23a)_14%,transparent)]"
    : row.kind === "delete"
      ? "bg-[color-mix(in_srgb,var(--el-color-danger,#f56c6c)_14%,transparent)]"
      : "";
</script>

<template>
  <section
    class="flex-1 min-w-0 flex flex-col min-h-0 p-3 bg-dp-card-bg border border-dp-card-border rounded-[10px]"
  >
    <!-- SSE 事件流（进行中或已有结果时独占响应区） -->
    <template v-if="sseActive || sseEvents.length">
      <div class="shrink-0 flex items-center gap-2 min-w-0">
        <span class="px-2 py-0.5 rounded-md text-[11.5px] font-semibold shrink-0 bg-dp-tag-soft text-dp-accent">
          SSE
        </span>
        <span v-if="sseActive" class="flex items-center gap-1 text-[11px] text-dp-tertiary shrink-0">
          <el-icon :size="12" class="is-loading"><Loading /></el-icon>
          {{ t("toolbox.tools.http.sseStreaming") }}
        </span>
        <span v-if="sseStats" class="text-[11px] text-dp-tertiary shrink-0">
          {{ sseStats.events }} · {{ fmt(sseStats.bytes) }} · {{ sseStats.durationMs }} ms
          <template v-if="sseStats.truncated"> · {{ t("toolbox.tools.http.truncated") }}</template>
        </span>
        <el-button
          v-if="sseActive"
          size="small"
          type="danger"
          plain
          :icon="VideoPause"
          class="ml-auto shrink-0"
          @click="emit('stop-sse')"
        >
          {{ t("toolbox.tools.http.sseStop") }}
        </el-button>
        <span v-else class="ml-auto" />
      </div>

      <el-scrollbar class="flex-1 min-h-0 mt-2.5 dp-scroll">
        <div ref="sseListEl" class="flex flex-col gap-1.5 pr-1">
          <div
            v-for="(event, index) in sseEvents"
            :key="index"
            class="rounded-md bg-dp-panel-bg border border-dp-card-border p-2"
          >
            <div class="flex items-center gap-1.5 mb-1">
              <span class="text-[10px] font-mono text-dp-tertiary">#{{ sseTime(index) }}</span>
              <span
                v-if="event.event"
                class="px-1.5 rounded text-[10px] bg-[color-mix(in_srgb,var(--el-color-primary,#409eff)_14%,transparent)] text-dp-accent"
              >
                {{ event.event }}
              </span>
              <span v-if="event.id" class="text-[10px] text-dp-tertiary">id: {{ event.id }}</span>
            </div>
            <pre class="m-0 text-[11.5px] leading-[1.5] font-mono whitespace-pre-wrap break-all text-dp-secondary select-text">{{ event.data }}</pre>
          </div>
          <div v-if="!sseEvents.length" class="py-6 text-center text-[12px] text-dp-tertiary">
            {{ t("toolbox.tools.http.sseWaiting") }}
          </div>
        </div>
      </el-scrollbar>
    </template>

    <!-- 普通 HTTP 响应 -->
    <template v-else-if="resp">
      <div class="shrink-0 flex items-center gap-2 min-w-0">
        <span
          class="px-2 py-0.5 rounded-md text-[11.5px] font-semibold shrink-0"
          :class="statusClass(resp.status)"
        >
          {{ resp.status }}{{ resp.statusText ? ` ${resp.statusText}` : "" }}
        </span>
        <span class="text-[11px] text-dp-tertiary shrink-0">
          {{ resp.durationMs }} ms · {{ fmt(resp.size) }}
        </span>
        <el-button
          size="small"
          text
          :icon="Clock"
          :title="t('toolbox.tools.http.respHistory')"
          class="ml-auto shrink-0"
          @click="openHistory"
        />
        <el-button
          v-if="!resp.binary && shownBody"
          size="small"
          text
          type="primary"
          :icon="DocumentCopy"
          class="shrink-0"
          @click="copyBody"
        >
          {{ t("toolbox.tools.http.copyBody") }}
        </el-button>
      </div>
      <div v-if="resp.truncated" class="shrink-0 mt-1 text-[11px] text-[var(--el-color-warning,#e6a23c)]">
        {{ t("toolbox.tools.http.truncated") }}
      </div>

      <div class="shrink-0 mt-2.5 flex items-center gap-2 min-w-0">
        <el-radio-group v-model="respTab" size="small">
          <el-radio-button value="body">{{ t("toolbox.tools.http.respBody") }}</el-radio-button>
          <el-radio-button value="response">
            {{ t("toolbox.tools.http.respHeaders") }}
          </el-radio-button>
          <el-radio-button value="request">
            {{ t("toolbox.tools.http.reqHeaders") }}
          </el-radio-button>
        </el-radio-group>
        <el-tooltip
          :content="revealSecrets ? t('toolbox.tools.http.hideSecrets') : t('toolbox.tools.http.showSecrets')"
          placement="bottom"
        >
          <el-button
            size="small"
            text
            :icon="revealSecrets ? Hide : View"
            class="ml-auto shrink-0"
            @click="revealSecrets = !revealSecrets"
          />
        </el-tooltip>
      </div>

      <!-- 响应体：三视图 / 图片 / 二进制 -->
      <template v-if="respTab === 'body'">
        <!-- 视图切换 + JSONPath（仅 JSON 结构时出现） -->
        <div
          v-if="parsedJson"
          class="shrink-0 mt-2.5 flex items-center gap-2 min-w-0"
        >
          <el-radio-group v-model="bodyMode" size="small">
            <el-radio-button value="pretty">{{ t("toolbox.tools.http.viewPretty") }}</el-radio-button>
            <el-radio-button value="tree">{{ t("toolbox.tools.http.viewTree") }}</el-radio-button>
            <el-radio-button value="raw">{{ t("toolbox.tools.http.viewRaw") }}</el-radio-button>
          </el-radio-group>
          <el-input
            v-model="jsonPathText"
            size="small"
            clearable
            placeholder="$.data.list[*].id"
            class="w-[180px] [&_.el-input\_\_inner]:font-mono [&_.el-input\_\_inner]:text-[11px] [&_.el-input\_\_inner]:select-text"
          />
          <span v-if="jsonPathText && jsonPathResult" class="text-[11px] text-dp-tertiary shrink-0">
            {{ jsonPathCount }}
          </span>
        </div>

        <!-- 树形视图 -->
        <el-scrollbar v-if="parsedJson && bodyMode === 'tree'" class="flex-1 min-h-0 mt-2.5 dp-scroll">
          <div class="p-1.5 rounded-md bg-dp-panel-bg border border-dp-card-border min-w-0">
            <JsonTree :value="parsedJson" :expand-depth="2" />
          </div>
        </el-scrollbar>
        <!-- 美化 / 原始 / 过滤结果 -->
        <el-scrollbar v-else class="flex-1 min-h-0 mt-2.5 dp-scroll">
          <div v-if="resp.binary && resp.dataUrl" class="flex flex-col items-center gap-2 py-3">
            <img
              :src="resp.dataUrl"
              class="max-w-full max-h-full object-contain rounded-md border border-dp-card-border select-text"
              alt="response image"
              @load="onImgLoad"
            />
            <span class="text-[11px] text-dp-tertiary">
              {{ imgInfo ? `${imgInfo} · ${fmt(resp.size)}` : fmt(resp.size) }}
            </span>
          </div>
          <div v-else-if="resp.binary" class="text-[12px] text-dp-tertiary">
            {{ t("toolbox.tools.http.binary") }}
          </div>
          <pre
            v-else-if="shownBody"
            class="m-0 p-2.5 rounded-md bg-dp-panel-bg border border-dp-card-border text-[12px] leading-[1.55] font-mono whitespace-pre-wrap break-all text-dp-secondary"
            >{{ shownBody }}</pre
          >
          <div v-else class="text-[12px] text-dp-tertiary">
            {{ t("toolbox.tools.http.emptyValue") }}
          </div>
        </el-scrollbar>
      </template>

      <!-- 响应头/请求头 -->
      <el-scrollbar v-else class="flex-1 min-h-0 mt-2.5 dp-scroll">
        <div class="flex flex-col gap-1.5 pr-1">
          <div
            v-for="(h, i) in shownHeaders"
            :key="`${h.name}-${i}`"
            class="flex gap-2 text-[12px] leading-[1.6] min-w-0"
          >
            <span class="text-dp-accent shrink-0 max-w-[46%] break-all">{{ h.name }}</span>
            <span
              class="text-dp-muted break-all flex-1 min-w-0 select-text"
              :class="{ 'font-mono': isSecret(h) && !revealSecrets }"
            >
              {{ displayValue(h) }}
            </span>
          </div>
          <div v-if="!shownHeaders.length" class="text-[12px] text-dp-tertiary">
            {{ t("toolbox.tools.http.emptyValue") }}
          </div>
        </div>
      </el-scrollbar>

      <div
        class="shrink-0 mt-2 pt-2 border-t border-dp-divider text-[11px] leading-[1.6] text-dp-tertiary break-all"
      >
        {{ t("toolbox.tools.http.finalUrl") }}: {{ resp.finalUrl }}
      </div>
    </template>

    <!-- 空态 -->
    <div v-else class="flex-1 flex items-center justify-center px-4 text-center">
      <span class="text-[12px] leading-[1.8] text-dp-tertiary">
        {{ busy ? t("toolbox.tools.http.sending") : t("toolbox.tools.http.hint") }}
      </span>
    </div>

    <!-- 响应历史 + 对比 -->
    <el-dialog v-model="historyVisible" :title="t('toolbox.tools.http.respHistory')" width="560px">
      <div class="flex flex-col gap-2 min-w-0">
        <div class="flex items-center gap-2">
          <span class="text-[12px] text-dp-muted">
            {{ historyLoading ? t("toolbox.tools.http.sending") : `${historyItems.length}` }}
          </span>
          <el-button size="small" type="primary" :disabled="selectedIds.length !== 2" :loading="diffRunning" @click="runDiff">
            {{ t("toolbox.tools.http.diffCompare") }}
          </el-button>
          <span class="text-[11px] text-dp-tertiary">{{ t("toolbox.tools.http.diffPickHint") }}</span>
          <el-button size="small" text type="danger" class="ml-auto" @click="clearHistory">
            {{ t("common.delete") }}
          </el-button>
        </div>
        <el-scrollbar max-height="220px" class="dp-scroll">
          <div class="flex flex-col gap-1">
            <div
              v-for="item in historyItems"
              :key="item.id"
              class="group flex items-center gap-2 px-2 h-[30px] rounded-md cursor-pointer border"
              :class="selectedIds.includes(item.id) ? 'border-dp-accent bg-[color-mix(in_srgb,var(--el-color-primary,#409eff)_10%,transparent)]' : 'border-dp-card-border'"
              @click="toggleSelect(item.id)"
            >
              <span class="text-[11px] font-semibold shrink-0" :class="item.status >= 400 ? 'text-[var(--el-color-danger,#f56c6c)]' : 'text-[var(--el-color-success,#67c23a)]'">
                {{ item.status }}
              </span>
              <span class="text-[11px] text-dp-tertiary shrink-0">{{ item.create_time.slice(11, 19) }}</span>
              <span class="text-[11px] text-dp-muted shrink-0">{{ item.duration_ms }} ms</span>
              <span class="text-[11px] text-dp-secondary truncate flex-1 min-w-0">
                {{ item.body.slice(0, 80) }}
              </span>
            </div>
            <div v-if="!historyItems.length && !historyLoading" class="py-4 text-center text-[12px] text-dp-tertiary">
              {{ t("toolbox.tools.http.emptyHistory") }}
            </div>
          </div>
        </el-scrollbar>
        <div v-if="diffResult" class="rounded-md border border-dp-card-border overflow-hidden">
          <div class="px-2 py-1 text-[11px] text-dp-muted border-b border-dp-divider">
            +{{ diffResult.added }} −{{ diffResult.removed }}
          </div>
          <el-scrollbar max-height="240px" class="dp-scroll">
            <div class="py-1">
              <div
                v-for="(row, index) in diffResult.rows"
                :key="index"
                class="flex text-[11px] font-mono leading-[1.5] px-1"
                :class="rowClass(row)"
              >
                <span class="w-[34px] text-right pr-1 text-dp-tertiary shrink-0">{{ row.oldLineno ?? "" }}</span>
                <span class="w-[34px] text-right pr-1 text-dp-tertiary shrink-0">{{ row.newLineno ?? "" }}</span>
                <span class="whitespace-pre-wrap break-all flex-1 min-w-0 select-text">{{ rowText(row) }}</span>
              </div>
            </div>
          </el-scrollbar>
        </div>
      </div>
    </el-dialog>
  </section>
</template>
