<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ArrowRight, Delete, Edit, Plus, Position, VideoPause } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { useConfigStore } from "@/stores/config";
import { isModelReady, streamChatCompletion } from "@/api/ai";
import { launchItem } from "@/api/launcher";
import { renderMarkdown } from "@/lib/md";
import { getChatMessages, addChatMessage, deleteChatMessage, clearChatMessages } from "@/api/chat";
import type { AiModel, ChatEntry, ChatMessage } from "@/types";

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
const rendered = computed<ChatEntry[]>(() =>
  pending.value ? [...messages.value, pending.value] : messages.value
);

const input = ref("");
const streaming = ref(false);
let abortController: AbortController | null = null;

const listRef = ref<{ wrapRef: HTMLElement } | null>(null);

/** 滚动到消息底部。 */
function scrollToEnd() {
  void nextTick(() => {
    const wrap = listRef.value?.wrapRef;
    if (wrap) {
      wrap.scrollTop = wrap.scrollHeight;
    }
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
  const history: ChatMessage[] = messages.value.map((m) => ({ role: m.role, content: m.content }));
  pending.value = { id: -1, role: "assistant", content: "", created_at: 0 };
  streaming.value = true;
  abortController = new AbortController();
  try {
    await streamChatCompletion({
      model,
      messages: history,
      signal: abortController.signal,
      onDelta: (delta) => {
        if (pending.value) {
          pending.value.content += delta;
        }
      },
    });
    const content = pending.value?.content ?? "";
    if (content) {
      await saveAssistant(content);
    }
  } catch (err) {
    const partial = pending.value?.content ?? "";
    const msg = abortController?.signal.aborted
      ? t("chat.stopped")
      : requestFailText(err, model);
    if (partial) {
      await saveAssistant(`${partial}\n\n(${msg})`);
    } else {
      ElMessage.error(msg);
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
      <div class="inline-flex items-center gap-1">
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
                : 'bg-dp-btn-bg text-dp-primary rounded-bl-[4px]',
              msg.role !== 'user' && pending && msg.id < 0 ? 'chat-md-cursor' : '',
            ]"
            @click="msg.role === 'user' ? undefined : onLinkClick($event)"
          >
            <span v-if="msg.role === 'user'" class="whitespace-pre-wrap">{{ msg.content }}</span>
            <div
              v-else
              class="chat-md-body"
              v-html="renderMarkdown(msg.content)"
            />
          </div>
          <el-button
            v-if="msg.id > 0"
            class="shrink-0 self-center w-5 h-5 min-w-5 p-0 my-0 mx-0.5 opacity-0 transition-opacity duration-150 ease-[ease] [.msg-row:hover_&]:opacity-55 [.msg-row_&]:hover:opacity-100"
            text
            size="small"
            :icon="Delete"
            :title="t('common.delete')"
            @click="removeMessage(msg)"
          />
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
  </div>
</template>

