<script setup lang="ts">
import { computed, h, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  ArrowRight,
  CircleCheck,
  CircleClose,
  Connection,
  CopyDocument,
  Delete,
  Edit,
  Loading,
  Plus,
  Position,
  VideoPause,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { useConfigStore } from "@/stores/config";
import { AiApiError, isModelReady, streamChatCompletion, type StreamChatResult } from "@/api/ai";
import { launchItem } from "@/api/launcher";
import { copyToClipboard } from "@/api/clipboard";
import { mcpConnect, mcpDisconnect, mcpServerStatus } from "@/api/mcp";
import { renderMarkdown } from "@/lib/md";
import { SYSTEM_PROMPT, collectTools, type AggregatedTools, type ToolExecResult } from "@/lib/ai-tools";
import { getChatMessages, addChatMessage, deleteChatMessage, clearChatMessages } from "@/api/chat";
import type { AiModel, ChatEntry, ChatMessage, McpServerConfig, ToolCall } from "@/types";

const { t } = useI18n();
const config = useConfigStore();

const models = computed<AiModel[]>(() => config.settings.ai_models ?? []);
const activeId = computed<string | null>(() => config.settings.ai_active ?? null);
const activeModel = computed<AiModel | null>(
  () => models.value.find((m) => m.id === activeId.value) ?? null
);

/** 已入库的聊天记录（SQLite）。 */
const messages = ref<ChatEntry[]>([]);
/** 流式输出中的 assistant 占位（负数 id，未入库）。 */
const pending = ref<ChatEntry | null>(null);
/** 工具结果消息不单独成气泡，挂在 assistant 气泡的工具 chips 上。 */
const rendered = computed<ChatEntry[]>(() => {
  const list = messages.value.filter((m) => m.role !== "tool");
  return pending.value ? [...list, pending.value] : list;
});

const input = ref("");
const streaming = ref(false);
let abortController: AbortController | null = null;

/** agent 循环上限：超出后强制无工具收尾，防止死循环。 */
const MAX_TOOL_ROUNDS = 8;
/** 请求历史窗口：只回传最近若干条，避免无限增长。 */
const HISTORY_WINDOW = 60;
/** 当前会话模型是否不支持 function-calling（首次 4xx 后置位并降级为普通对话）。 */
let toolsUnsupported = false;

const listRef = ref<{ wrapRef: HTMLElement } | null>(null);

/** 滚动到消息底部。 */
function scrollToEnd() {
  // 双 nextTick + rAF:等视图切换过渡与消息内容(代码块/图片)布局完成后再定位,
  // 否则进入标签页时会停在顶部而不是最新消息
  void nextTick(() => {
    void nextTick(() => {
      requestAnimationFrame(() => {
        const wrap = listRef.value?.wrapRef;
        if (wrap) {
          wrap.scrollTop = wrap.scrollHeight;
        }
      });
    });
  });
}

watch(rendered, scrollToEnd, { deep: true });
watch(
  () => models.value.length,
  () => {
    if (!activeModel.value && models.value.length > 0) {
      void config.setActiveAiModel(models.value[0].id);
    }
  },
  { immediate: true }
);

onMounted(async () => {
  try {
    messages.value = await getChatMessages();
  } catch (err) {
    console.error("[desk-pet] 聊天记录加载失败:", err);
  }
  scrollToEnd();
  // 延时补一次:消息里的图片/代码块晚于首帧渲染会撑高内容,把定位重新压回底部
  window.setTimeout(scrollToEnd, 180);
});

/** 把流式完成的回复写入 SQLite；失败时提示（内容已无法恢复）。 */
async function saveAssistant(content: string): Promise<void> {
  try {
    messages.value.push(await addChatMessage("assistant", content));
  } catch (err) {
    console.error("[desk-pet] 保存回复失败:", err);
    ElMessage.error(t("chat.saveFail"));
  }
}

/** 组装请求历史：system prompt + 最近窗口内的持久化消息（修复被删出孤儿工具消息的记录）。 */
function buildHistory(): ChatMessage[] {
  const recent = messages.value.slice(-HISTORY_WINDOW);
  const callIds = new Set<string>();
  for (const m of recent) {
    if (m.role === "assistant" && m.tool_calls) {
      for (const call of toolCallsOf(m)) {
        callIds.add(call.id);
      }
    }
  }
  const history: ChatMessage[] = [{ role: "system", content: SYSTEM_PROMPT }];
  for (const m of recent) {
    if (m.role === "tool") {
      // tool 结果必须紧跟对应的 assistant.tool_calls；配对被删则整条跳过，避免 API 报错
      if (m.tool_call_id && callIds.has(m.tool_call_id)) {
        history.push({ role: "tool", content: m.content, tool_call_id: m.tool_call_id });
      }
    } else if (m.role === "assistant" && m.tool_calls) {
      const calls = toolCallsOf(m);
      // assistant 的 tool_calls 必须有全部配对的 tool 结果，否则降级为纯文本
      const complete =
        calls.length > 0 && calls.every((c) => recent.some((t) => t.role === "tool" && t.tool_call_id === c.id));
      if (complete) {
        history.push({ role: "assistant", content: m.content, tool_calls: calls });
      } else {
        history.push({ role: "assistant", content: m.content });
      }
    } else {
      history.push({ role: m.role, content: m.content });
    }
  }
  return history;
}

/** 执行一轮工具调用：逐个执行并持久化结果（role:"tool"），返回是否全部成功落库。 */
async function runToolCalls(
  calls: ToolCall[],
  aggregated: AggregatedTools
): Promise<void> {
  for (const call of calls) {
    let result: ToolExecResult;
    if (abortController?.signal.aborted) {
      result = { ok: false, text: "已中断 Interrupted" };
    } else {
      const executor = aggregated.executors.get(call.function.name);
      let args: Record<string, unknown> = {};
      try {
        args = JSON.parse(call.function.arguments || "{}") as Record<string, unknown>;
      } catch {
        // 参数不合法时按空对象执行，让工具自己返回参数错误
      }
      try {
        result = executor
          ? await executor(args)
          : { ok: false, text: `未知工具 Unknown tool: ${call.function.name}` };
      } catch (err) {
        result = { ok: false, text: `工具执行异常 Tool error: ${String(err)}` };
      }
    }
    try {
      messages.value.push(await addChatMessage("tool", JSON.stringify(result), undefined, call.id));
    } catch (err) {
      console.error("[desk-pet] 保存工具结果失败:", err);
    }
  }
}

async function send() {
  const text = input.value.trim();
  if (!text || streaming.value) {
    return;
  }
  const model = activeModel.value;
  if (!model) {
    ElMessage.warning(t("chat.noModel"));
    showModels.value = true;
    return;
  }
  if (!isModelReady(model)) {
    ElMessage.warning(t("chat.modelIncomplete"));
    showModels.value = true;
    return;
  }
  let userMsg: ChatEntry;
  try {
    userMsg = await addChatMessage("user", text);
  } catch (err) {
    console.error("[desk-pet] 保存消息失败:", err);
    ElMessage.error(t("chat.saveFail"));
    return;
  }
  input.value = "";
  messages.value.push(userMsg);
  streaming.value = true;
  abortController = new AbortController();
  try {
    const aggregated = toolsUnsupported ? null : await collectTools();
    for (let round = 0; ; round += 1) {
      // 轮次用尽后不再提供工具，强制模型给出文字结论
      const allowTools = aggregated !== null && round < MAX_TOOL_ROUNDS;
      pending.value = { id: -1, role: "assistant", content: "", created_at: 0 };
      let result: StreamChatResult;
      try {
        result = await streamChatCompletion({
          model,
          messages: buildHistory(),
          tools: allowTools && aggregated ? aggregated.definitions : undefined,
          signal: abortController.signal,
          onDelta: (delta) => {
            if (pending.value) {
              pending.value.content += delta;
            }
          },
        });
      } catch (err) {
        // 部分 OpenAI 兼容服务不支持 tools 参数（400/404/422 等）：降级重试，本次会话不再携带
        if (
          aggregated &&
          !toolsUnsupported &&
          err instanceof AiApiError &&
          err.status >= 400 &&
          err.status < 500
        ) {
          toolsUnsupported = true;
          ElMessage.info(t("chat.toolsUnsupported"));
          continue;
        }
        throw err;
      }
      if (!allowTools || result.toolCalls.length === 0) {
        const content = result.content;
        if (content) {
          await saveAssistant(content);
        }
        return;
      }
      // 工具轮：持久化带 tool_calls 的 assistant 消息（气泡上显示工具 chips）
      const assistantMsg = await addChatMessage("assistant", result.content, result.toolCalls);
      messages.value.push(assistantMsg);
      pending.value = null;
      await runToolCalls(result.toolCalls, aggregated);
      // 用户在工具执行期间点了停止：保留已产生的记录，结束本轮对话
      if (abortController?.signal.aborted) {
        return;
      }
    }
  } catch (err) {
    const partial = pending.value?.content ?? "";
    const msg = abortController?.signal.aborted
      ? t("chat.stopped")
      : requestFailText(err, model);
    if (partial) {
      await saveAssistant(`${partial}\n\n(${msg})`);
    } else if (abortController?.signal.aborted) {
      ElMessage.error(msg);
    } else {
      showRequestError(msg);
    }
  } finally {
    pending.value = null;
    streaming.value = false;
    abortController = null;
  }
}

function stop() {
  abortController?.abort();
}

/**
 * 请求失败提示：toast 内嵌"复制"按钮，一键复制完整错误文本（便于反馈/排查）。
 * 错误文本可能很长，须 break-all 且给足停留时间。
 */
function showRequestError(text: string) {
  ElMessage({
    type: "error",
    showClose: true,
    duration: 6000,
    message: h("div", { class: "flex items-start gap-2.5" }, [
      h("div", { class: "min-w-0 text-[12px] leading-[1.5] break-all" }, text),
      h(
        "span",
        {
          class:
            "shrink-0 self-center cursor-pointer select-none text-[12px] font-medium underline underline-offset-2",
          onClick: () => {
            void copyToClipboard(text)
              .then(() => ElMessage.success(t("common.copied")))
              .catch((err: unknown) =>
                ElMessage.error(t("common.copyFail", { err: String(err) }))
              );
          },
        },
        t("common.copy")
      ),
    ]),
  });
}

/** Enter 发送；中文输入法选词确认（isComposing）不触发。 */
function onEnterKey(e: KeyboardEvent) {
  if (e.isComposing) {
    return;
  }
  void send();
}

/** 点击 markdown 内容里的链接：仅放行 http/https/mailto，用系统浏览器打开，不让面板页面跳走。 */
function onLinkClick(e: MouseEvent) {
  const anchor = (e.target as HTMLElement | null)?.closest?.("a");
  if (!(anchor instanceof HTMLAnchorElement)) {
    return;
  }
  e.preventDefault();
  const href = anchor.getAttribute("href") ?? "";
  if (!/^(https?:\/\/|mailto:)/i.test(href)) {
    return;
  }
  void launchItem({
    id: "md-link",
    name: href,
    icon: "",
    item_type: "url",
    value: href,
    color: "",
  }).catch(() => undefined);
}

/** ---------- 工具调用 chips ---------- */

/** 解析 assistant 消息持久化的 tool_calls JSON。 */
function toolCallsOf(msg: ChatEntry): ToolCall[] {
  if (!msg.tool_calls) {
    return [];
  }
  try {
    return JSON.parse(msg.tool_calls) as ToolCall[];
  } catch {
    return [];
  }
}

/** 查找 tool 消息的执行结果（找不到视为执行中）。 */
function toolResultOf(callId: string): ToolExecResult | null {
  const m = messages.value.find((t) => t.role === "tool" && t.tool_call_id === callId);
  if (!m) {
    return null;
  }
  try {
    return JSON.parse(m.content) as ToolExecResult;
  } catch {
    return { ok: false, text: m.content };
  }
}

/** chips 展示名：mcp__<server>__<tool> 显示为 "server · tool"。 */
function toolDisplayName(name: string): string {
  const parts = name.split("__");
  if (parts[0] === "mcp" && parts.length >= 3) {
    return `${parts[1]} · ${parts.slice(2).join("__")}`;
  }
  return name;
}

/** chip 悬浮提示：完整参数 JSON（截断）。 */
function toolArgsTitle(call: ToolCall): string {
  return call.function.arguments.slice(0, 300);
}

/** 删除单条聊天记录。 */
async function removeMessage(msg: ChatEntry) {
  if (msg.id <= 0) {
    return;
  }
  try {
    await deleteChatMessage(msg.id);
    messages.value = messages.value.filter((m) => m.id !== msg.id);
  } catch (err) {
    console.error("[desk-pet] 删除消息失败:", err);
    ElMessage.error(t("chat.deleteFail"));
  }
}

/** 复制单条消息内容到剪贴板。 */
async function copyMessage(msg: ChatEntry) {
  const content = msg.content.trim();
  if (!content) {
    return;
  }
  try {
    await copyToClipboard(content);
    ElMessage.success(t("common.copied"));
  } catch (err) {
    console.error("[desk-pet] 复制消息失败:", err);
    ElMessage.error(t("common.copyFail", { err: String(err) }));
  }
}

async function clearHistory() {
  if (rendered.value.length === 0) {
    return;
  }
  try {
    await ElMessageBox.confirm(t("chat.clearTitle"), t("chat.title"), {
      confirmButtonText: t("common.confirm"),
      cancelButtonText: t("common.cancel"),
      type: "warning",
    });
  } catch {
    return;
  }
  try {
    await clearChatMessages();
    messages.value = [];
    ElMessage.success(t("chat.cleared"));
  } catch (err) {
    console.error("[desk-pet] 清空聊天失败:", err);
    ElMessage.error(t("chat.deleteFail"));
  }
}

/** ---------- 模型管理 ---------- */
const showModels = ref(false);
const editing = ref<AiModel | null>(null);
const form = ref<Omit<AiModel, "id">>({ name: "", base_url: "", api_key: "", model: "" });

function openAdd() {
  editing.value = null;
  form.value = { name: "", base_url: "", api_key: "", model: "" };
}

/** base_url 未带协议时补 http://（不得使用 https://，会按域名解析失败）。 */
function normalizeBaseUrl(url: string): string {
  const trimmed = url.replace(/^([a-zA-Z][a-zA-Z0-9+.-]*)\/\//, "$1://");
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
}

/** 本地服务 HTTPS 连不上时的针对性提示（LM Studio、Ollama 等本地服务仅提供 HTTP）。 */
function requestFailText(err: unknown, model: AiModel): string {
  const raw = err instanceof Error ? err.message : String(err);
  const isLocal = /[/:](localhost|127\.0\.0\.1|\[::1\])/i.test(model.base_url);
  if (isLocal && /^https:\/\//i.test(model.base_url) && raw.includes("error sending request")) {
    return `${t("chat.requestFail")}: ${raw} — ${t("chat.localHttpsHint", {
      url: model.base_url.replace(/^https:\/\//i, "http://"),
    })}`;
  }
  return `${t("chat.requestFail")}: ${raw}`;
}

function openEdit(model: AiModel) {
  editing.value = model;
  form.value = { name: model.name, base_url: model.base_url, api_key: model.api_key, model: model.model };
}

async function saveModel() {
  const f = form.value;
  if (!f.base_url.trim() || !f.model.trim()) {
    ElMessage.warning(t("chat.emptyWarn"));
    return;
  }
  const payload: Omit<AiModel, "id"> = {
    name: f.name.trim() || f.model.trim(),
    // 未写协议时默认按 http:// 处理（本地服务基本为 HTTP，少敲一个前缀）
    base_url: normalizeBaseUrl(f.base_url.trim()),
    api_key: f.api_key.trim(),
    model: f.model.trim(),
  };
  if (editing.value) {
    await config.updateAiModel({ ...payload, id: editing.value.id });
  } else {
    await config.addAiModel(payload);
  }
  ElMessage.success(editing.value ? t("common.saved") : t("common.added"));
  showModels.value = false;
}

async function removeModel(model: AiModel) {
  await config.removeAiModel(model.id);
  if (editing.value?.id === model.id) {
    editing.value = null;
    showModels.value = false;
  }
}

/** ---------- MCP 服务器管理 ---------- */
const showMcp = ref(false);
const mcpServers = computed<McpServerConfig[]>(() => config.settings.mcp_servers ?? []);
/** 已连接的服务器 id（状态点） */
const mcpConnectedIds = ref<Set<string>>(new Set());
const mcpConnectingId = ref<string | null>(null);
const mcpEditing = ref<McpServerConfig | null>(null);
const mcpForm = ref({
  name: "",
  transport: "stdio" as "stdio" | "http",
  command: "",
  args: "",
  env: "",
  url: "",
});

watch(showMcp, (open) => {
  if (open) {
    void refreshMcpStatus();
  }
});

async function refreshMcpStatus() {
  try {
    mcpConnectedIds.value = new Set(await mcpServerStatus());
  } catch (err) {
    console.error("[desk-pet] MCP 状态查询失败:", err);
  }
}

function mcpStatus(server: McpServerConfig): "connected" | "connecting" | "off" {
  if (mcpConnectingId.value === server.id) {
    return "connecting";
  }
  return mcpConnectedIds.value.has(server.id) ? "connected" : "off";
}

function mcpStatusText(server: McpServerConfig): string {
  const status = mcpStatus(server);
  return t(
    status === "connected"
      ? "chat.mcpStatusOn"
      : status === "connecting"
        ? "chat.mcpStatusConnecting"
        : "chat.mcpStatusOff"
  );
}

function openMcpAdd() {
  mcpEditing.value = null;
  mcpForm.value = { name: "", transport: "stdio", command: "", args: "", env: "", url: "" };
}

function openMcpEdit(server: McpServerConfig) {
  mcpEditing.value = server;
  mcpForm.value = {
    name: server.name,
    transport: server.transport === "http" ? "http" : "stdio",
    command: server.command ?? "",
    args: (server.args ?? []).join(" "),
    env: Object.entries(server.env ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
    url: server.url ?? "",
  };
}

/** 环境变量文本（每行 KEY=value）解析为对象。 */
function parseEnvText(text: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of text.split(/\n+/)) {
    const idx = line.indexOf("=");
    if (idx > 0) {
      env[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    }
  }
  return env;
}

async function saveMcpServer() {
  const f = mcpForm.value;
  if (!f.name.trim()) {
    ElMessage.warning(t("chat.mcpNameRequired"));
    return;
  }
  if (f.transport === "http" && !f.url.trim()) {
    ElMessage.warning(t("chat.mcpUrlRequired"));
    return;
  }
  const payload = {
    name: f.name.trim(),
    transport: f.transport,
    command: f.transport === "stdio" ? f.command.trim() || null : null,
    args: f.transport === "stdio" ? f.args.trim().split(/\s+/).filter(Boolean) : [],
    env: f.transport === "stdio" ? parseEnvText(f.env) : {},
    url: f.transport === "http" ? f.url.trim() || null : null,
  };
  if (mcpEditing.value) {
    await config.updateMcpServer({ ...payload, id: mcpEditing.value.id, enabled: mcpEditing.value.enabled ?? true });
    ElMessage.success(t("common.saved"));
  } else {
    const id = await config.addMcpServer(payload);
    ElMessage.success(t("common.added"));
    openMcpAdd();
    // 新增后立即尝试连接，让用户马上看到状态
    void connectMcp(id);
  }
}

async function connectMcp(serverId: string) {
  mcpConnectingId.value = serverId;
  try {
    await mcpConnect(serverId);
    await refreshMcpStatus();
    ElMessage.success(t("chat.mcpConnectOk"));
  } catch (err) {
    ElMessage.error(`${t("chat.mcpConnectFail")}: ${String(err)}`);
  } finally {
    mcpConnectingId.value = null;
  }
}

async function disconnectMcp(serverId: string) {
  try {
    await mcpDisconnect(serverId);
  } catch (err) {
    console.error("[desk-pet] MCP 断开失败:", err);
  }
  await refreshMcpStatus();
}

async function toggleMcpEnabled(server: McpServerConfig, enabled: boolean) {
  await config.setMcpServerEnabled(server.id, enabled);
  if (enabled) {
    void connectMcp(server.id);
  } else {
    void disconnectMcp(server.id);
  }
}

async function removeMcpServer(server: McpServerConfig) {
  try {
    await ElMessageBox.confirm(t("chat.mcpDeleteConfirm", { name: server.name }), t("chat.mcpManager"), {
      confirmButtonText: t("common.confirm"),
      cancelButtonText: t("common.cancel"),
      type: "warning",
    });
  } catch {
    return;
  }
  void disconnectMcp(server.id);
  await config.removeMcpServer(server.id);
  if (mcpEditing.value?.id === server.id) {
    openMcpAdd();
  }
}

/** ---------- Vortex 推广位（AI API 中转服务） ---------- */
const VORTEX_URL = "https://ht-shaipe.github.io/vortex/"; // 官网落地页，点击推广卡即在系统浏览器打开
const VORTEX_API_URL = ""; // 中转接口地址，补充后显示"一键填入"按钮

/** 系统浏览器打开 Vortex 官网。 */
function openVortex() {
  if (!VORTEX_URL) {
    return;
  }
  void launchItem({
    id: "vortex-ad",
    name: "Vortex",
    icon: "",
    item_type: "url",
    value: VORTEX_URL,
    color: "",
  }).catch(() => undefined);
}

/** 一键填入 Vortex 中转接口地址（名称为空时顺便预填）。 */
function fillVortex() {
  form.value.base_url = VORTEX_API_URL;
  if (!form.value.name.trim()) {
    form.value.name = "Vortex";
  }
  ElMessage.success(t("chat.vortexFilled"));
}
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("chat.title") }}</span>
      <!-- 按钮组收紧间距：覆盖 EP 默认的 12px 相邻 margin 与 text 按钮左右内边距 -->
      <div class="inline-flex items-center gap-1 [&_.el-button+.el-button]:ml-0 [&_.el-button]:px-1">
        <el-select
          v-if="models.length > 0"
          :model-value="activeId"
          size="small"
          class="w-[120px]"
          @update:model-value="(v: string) => config.setActiveAiModel(v)"
        >
          <el-option v-for="m in models" :key="m.id" :value="m.id" :label="m.name || m.model" />
        </el-select>
        <el-button size="small" text type="primary" :icon="Plus" @click="openAdd(); showModels = true">
          {{ t("chat.addModel") }}
        </el-button>
        <el-button
          size="small"
          text
          :icon="Connection"
          :title="t('chat.mcpManager')"
          @click="showMcp = true"
        />
        <el-button size="small" text :icon="Delete" :title="t('chat.clear')" @click="clearHistory" />
      </div>
    </div>

    <div class="flex-1 min-h-0 flex flex-col">
      <el-empty
        v-if="models.length === 0"
        :description="t('chat.emptyModels')"
        :image-size="60"
        class="m-auto"
      >
        <el-button type="primary" size="small" :icon="Plus" @click="openAdd(); showModels = true">
          {{ t("chat.addModel") }}
        </el-button>
      </el-empty>
      <el-empty
        v-else-if="rendered.length === 0"
        :description="t('chat.emptyMessages')"
        :image-size="60"
        class="m-auto"
      />
      <el-scrollbar v-else ref="listRef" class="view-scroll pr-1">
        <div
          v-for="msg in rendered"
          :key="msg.id"
          class="msg-row flex mb-2.5"
          :class="msg.role === 'user' ? 'justify-end' : 'justify-start'"
        >
          <div
            class="max-w-[82%] px-[11px] py-[7px] rounded-xl text-[13px] leading-[1.55] [word-break:break-word]"
            :class="[
              msg.role === 'user'
                ? 'bg-dp-accent text-white rounded-br-[4px]'
                : 'bg-[color-mix(in_srgb,var(--dp-accent)_12%,var(--dp-card-bg))] text-dp-primary rounded-bl-[4px]',
              msg.role !== 'user' && pending && msg.id < 0 ? 'chat-md-cursor' : '',
            ]"
            @click="msg.role === 'user' ? undefined : onLinkClick($event)"
          >
            <!-- 工具调用 chips：assistant 气泡顶部，执行中有结果后原地变为成功/失败 -->
            <div
              v-if="msg.role !== 'user' && toolCallsOf(msg).length > 0"
              class="mb-1.5 flex flex-wrap gap-1"
            >
              <span
                v-for="call in toolCallsOf(msg)"
                :key="call.id"
                class="inline-flex max-w-full items-center gap-1 rounded-full bg-dp-panel-bg border border-dp-card-border py-px pl-2 pr-1.5 text-[10.5px] leading-[1.5] text-dp-secondary"
                :title="toolArgsTitle(call)"
              >
                <el-icon
                  :size="11"
                  class="shrink-0"
                  :class="toolResultOf(call.id) ? (toolResultOf(call.id)!.ok ? 'text-[#67c23a]' : 'text-[#f56c6c]') : 'animate-spin text-dp-tertiary'"
                >
                  <CircleCheck v-if="toolResultOf(call.id)?.ok" />
                  <CircleClose v-else-if="toolResultOf(call.id)" />
                  <Loading v-else />
                </el-icon>
                <span class="truncate font-mono">{{ toolDisplayName(call.function.name) }}</span>
              </span>
            </div>
            <span v-if="msg.role === 'user'" class="whitespace-pre-wrap">{{ msg.content }}</span>
            <div
              v-else-if="msg.content"
              class="chat-md-body"
              v-html="renderMarkdown(msg.content)"
            />
          </div>
          <!-- 操作按钮组：用户消息（靠右）放气泡左侧，助手消息（靠左）放气泡右侧，上下排列 -->
          <div
            v-if="msg.content.trim() || msg.id > 0"
            class="flex flex-col justify-center shrink-0 gap-0.5 mx-0.5 order-first [&_.el-button+_.el-button]:ml-0"
            :class="msg.role === 'user' ? 'order-first' : 'order-last'"
          >
            <el-button
              v-if="msg.content.trim()"
              class="w-5 h-5 min-w-5 p-0 m-0 opacity-0 transition-opacity duration-150 ease-[ease] [.msg-row:hover_&]:opacity-55 [.msg-row_&]:hover:opacity-100"
              text
              size="small"
              :icon="CopyDocument"
              :title="t('common.copy')"
              @click="copyMessage(msg)"
            />
            <el-button
              v-if="msg.id > 0"
              class="w-5 h-5 min-w-5 p-0 m-0 opacity-0 transition-opacity duration-150 ease-[ease] [.msg-row:hover_&]:opacity-55 [.msg-row_&]:hover:opacity-100"
              text
              size="small"
              :icon="Delete"
              :title="t('common.delete')"
              @click="removeMessage(msg)"
            />
          </div>
        </div>
      </el-scrollbar>
    </div>

    <div
      class="relative pt-2 shrink-0 [&_.el-textarea\_\_inner]:pt-[9px] [&_.el-textarea\_\_inner]:pr-[46px] [&_.el-textarea\_\_inner]:pb-[9px] [&_.el-textarea\_\_inner]:pl-3 [&_.el-textarea\_\_inner]:rounded-xl [&_.el-textarea\_\_inner]:leading-[1.55] [&_.el-textarea\_\_inner]:resize-none [&_.el-textarea\_\_inner]:[scrollbar-width:none] [&_.el-textarea\_\_inner::-webkit-scrollbar]:hidden"
    >
      <el-input
        v-model="input"
        type="textarea"
        :autosize="{ minRows: 1, maxRows: 4 }"
        :placeholder="t('chat.inputPh')"
        :disabled="models.length === 0"
        @keydown.enter.exact.prevent="onEnterKey"
      />
      <el-button
        v-if="streaming"
        class="absolute right-1.5 bottom-1.5 w-[30px] h-[30px] min-w-[30px] p-0 m-0 [&_.el-icon]:text-[15px]"
        type="danger"
        :icon="VideoPause"
        circle
        @click="stop"
      />
      <el-button
        v-else
        class="absolute right-1.5 bottom-1.5 w-[30px] h-[30px] min-w-[30px] p-0 m-0 [&_.el-icon]:text-[15px] disabled:border-transparent disabled:bg-dp-btn-bg disabled:text-dp-tertiary"
        type="primary"
        :icon="Position"
        circle
        :disabled="!input.trim()"
        @click="send"
      />
    </div>

    <el-dialog
      v-model="showModels"
      :title="editing ? t('chat.editModel') : t('chat.addModel')"
      width="340px"
      class="max-w-[calc(100vw-24px)] [&_.el-dialog\_\_body]:pt-3"
      append-to-body
    >
      <!-- 弹窗内容统一交给 el-scrollbar（限高由 main.css 的 .el-dialog 统一约束） -->
      <el-scrollbar class="dp-scroll pr-1">
        <div class="mb-1 text-[11px] font-semibold text-dp-secondary">{{ t("chat.myModels") }}</div>
        <el-scrollbar max-height="110px" class="pr-1">
          <div
            v-for="m in models"
            :key="m.id"
            class="flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-dp-btn-bg"
          >
            <div class="flex flex-col min-w-0 mr-auto">
              <span
                class="overflow-hidden text-ellipsis whitespace-nowrap text-[13px]"
                :class="m.id === activeId ? 'text-dp-accent font-semibold' : 'text-dp-primary'"
              >{{ m.name || m.model }}</span>
              <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-dp-muted">
                {{ m.model }}
              </span>
            </div>
            <div class="inline-flex items-center shrink-0 [&_.el-button+_.el-button]:ml-0.5">
              <el-button v-if="m.id !== activeId" size="small" text type="primary" @click="config.setActiveAiModel(m.id)">
                {{ t("chat.use") }}
              </el-button>
              <el-button size="small" text :icon="Edit" @click="openEdit(m)" />
              <el-button size="small" text :icon="Delete" @click="removeModel(m)" />
            </div>
          </div>
          <el-empty v-if="models.length === 0" :description="t('chat.emptyModels')" :image-size="50" />
        </el-scrollbar>
        <el-divider class="my-3" />
        <el-form
          label-width="62px"
          class="[&_.el-form-item]:mb-2.5 [&_.el-form-item]:items-center [&_.el-form-item\_\_label]:p-0 [&_.el-form-item\_\_label]:pr-2 [&_.el-form-item\_\_label]:text-[12px] [&_.el-form-item\_\_label]:font-medium [&_.el-form-item\_\_label]:leading-none [&_.el-form-item\_\_label]:text-dp-secondary"
          @submit.prevent
        >
          <el-form-item :label="t('chat.modelName')">
            <el-input v-model="form.name" :placeholder="t('chat.modelNamePh')" />
          </el-form-item>
          <el-form-item :label="t('chat.baseUrl')">
            <el-input v-model="form.base_url" :placeholder="t('chat.baseUrlPh')" clearable />
          </el-form-item>
          <el-form-item :label="t('chat.apiKey')">
            <el-input v-model="form.api_key" type="password" show-password :placeholder="t('chat.apiKeyPh')" />
          </el-form-item>
          <el-form-item :label="t('chat.modelId')">
            <el-input v-model="form.model" :placeholder="t('chat.modelIdPh')" />
          </el-form-item>
          <div class="flex justify-end gap-2">
            <el-button size="small" @click="openAdd">{{ t("chat.newForm") }}</el-button>
            <el-button size="small" type="primary" @click="saveModel">
              {{ t("common.confirm") }}
            </el-button>
          </div>
        </el-form>

        <!-- Vortex 推广位：整卡点击在系统浏览器打开官网 -->
        <div
          class="relative mt-3 cursor-pointer overflow-hidden rounded-xl px-3 py-2.5 bg-[linear-gradient(120deg,#7c3aed_0%,#4f46e5_50%,#0284c7_100%)] transition-opacity duration-150 hover:opacity-95"
          role="link"
          :title="VORTEX_URL"
          @click="openVortex"
        >
          <div class="absolute -right-5 -top-7 h-16 w-16 rounded-full bg-white/10" />
          <div class="absolute -left-6 -bottom-9 h-14 w-14 rounded-full bg-white/10" />
          <div class="relative flex items-center gap-2.5">
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-white/20 text-[15px] font-bold text-white">V</div>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-bold leading-tight tracking-wide text-white">Vortex</div>
              <div class="text-[10.5px] leading-tight text-white/85">{{ t("chat.vortexTagline") }}</div>
            </div>
            <el-button
              v-if="VORTEX_API_URL"
              size="small"
              class="h-6 px-2 m-0 shrink-0 !bg-white/15 !border-transparent !text-white hover:!bg-white/25"
              @click.stop="fillVortex"
            >
              {{ t("chat.vortexFill") }}
            </el-button>
            <span class="inline-flex shrink-0 items-center gap-0.5 text-[10.5px] text-white/85">
              {{ t("chat.vortexLearn") }}
              <el-icon class="text-[10px]"><ArrowRight /></el-icon>
            </span>
          </div>
        </div>
      </el-scrollbar>
    </el-dialog>

    <!-- MCP 服务器管理：列表 + 添加/编辑表单 -->
    <el-dialog
      v-model="showMcp"
      :title="t('chat.mcpManager')"
      width="340px"
      class="max-w-[calc(100vw-24px)] [&_.el-dialog\_\_body]:pt-3"
      append-to-body
    >
      <el-scrollbar class="dp-scroll pr-1">
        <div class="mb-1 text-[11px] font-semibold text-dp-secondary">{{ t("chat.mcpMyServers") }}</div>
        <el-scrollbar max-height="150px" class="pr-1">
          <div
            v-for="s in mcpServers"
            :key="s.id"
            class="flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-dp-btn-bg"
          >
            <span
              class="w-2 h-2 rounded-full shrink-0"
              :class="mcpStatus(s) === 'connected' ? 'bg-[#67c23a]' : mcpStatus(s) === 'connecting' ? 'bg-dp-accent animate-pulse' : 'bg-dp-divider'"
              :title="mcpStatusText(s)"
            />
            <div class="flex flex-col min-w-0 mr-auto">
              <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[13px] text-dp-primary">
                {{ s.name }}
                <span class="text-[10px] text-dp-tertiary">({{ s.transport === "http" ? "HTTP" : "stdio" }})</span>
              </span>
              <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-dp-muted">
                {{ s.transport === "http" ? s.url : s.command }}
              </span>
            </div>
            <div class="inline-flex items-center shrink-0 gap-1 [&_.el-button+_.el-button]:ml-0.5">
              <el-switch
                size="small"
                :model-value="s.enabled ?? true"
                :title="t('chat.mcpEnabled')"
                @change="(v: string | number | boolean) => toggleMcpEnabled(s, Boolean(v))"
              />
              <el-button
                v-if="mcpStatus(s) !== 'connected'"
                size="small"
                text
                type="primary"
                :loading="mcpConnectingId === s.id"
                @click="connectMcp(s.id)"
              >
                {{ t("chat.mcpConnect") }}
              </el-button>
              <el-button v-else size="small" text @click="disconnectMcp(s.id)">
                {{ t("chat.mcpDisconnect") }}
              </el-button>
              <el-button size="small" text :icon="Edit" @click="openMcpEdit(s)" />
              <el-button size="small" text :icon="Delete" @click="removeMcpServer(s)" />
            </div>
          </div>
          <el-empty v-if="mcpServers.length === 0" :description="t('chat.mcpEmpty')" :image-size="50" />
        </el-scrollbar>
        <el-divider class="my-3" />
        <el-form
          label-width="62px"
          class="[&_.el-form-item]:mb-2.5 [&_.el-form-item]:items-center [&_.el-form-item\_\_label]:p-0 [&_.el-form-item\_\_label]:pr-2 [&_.el-form-item\_\_label]:text-[12px] [&_.el-form-item\_\_label]:font-medium [&_.el-form-item\_\_label]:leading-none [&_.el-form-item\_\_label]:text-dp-secondary"
          @submit.prevent
        >
          <el-form-item :label="t('chat.mcpName')">
            <el-input v-model="mcpForm.name" :placeholder="t('chat.mcpNamePh')" />
          </el-form-item>
          <el-form-item :label="t('chat.mcpType')">
            <el-radio-group v-model="mcpForm.transport" size="small">
              <el-radio-button value="stdio">stdio</el-radio-button>
              <el-radio-button value="http">HTTP</el-radio-button>
            </el-radio-group>
          </el-form-item>
          <template v-if="mcpForm.transport === 'stdio'">
            <el-form-item :label="t('chat.mcpCommand')">
              <el-input v-model="mcpForm.command" :placeholder="t('chat.mcpCommandPh')" />
            </el-form-item>
            <el-form-item :label="t('chat.mcpArgs')">
              <el-input v-model="mcpForm.args" :placeholder="t('chat.mcpArgsPh')" />
            </el-form-item>
            <el-form-item :label="t('chat.mcpEnv')">
              <el-input
                v-model="mcpForm.env"
                type="textarea"
                :rows="2"
                :placeholder="t('chat.mcpEnvPh')"
              />
            </el-form-item>
          </template>
          <el-form-item v-else :label="t('chat.mcpUrl')">
            <el-input v-model="mcpForm.url" :placeholder="t('chat.mcpUrlPh')" clearable />
          </el-form-item>
          <div class="flex justify-end gap-2">
            <el-button size="small" @click="openMcpAdd">{{ t("chat.newForm") }}</el-button>
            <el-button size="small" type="primary" @click="saveMcpServer">
              {{ t("common.confirm") }}
            </el-button>
          </div>
        </el-form>
      </el-scrollbar>
    </el-dialog>
  </div>
</template>
