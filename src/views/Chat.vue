<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Delete, Edit, Plus, Position, VideoPause } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { useConfigStore } from "@/stores/config";
import { isModelReady, streamChatCompletion } from "@/api/ai";
import type { AiModel, ChatMessage } from "@/types";

const { t } = useI18n();
const config = useConfigStore();

const models = computed<AiModel[]>(() => config.settings.ai_models ?? []);
const activeId = computed<string | null>(() => config.settings.ai_active ?? null);
const activeModel = computed<AiModel | null>(
  () => models.value.find((m) => m.id === activeId.value) ?? null
);
const messages = computed<ChatMessage[]>(() => config.settings.ai_messages ?? []);

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

watch(messages, scrollToEnd, { deep: true });
watch(
  () => models.value.length,
  () => {
    if (!activeModel.value && models.value.length > 0) {
      void config.setActiveAiModel(models.value[0].id);
    }
  },
  { immediate: true }
);

function persist(list: ChatMessage[]) {
  void config.setAiMessages(list);
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
  const history: ChatMessage[] = [...messages.value, { role: "user", content: text }];
  input.value = "";
  persist([...history, { role: "assistant", content: "" }]);
  streaming.value = true;
  abortController = new AbortController();
  const list = [...history, { role: "assistant", content: "" }];
  const patch = (content: string) => {
    list[list.length - 1] = { role: "assistant", content };
    persist(list);
  };
  try {
    await streamChatCompletion({
      model,
      messages: history,
      signal: abortController.signal,
      onDelta: (delta) => {
        patch(list[list.length - 1].content + delta);
      },
    });
  } catch (err) {
    const fallback = list[list.length - 1].content;
    const msg = abortController?.signal.aborted
      ? t("chat.stopped")
      : `${t("chat.requestFail")}: ${err instanceof Error ? err.message : String(err)}`;
    if (!fallback) {
      // 无任何输出时移除空的 assistant 占位，改为错误提示
      persist(list.slice(0, -1));
      ElMessage.error(msg);
    } else {
      patch(`${fallback}\n\n(${msg})`);
    }
  } finally {
    streaming.value = false;
    abortController = null;
  }
}

function stop() {
  abortController?.abort();
}

function clearHistory() {
  if (messages.value.length === 0) {
    return;
  }
  persist([]);
}

/** ---------- 模型管理 ---------- */
const showModels = ref(false);
const editing = ref<AiModel | null>(null);
const form = ref<Omit<AiModel, "id">>({ name: "", base_url: "", api_key: "", model: "" });

function openAdd() {
  editing.value = null;
  form.value = { name: "", base_url: "", api_key: "", model: "" };
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
    base_url: f.base_url.trim(),
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
</script>

<template>
  <div class="chat-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("chat.title") }}</span>
      <div class="toolbar-actions">
        <el-select
          v-if="models.length > 0"
          :model-value="activeId"
          size="small"
          class="model-select"
          @update:model-value="(v: string) => config.setActiveAiModel(v)"
        >
          <el-option v-for="m in models" :key="m.id" :value="m.id" :label="m.name || m.model" />
        </el-select>
        <el-button size="small" text type="primary" :icon="Plus" @click="openAdd(); showModels = true">
          {{ t("chat.addModel") }}
        </el-button>
        <el-button size="small" text :icon="Delete" @click="clearHistory" />
      </div>
    </div>

    <div class="chat-body">
      <el-empty
        v-if="models.length === 0"
        :description="t('chat.emptyModels')"
        :image-size="60"
        class="chat-empty"
      >
        <el-button type="primary" size="small" :icon="Plus" @click="openAdd(); showModels = true">
          {{ t("chat.addModel") }}
        </el-button>
      </el-empty>
      <el-empty
        v-else-if="messages.length === 0"
        :description="t('chat.emptyMessages')"
        :image-size="60"
        class="chat-empty"
      />
      <el-scrollbar v-else ref="listRef" class="view-scroll chat-list">
        <div
          v-for="(msg, i) in messages"
          :key="i"
          class="msg-row"
          :class="msg.role === 'user' ? 'mine' : 'theirs'"
        >
          <div class="msg-bubble" :class="msg.role">
            <span class="msg-text">{{ msg.content }}</span>
            <span v-if="streaming && i === messages.length - 1 && msg.role === 'assistant'" class="cursor" />
          </div>
        </div>
      </el-scrollbar>
    </div>

    <div class="chat-input">
      <el-input
        v-model="input"
        type="textarea"
        :autosize="{ minRows: 1, maxRows: 4 }"
        :placeholder="t('chat.inputPh')"
        :disabled="models.length === 0"
        @keydown.enter.exact.prevent="send"
      />
      <el-button
        v-if="streaming"
        class="send-btn stop"
        type="danger"
        :icon="VideoPause"
        circle
        @click="stop"
      />
      <el-button
        v-else
        class="send-btn"
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
      width="320px"
      class="model-dialog"
      append-to-body
    >
      <el-scrollbar max-height="110px" class="model-list">
        <div v-for="m in models" :key="m.id" class="model-item">
          <div class="model-info">
            <span class="model-name" :class="{ active: m.id === activeId }">{{ m.name || m.model }}</span>
            <span class="model-meta">{{ m.model }}</span>
          </div>
          <div class="model-ops">
            <el-button v-if="m.id !== activeId" size="small" text type="primary" @click="config.setActiveAiModel(m.id)">
              {{ t("chat.use") }}
            </el-button>
            <el-button size="small" text :icon="Edit" @click="openEdit(m)" />
            <el-button size="small" text :icon="Delete" @click="removeModel(m)" />
          </div>
        </div>
        <el-empty v-if="models.length === 0" :description="t('chat.emptyModels')" :image-size="50" />
      </el-scrollbar>
      <el-divider class="model-divider" />
      <el-form label-position="left" label-width="76px" class="model-form" @submit.prevent>
        <el-form-item :label="t('chat.modelName')">
          <el-input v-model="form.name" :placeholder="t('chat.modelNamePh')" />
        </el-form-item>
        <el-form-item :label="t('chat.baseUrl')">
          <el-input v-model="form.base_url" :placeholder="t('chat.baseUrlPh')" />
        </el-form-item>
        <el-form-item :label="t('chat.apiKey')">
          <el-input v-model="form.api_key" type="password" show-password :placeholder="t('chat.apiKeyPh')" />
        </el-form-item>
        <el-form-item :label="t('chat.modelId')">
          <el-input v-model="form.model" :placeholder="t('chat.modelIdPh')" />
        </el-form-item>
        <div class="model-form-ops">
          <el-button size="small" @click="openAdd">{{ t("chat.newForm") }}</el-button>
          <el-button size="small" type="primary" @click="saveModel">
            {{ t("common.confirm") }}
          </el-button>
        </div>
      </el-form>
    </el-dialog>
  </div>
</template>

<style scoped>
.chat-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.toolbar-actions {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.model-select {
  width: 120px;
}

.chat-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.chat-empty {
  margin: auto;
}

.chat-list {
  flex: 1;
  min-height: 0;
  padding-right: 4px;
}

.msg-row {
  display: flex;
  margin-bottom: 10px;
}

.msg-row.mine {
  justify-content: flex-end;
}

.msg-row.theirs {
  justify-content: flex-start;
}

.msg-bubble {
  max-width: 82%;
  padding: 7px 11px;
  border-radius: 12px;
  font-size: 13px;
  line-height: 1.55;
  word-break: break-word;
}

.msg-bubble.user {
  background: var(--dp-accent);
  color: #fff;
  border-bottom-right-radius: 4px;
}

.msg-bubble.assistant {
  background: var(--dp-btn-bg);
  color: var(--dp-text-primary);
  border-bottom-left-radius: 4px;
}

.msg-text {
  white-space: pre-wrap;
}

/* 流式输出光标 */
.cursor {
  display: inline-block;
  width: 2px;
  height: 14px;
  margin-left: 2px;
  vertical-align: -2px;
  background: var(--dp-accent);
  animation: blink 0.9s step-end infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}

.chat-input {
  display: flex;
  align-items: flex-end;
  gap: 8px;
  padding-top: 8px;
  flex-shrink: 0;
}

.chat-input :deep(.el-textarea__inner) {
  border-radius: 10px;
}

.send-btn {
  flex-shrink: 0;
  margin-bottom: 2px;
}

/* ---------- 模型管理 ---------- */
.model-list {
  padding-right: 4px;
}

.model-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 8px;
}

.model-item:hover {
  background: var(--dp-btn-bg);
}

.model-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  margin-right: auto;
}

.model-name {
  font-size: 13px;
  color: var(--dp-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-name.active {
  color: var(--dp-accent);
  font-weight: 600;
}

.model-meta {
  font-size: 11px;
  color: var(--dp-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-ops {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
}

.model-ops .el-button + .el-button {
  margin-left: 2px;
}

.model-divider {
  margin: 12px 0;
}

.model-form :deep(.el-form-item) {
  margin-bottom: 10px;
}

.model-form-ops {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>

<style>
/* 模型管理弹窗：面板窗口较小，约束弹窗宽高避免出现页面滚动条 */
.model-dialog {
  max-width: calc(100vw - 24px);
}

.model-dialog .el-dialog {
  max-width: calc(100vw - 24px);
}

.model-dialog .el-dialog__body {
  max-height: calc(100vh - 140px);
  overflow-y: auto;
  padding-top: 10px;
}
</style>
