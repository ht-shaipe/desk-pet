<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { DocumentCopy, Sort } from "@element-plus/icons-vue";
import { copyToClipboard } from "@/api/clipboard";

const { t } = useI18n();

type Base64Mode = "encode" | "decode";
const mode = ref<Base64Mode>("encode");

const input = ref("");
const output = ref("");

function encodeUtf8(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (const b of bytes) {
    bin += String.fromCharCode(b);
  }
  return btoa(bin);
}

function decodeUtf8(b64: string): string {
  const bin = atob(b64);
  const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function convert() {
  const text = input.value.trim();
  if (!text) {
    ElMessage.warning(t("toolbox.tools.base64.emptyWarn"));
    return;
  }
  try {
    output.value = mode.value === "encode" ? encodeUtf8(text) : decodeUtf8(text);
  } catch {
    output.value = "";
    ElMessage.error(t("toolbox.tools.base64.invalidInput"));
  }
}

function swap() {
  if (!output.value) {
    return;
  }
  input.value = output.value;
  output.value = "";
  mode.value = mode.value === "encode" ? "decode" : "encode";
  convert();
}

async function copyResult() {
  await copyToClipboard(output.value);
  ElMessage.success(t("toolbox.tools.base64.copied"));
}
</script>

<template>
  <div class="base64-tool">
    <div class="mode-row">
      <div class="mode-switch">
        <button
          class="switch-btn"
          :class="{ active: mode === 'encode' }"
          @click="mode = 'encode'"
        >
          {{ t("toolbox.tools.base64.encode") }}
        </button>
        <button
          class="switch-btn"
          :class="{ active: mode === 'decode' }"
          @click="mode = 'decode'"
        >
          {{ t("toolbox.tools.base64.decode") }}
        </button>
      </div>
    </div>

    <el-scrollbar class="view-scroll">
      <el-input
        v-model="input"
        type="textarea"
        :rows="4"
        :placeholder="t('toolbox.tools.base64.inputPh')"
      />
      <div class="action-row">
        <el-button type="primary" size="small" @click="convert">
          {{ mode === "encode" ? t("toolbox.tools.base64.encode") : t("toolbox.tools.base64.decode") }}
        </el-button>
        <el-button size="small" :icon="Sort" :disabled="!output" @click="swap">
          {{ t("toolbox.tools.base64.swap") }}
        </el-button>
      </div>
      <el-input
        v-model="output"
        type="textarea"
        :rows="4"
        readonly
        :placeholder="t('toolbox.tools.base64.outputPh')"
        class="output-area"
      />
      <div v-if="output" class="result-actions">
        <el-button size="small" text type="primary" :icon="DocumentCopy" @click="copyResult">
          {{ t("toolbox.tools.base64.copyResult") }}
        </el-button>
      </div>
    </el-scrollbar>
  </div>
</template>

<style scoped>
.base64-tool {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.mode-row {
  display: flex;
  margin-bottom: 10px;
}

.mode-switch {
  display: flex;
  gap: 4px;
}

.switch-btn {
  border: 1px solid var(--dp-card-border);
  background: var(--dp-card-bg);
  color: var(--dp-text-muted);
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 999px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.switch-btn.active {
  background: var(--dp-accent);
  border-color: var(--dp-accent);
  color: #ffffff;
}

.action-row {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}

.output-area {
  margin-top: 10px;
}

.result-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 6px;
}
</style>
