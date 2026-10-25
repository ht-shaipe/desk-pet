<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Close, DocumentCopy, Plus, Promotion } from "@element-plus/icons-vue";
import { httpRequest } from "@/api/toolbox";
import { copyToClipboard } from "@/api/clipboard";
import type { HttpHeader, HttpRequestSpec, HttpResponseData } from "@/types";

const { t } = useI18n();

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/**
 * JSON 模式下的输入框占位示例。
 * 不能放进 i18n：花括号会被 vue-i18n 消息编译器当作插值语法，渲染时抛错导致整块 UI 卡住。
 */
const bodyPhJson = '{\n  "key": "value"\n}';

/** 键值行带 uid：增删行时输入框内容不会串行 */
interface KvRow {
  uid: number;
  name: string;
  value: string;
}
let rowSeq = 0;
function newRow(): KvRow {
  return { uid: ++rowSeq, name: "", value: "" };
}

const url = ref("");
const method = ref("GET");
const params = ref<KvRow[]>([newRow()]);
const headers = ref<KvRow[]>([newRow()]);
const bodyType = ref<"json" | "text">("json");
const body = ref("");

const reqTab = ref<"params" | "body" | "headers">("params");
const respTab = ref<"body" | "response" | "request">("body");
const busy = ref(false);
const resp = ref<HttpResponseData | null>(null);

/** GET / HEAD 常不带体，但代理不拦截：填了就照原样发送 */
const bodyless = computed(() => method.value === "GET" || method.value === "HEAD");
const paramCount = computed(() => params.value.filter((r) => r.name.trim()).length);
const headerCount = computed(() => headers.value.filter((r) => r.name.trim()).length);

/** 参数页与请求头页共用一套行编辑器 */
const activeRows = computed(() => (reqTab.value === "params" ? params.value : headers.value));

function addRow() {
  activeRows.value.push(newRow());
}

function removeRow(uid: number) {
  const rows = activeRows.value;
  const i = rows.findIndex((r) => r.uid === uid);
  if (i >= 0) {
    rows.splice(i, 1);
  }
}

function toPairs(rows: KvRow[]): HttpHeader[] {
  return rows
    .filter((r) => r.name.trim())
    .map((r) => ({ name: r.name.trim(), value: r.value }));
}

async function send() {
  if (busy.value) {
    return;
  }
  const target = url.value.trim();
  if (!/^https?:\/\//i.test(target)) {
    ElMessage.warning(t("toolbox.tools.http.invalidUrl"));
    return;
  }
  const spec: HttpRequestSpec = {
    url: target,
    method: method.value,
    params: toPairs(params.value),
    headers: toPairs(headers.value),
    bodyType: bodyType.value,
    body: body.value || undefined,
  };
  busy.value = true;
  try {
    resp.value = await httpRequest(spec);
    respTab.value = "body";
  } catch (e) {
    resp.value = null;
    ElMessage.error(String(e));
  } finally {
    busy.value = false;
  }
}

/** JSON 自动缩进展示，其余按原文 */
const prettyBody = computed(() => {
  const text = resp.value?.body ?? "";
  if (!text) {
    return "";
  }
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
});

const shownHeaders = computed<HttpHeader[]>(() => {
  if (!resp.value) {
    return [];
  }
  return respTab.value === "response" ? resp.value.responseHeaders : resp.value.requestHeaders;
});

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

const bodyPh = computed(() =>
  bodyType.value === "json" ? bodyPhJson : t("toolbox.tools.http.bodyPhText"),
);

async function copyBody() {
  if (!prettyBody.value) {
    return;
  }
  await copyToClipboard(prettyBody.value);
  ElMessage.success(t("toolbox.tools.http.copied"));
}
</script>

<template>
  <div class="view-root">
    <div class="mb-2.5">
      <el-input
        v-model="url"
        size="small"
        :placeholder="t('toolbox.tools.http.urlPh')"
        class="[&_.el-input\_\_inner]:text-[11px] [&_.el-input\_\_inner]:font-mono [&_.el-input\_\_inner]:select-text"
        @keyup.enter="send"
      />
      <div class="mt-1.5 flex items-center gap-2">
        <el-select v-model="method" size="small" class="w-[92px] shrink-0">
          <el-option v-for="m in METHODS" :key="m" :label="m" :value="m" />
        </el-select>
        <span class="text-[10px] text-dp-tertiary truncate flex-1 min-w-0">
          {{ t("toolbox.tools.http.proxyHint") }}
        </span>
        <el-button
          size="small"
          type="primary"
          :icon="Promotion"
          :loading="busy"
          class="shrink-0"
          @click="send"
        >
          {{ t("toolbox.tools.http.send") }}
        </el-button>
      </div>
    </div>

    <el-scrollbar class="view-scroll">
      <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <el-radio-group v-model="reqTab" size="small">
          <el-radio-button value="params">
            {{ t("toolbox.tools.http.params") }}
            <template v-if="paramCount">({{ paramCount }})</template>
          </el-radio-button>
          <el-radio-button value="body">{{ t("toolbox.tools.http.body") }}</el-radio-button>
          <el-radio-button value="headers">
            {{ t("toolbox.tools.http.headers") }}
            <template v-if="headerCount">({{ headerCount }})</template>
          </el-radio-button>
        </el-radio-group>

        <div v-if="reqTab !== 'body'" class="mt-2 flex flex-col gap-1.5">
          <div
            v-for="row in activeRows"
            :key="row.uid"
            class="flex items-center gap-1.5 min-w-0"
          >
            <el-input
              v-model="row.name"
              size="small"
              placeholder="key"
              class="w-[36%] shrink-0 [&_.el-input\_\_inner]:text-[11px] [&_.el-input\_\_inner]:select-text"
            />
            <el-input
              v-model="row.value"
              size="small"
              placeholder="value"
              class="flex-1 min-w-0 [&_.el-input\_\_inner]:text-[11px] [&_.el-input\_\_inner]:select-text"
            />
            <el-button
              size="small"
              text
              :icon="Close"
              class="shrink-0"
              @click="removeRow(row.uid)"
            />
          </div>
          <el-button size="small" text type="primary" :icon="Plus" @click="addRow">
            {{ t("toolbox.tools.http.add") }}
          </el-button>
        </div>

        <div v-else class="mt-2 flex flex-col gap-1.5">
          <el-radio-group v-model="bodyType" size="small">
            <el-radio-button value="json">{{ t("toolbox.tools.http.bodyJson") }}</el-radio-button>
            <el-radio-button value="text">{{ t("toolbox.tools.http.bodyText") }}</el-radio-button>
          </el-radio-group>
          <el-input
            v-model="body"
            type="textarea"
            :rows="5"
            resize="none"
            :placeholder="bodyPh"
            class="select-text [&_.el-textarea\_\_inner]:text-[11px] [&_.el-textarea\_\_inner]:font-mono [&_.el-textarea\_\_inner]:select-text"
          />
          <div
            v-if="bodyless && body"
            class="text-[10px] leading-[1.6] text-[var(--el-color-warning,#e6a23c)]"
          >
            {{ t("toolbox.tools.http.bodyWarn") }}
          </div>
        </div>
      </div>

      <div
        v-if="resp"
        class="mt-2.5 p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]"
      >
        <div class="flex items-center gap-1.5 min-w-0">
          <span
            class="px-1.5 py-0.5 rounded-md text-[10.5px] font-semibold shrink-0"
            :class="statusClass(resp.status)"
          >
            {{ resp.status }}{{ resp.statusText ? ` ${resp.statusText}` : "" }}
          </span>
          <span class="text-[10px] text-dp-tertiary shrink-0">
            {{ resp.durationMs }} ms · {{ fmt(resp.size) }}
          </span>
          <el-button
            v-if="!resp.binary && prettyBody"
            size="small"
            text
            type="primary"
            :icon="DocumentCopy"
            class="ml-auto shrink-0"
            @click="copyBody"
          >
            {{ t("toolbox.tools.http.copyBody") }}
          </el-button>
        </div>
        <div v-if="resp.truncated" class="mt-1 text-[10px] text-[var(--el-color-warning,#e6a23c)]">
          {{ t("toolbox.tools.http.truncated") }}
        </div>

        <el-radio-group v-model="respTab" size="small" class="mt-2">
          <el-radio-button value="body">{{ t("toolbox.tools.http.respBody") }}</el-radio-button>
          <el-radio-button value="response">
            {{ t("toolbox.tools.http.respHeaders") }}
          </el-radio-button>
          <el-radio-button value="request">
            {{ t("toolbox.tools.http.reqHeaders") }}
          </el-radio-button>
        </el-radio-group>

        <div v-if="respTab === 'body'" class="mt-2">
          <div v-if="resp.binary" class="text-[11px] text-dp-tertiary">
            {{ t("toolbox.tools.http.binary") }}
          </div>
          <el-scrollbar v-else-if="prettyBody" max-height="220px" class="dp-scroll">
            <pre
              class="m-0 p-2 rounded-md bg-dp-panel-bg border border-dp-card-border text-[10.5px] leading-[1.55] font-mono whitespace-pre-wrap break-all text-dp-secondary"
              >{{ prettyBody }}</pre
            >
          </el-scrollbar>
          <div v-else class="text-[11px] text-dp-tertiary">
            {{ t("toolbox.tools.http.emptyValue") }}
          </div>
        </div>
        <div v-else class="mt-2 flex flex-col gap-1">
          <div
            v-for="(h, i) in shownHeaders"
            :key="`${h.name}-${i}`"
            class="flex gap-1.5 text-[10.5px] leading-[1.6] min-w-0"
          >
            <span class="text-dp-accent shrink-0 max-w-[46%] break-all">{{ h.name }}</span>
            <span class="text-dp-muted break-all flex-1 min-w-0">{{ h.value }}</span>
          </div>
          <div v-if="!shownHeaders.length" class="text-[11px] text-dp-tertiary">
            {{ t("toolbox.tools.http.emptyValue") }}
          </div>
        </div>

        <div class="mt-2 pt-1.5 border-t border-dp-divider text-[10px] leading-[1.6] text-dp-tertiary break-all">
          {{ t("toolbox.tools.http.finalUrl") }}: {{ resp.finalUrl }}
        </div>
      </div>
      <div v-else class="mt-2.5 px-2.5 py-3 text-center text-[10.5px] leading-[1.7] text-dp-tertiary">
        {{ busy ? t("toolbox.tools.http.sending") : t("toolbox.tools.http.hint") }}
      </div>
    </el-scrollbar>
  </div>
</template>
