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
  <div class="view-root">
    <div class="flex mb-2.5">
      <div class="flex gap-1">
        <button
          class="text-xs px-3 py-1 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="
            mode === 'encode'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
          @click="mode = 'encode'"
        >
          {{ t("toolbox.tools.base64.encode") }}
        </button>
        <button
          class="text-xs px-3 py-1 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="
            mode === 'decode'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
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
      <div class="flex gap-2 mt-2.5">
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
        class="mt-2.5"
      />
      <div v-if="output" class="flex justify-end mt-1.5">
        <el-button size="small" text type="primary" :icon="DocumentCopy" @click="copyResult">
          {{ t("toolbox.tools.base64.copyResult") }}
        </el-button>
      </div>
    </el-scrollbar>
  </div>
</template>

