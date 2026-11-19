<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Close, DocumentCopy, FolderOpened, Plus, Promotion } from "@element-plus/icons-vue";
import type { HttpAuth, HttpBodyType, HttpFormFile, HttpHeader, HttpRequestSpec } from "@/types";

const { t } = useI18n();

const props = defineProps<{
  /** 共享请求状态：子组件直接改嵌套字段（父级整体替换对象以触发重载） */
  spec: HttpRequestSpec;
  busy: boolean;
}>();

const emit = defineEmits<{
  send: [];
  stop: [];
  "copy-code": [kind: "curl" | "fetch" | "python"];
}>();

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/**
 * JSON 模式下的输入框占位示例。
 * 不能放进 i18n：花括号会被 vue-i18n 消息编译器当作插值语法，渲染时抛错导致整块 UI 卡住。
 */
const bodyPhJson = '{\n  "key": "value"\n}';

/* ---------- 键值行：uid 保证增删行时输入框不串行 ---------- */
interface KvRow {
  uid: number;
  name: string;
  value: string;
}
interface FileRow {
  uid: number;
  name: string;
  path: string;
}
let rowSeq = 0;
const toRows = (pairs?: HttpHeader[]): KvRow[] => (pairs ?? []).map((p) => ({ uid: ++rowSeq, name: p.name, value: p.value }));
const toFileRows = (files?: HttpFormFile[]): FileRow[] => (files ?? []).map((f) => ({ uid: ++rowSeq, name: f.name, path: f.path }));
const fromRows = (rows: KvRow[]): HttpHeader[] =>
  rows.filter((r) => r.name.trim()).map(({ name, value }) => ({ name, value }));

/* ---------- 本地编辑态 ---------- */
const paramsRows = ref<KvRow[]>([]);
const headersRows = ref<KvRow[]>([]);
const formRows = ref<KvRow[]>([]);
const fileRows = ref<FileRow[]>([]);
const bodyText = ref("");
const bodyType = ref<HttpBodyType>("json");
const auth = ref<HttpAuth>({ type: "none", token: "", username: "", password: "" });
/** 超时（秒）；undefined 表示用默认 30s */
const timeoutSec = ref<number | undefined>(undefined);
const reqTab = ref<"params" | "body" | "headers" | "auth">("params");

/** 父级整体替换 spec（载入收藏/历史/导入 cURL）时重建编辑态 */
watch(
  () => props.spec,
  (spec) => {
    paramsRows.value = toRows(spec.params);
    headersRows.value = toRows(spec.headers);
    formRows.value = toRows(spec.form);
    fileRows.value = toFileRows(spec.formFiles);
    bodyText.value = spec.body ?? "";
    bodyType.value = spec.bodyType ?? "json";
    auth.value = {
      type: spec.auth?.type ?? "none",
      key: "",
      value: "",
      in: "header",
      ...spec.auth,
    };
    timeoutSec.value = spec.timeoutMs ? spec.timeoutMs / 1000 : undefined;
    reqTab.value = "params";
  },
  { immediate: true },
);

/* 本地编辑写回共享 spec（引用不变，父级 watch 不会触发）。
   深度 watch 覆盖 v-model 键入与所有控件变更，发送时 spec 必然是最新的 */
watch(
  [paramsRows, headersRows, formRows, fileRows, bodyText, bodyType, auth, timeoutSec],
  () => {
    const spec = props.spec;
    spec.params = fromRows(paramsRows.value);
    spec.headers = fromRows(headersRows.value);
    spec.form = fromRows(formRows.value);
    spec.formFiles = fileRows.value
      .filter((r) => r.name.trim() && r.path.trim())
      .map(({ name, path }) => ({ name, path }));
    spec.body = bodyText.value;
    spec.bodyType = bodyType.value;
    spec.auth = { ...auth.value };
    spec.timeoutMs = timeoutSec.value && timeoutSec.value > 0 ? Math.round(timeoutSec.value * 1000) : undefined;
  },
  { deep: true },
);

const activeRows = computed(() => {
  if (reqTab.value === "params") {
    return paramsRows.value;
  }
  if (reqTab.value === "headers") {
    return headersRows.value;
  }
  return formRows.value;
});

function addRow() {
  activeRows.value.push({ uid: ++rowSeq, name: "", value: "" });
}

function removeRow(uid: number) {
  const rows = activeRows.value;
  const i = rows.findIndex((r) => r.uid === uid);
  if (i >= 0) {
    rows.splice(i, 1);
  }
}

const paramCount = computed(() => paramsRows.value.filter((r) => r.name.trim()).length);
const headerCount = computed(() => headersRows.value.filter((r) => r.name.trim()).length);

/** GET / HEAD 常不带体，但代理不拦截：填了就照原样发送 */
const bodyless = computed(() => props.spec.method === "GET" || props.spec.method === "HEAD");
const bodyPh = computed(() => (bodyType.value === "json" ? bodyPhJson : t("toolbox.tools.http.bodyPhText")));

/* ---------- form-data 文件 ---------- */
async function pickFile() {
  let path: string | null = null;
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, directory: false });
    if (!picked || Array.isArray(picked)) {
      return;
    }
    path = picked;
  } catch {
    // 无对话框环境（如浏览器调试）直接忽略
    return;
  }
  const fileName = path.split(/[\\/]/).pop() ?? path;
  fileRows.value.push({ uid: ++rowSeq, name: fileName, path });
}

const methodClass = computed(() => {
  switch (props.spec.method) {
    case "GET":
      return "text-[var(--el-color-success,#67c23a)]";
    case "POST":
      return "text-[var(--el-color-primary,#409eff)]";
    case "PUT":
      return "text-[var(--el-color-warning,#e6a23c)]";
    case "DELETE":
      return "text-[var(--el-color-danger,#f56c6c)]";
    default:
      return "text-dp-muted";
  }
});
</script>

<template>
  <section class="flex flex-col min-h-0 min-w-0 gap-2.5">
    <!-- 方法 / 地址 / 发送 -->
    <div class="flex items-center gap-2">
      <el-select
        v-model="spec.method"
        size="default"
        class="w-[108px] shrink-0"
      >
        <el-option v-for="m in METHODS" :key="m" :label="m" :value="m">
          <span class="text-[12px] font-semibold" :class="m === spec.method ? methodClass : 'text-dp-secondary'">
            {{ m }}
          </span>
        </el-option>
      </el-select>
      <el-input
        v-model="spec.url"
        size="default"
        :placeholder="t('toolbox.tools.http.urlPh')"
        class="flex-1 min-w-0 [&_.el-input\_\_inner]:font-mono [&_.el-input\_\_inner]:select-text"
        @keyup.enter="emit('send')"
      />
        <el-button
          v-if="!busy"
          size="default"
          type="primary"
          :icon="Promotion"
          class="shrink-0"
          @click="emit('send')"
        >
          {{ t("toolbox.tools.http.send") }}
        </el-button>
        <el-button
          v-else
          size="default"
          type="danger"
          :icon="Close"
          class="shrink-0"
          @click="emit('stop')"
        >
          {{ t("toolbox.tools.http.stop") }}
        </el-button>
      <el-dropdown trigger="click" placement="bottom-end" @command="emit('copy-code', $event)">
        <el-tooltip :content="t('toolbox.tools.http.copyAs')" placement="bottom">
          <el-button size="default" text :icon="DocumentCopy" class="shrink-0" />
        </el-tooltip>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="curl">cURL</el-dropdown-item>
            <el-dropdown-item command="fetch">fetch (JS)</el-dropdown-item>
            <el-dropdown-item command="python">Python requests</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>

    <!-- 编辑区卡片 -->
    <div class="flex-1 min-h-0 flex flex-col p-3 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
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
        <el-radio-button value="auth">{{ t("toolbox.tools.http.options") }}</el-radio-button>
      </el-radio-group>

      <!-- Params / Headers / Form 文本字段：键值行编辑器 -->
      <el-scrollbar v-if="reqTab !== 'body' && reqTab !== 'auth'" class="flex-1 min-h-0 mt-2.5 dp-scroll">
        <div class="flex flex-col gap-2 pr-1">
          <div v-for="row in activeRows" :key="row.uid" class="flex items-center gap-1.5 min-w-0">
            <el-input
              v-model="row.name"
              placeholder="key"
              class="w-[38%] shrink-0 [&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
            <el-input
              v-model="row.value"
              placeholder="value"
              class="flex-1 min-w-0 [&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
            <el-button size="small" text :icon="Close" class="shrink-0" @click="removeRow(row.uid)" />
          </div>
          <el-button size="small" text type="primary" :icon="Plus" @click="addRow">
            {{ t("toolbox.tools.http.add") }}
          </el-button>
        </div>
      </el-scrollbar>

      <!-- Body -->
      <template v-else-if="reqTab === 'body'">
        <el-radio-group v-model="bodyType" size="small" class="mt-2.5">
          <el-radio-button value="json">{{ t("toolbox.tools.http.bodyJson") }}</el-radio-button>
          <el-radio-button value="text">{{ t("toolbox.tools.http.bodyText") }}</el-radio-button>
          <el-radio-button value="form-urlencoded">{{ t("toolbox.tools.http.bodyFormUrl") }}</el-radio-button>
          <el-radio-button value="form-data">{{ t("toolbox.tools.http.bodyFormData") }}</el-radio-button>
        </el-radio-group>

        <!-- json / text：文本域 -->
        <div v-if="bodyType === 'json' || bodyType === 'text'" class="flex-1 min-h-0 mt-2 flex flex-col">
          <el-input
            v-model="bodyText"
            type="textarea"
            resize="none"
            :placeholder="bodyPh"
            class="flex-1 min-h-0 [&_.el-textarea\_\_inner]:h-full [&_.el-textarea\_\_inner]:text-[12px] [&_.el-textarea\_\_inner]:font-mono [&_.el-textarea\_\_inner]:leading-[1.6] [&_.el-textarea\_\_inner]:select-text"
          />
          <div
            v-if="bodyless && bodyText"
            class="shrink-0 mt-1.5 text-[11px] leading-[1.6] text-[var(--el-color-warning,#e6a23c)]"
          >
            {{ t("toolbox.tools.http.bodyWarn") }}
          </div>
        </div>

        <!-- form-urlencoded / form-data：字段行 + 文件行 -->
        <el-scrollbar v-else class="flex-1 min-h-0 mt-2.5 dp-scroll">
          <div class="flex flex-col gap-2 pr-1">
            <div v-for="row in formRows" :key="row.uid" class="flex items-center gap-1.5 min-w-0">
              <el-input
                v-model="row.name"
                placeholder="field"
                class="w-[38%] shrink-0 [&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
              />
              <el-input
                v-model="row.value"
                placeholder="value"
                class="flex-1 min-w-0 [&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
              />
              <el-button size="small" text :icon="Close" class="shrink-0" @click="removeRow(row.uid)" />
            </div>
            <div v-for="row in fileRows" :key="row.uid" class="flex items-center gap-1.5 min-w-0">
              <el-input
                v-model="row.name"
                placeholder="field"
                class="w-[38%] shrink-0 [&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
              />
              <span
                class="flex-1 min-w-0 h-[24px] px-1.5 flex items-center rounded-[4px] bg-dp-panel-bg border border-dp-card-border text-[11px] font-mono text-dp-muted truncate select-text"
                :title="row.path"
              >
                {{ row.path }}
              </span>
              <el-button
                size="small"
                text
                :icon="Close"
                class="shrink-0"
                @click="fileRows.splice(fileRows.findIndex((r) => r.uid === row.uid), 1)"
              />
            </div>
            <div class="flex items-center gap-3">
              <el-button size="small" text type="primary" :icon="Plus" @click="addRow">
                {{ t("toolbox.tools.http.add") }}
              </el-button>
              <el-button size="small" text type="primary" :icon="FolderOpened" @click="pickFile">
                {{ t("toolbox.tools.http.addFile") }}
              </el-button>
            </div>
          </div>
        </el-scrollbar>
      </template>

      <!-- 选项：认证 + 超时 -->
      <el-scrollbar v-else class="flex-1 min-h-0 mt-2.5 dp-scroll">
        <div class="flex flex-col gap-2.5 pr-1">
          <div class="text-[11px] font-medium text-dp-muted">{{ t("toolbox.tools.http.auth") }}</div>
          <el-radio-group v-model="auth.type" size="small">
            <el-radio-button value="none">{{ t("toolbox.tools.http.authNone") }}</el-radio-button>
            <el-radio-button value="bearer">{{ t("toolbox.tools.http.authBearer") }}</el-radio-button>
            <el-radio-button value="basic">{{ t("toolbox.tools.http.authBasic") }}</el-radio-button>
            <el-radio-button value="apikey">{{ t("toolbox.tools.http.authApiKey") }}</el-radio-button>
          </el-radio-group>
          <el-input
            v-if="auth.type === 'bearer'"
            v-model="auth.token"
            type="password"
            show-password
            :placeholder="t('toolbox.tools.http.tokenPh')"
            class="[&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
          />
          <template v-if="auth.type === 'basic'">
            <el-input
              v-model="auth.username"
              :placeholder="t('toolbox.tools.http.usernamePh')"
              class="[&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
            <el-input
              v-model="auth.password"
              type="password"
              show-password
              :placeholder="t('toolbox.tools.http.passwordPh')"
              class="[&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
          </template>
          <template v-if="auth.type === 'apikey'">
            <el-radio-group v-model="auth.in" size="small">
              <el-radio-button value="header">header</el-radio-button>
              <el-radio-button value="query">query</el-radio-button>
            </el-radio-group>
            <el-input
              v-model="auth.key"
              placeholder="X-API-Key"
              class="[&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
            <el-input
              v-model="auth.value"
              type="password"
              show-password
              :placeholder="t('toolbox.tools.http.keyValuePh')"
              class="[&_.el-input\_\_inner]:text-[12px] [&_.el-input\_\_inner]:select-text"
            />
          </template>
          <div v-if="auth.type !== 'none'" class="text-[11px] leading-[1.6] text-dp-tertiary">
            {{ t("toolbox.tools.http.authHint") }}
          </div>

          <div class="mt-1 h-px bg-dp-divider" />
          <div class="text-[11px] font-medium text-dp-muted">{{ t("toolbox.tools.http.timeoutLabel") }}</div>
          <div class="flex items-center gap-2">
            <el-input-number
              v-model="timeoutSec"
              :min="1"
              :max="600"
              size="small"
              :placeholder="'30'"
              controls-position="right"
              class="w-[110px]"
            />
            <span class="text-[11px] text-dp-tertiary">{{ t("toolbox.tools.http.timeoutHint") }}</span>
          </div>
        </div>
      </el-scrollbar>
    </div>
  </section>
</template>
