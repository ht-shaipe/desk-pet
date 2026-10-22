<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import CryptoJS from "crypto-js";
import { DocumentCopy, Lock } from "@element-plus/icons-vue";
import { copyToClipboard } from "@/api/clipboard";

const { t } = useI18n();

type DesMode = "encrypt" | "decrypt";
const mode = ref<DesMode>("encrypt");

const key = ref("");
const input = ref("");
const output = ref("");

/** DES 密钥固定 8 字节：截断或以 \0 补齐，行为与常见在线工具一致。 */
function parseKey(): CryptoJS.lib.WordArray {
  const k = key.value.length >= 8 ? key.value.slice(0, 8) : key.value.padEnd(8, "\0");
  return CryptoJS.enc.Utf8.parse(k);
}

const DES_OPTS = {
  mode: CryptoJS.mode.ECB,
  padding: CryptoJS.pad.Pkcs7,
} as const;

function convert() {
  const text = input.value;
  if (!text.trim()) {
    ElMessage.warning(t("toolbox.tools.des.emptyWarn"));
    return;
  }
  if (!key.value) {
    ElMessage.warning(t("toolbox.tools.des.keyWarn"));
    return;
  }
  const keyWA = parseKey();
  try {
    if (mode.value === "encrypt") {
      output.value = CryptoJS.DES.encrypt(text, keyWA, DES_OPTS).toString();
    } else {
      const cipher = CryptoJS.lib.CipherParams.create({
        ciphertext: CryptoJS.enc.Base64.parse(text.trim()),
      });
      const plain = CryptoJS.DES.decrypt(cipher, keyWA, DES_OPTS).toString(CryptoJS.enc.Utf8);
      if (!plain) {
        throw new Error("empty");
      }
      output.value = plain;
    }
  } catch {
    output.value = "";
    ElMessage.error(t("toolbox.tools.des.fail"));
  }
}

async function copyResult() {
  await copyToClipboard(output.value);
  ElMessage.success(t("toolbox.tools.des.copied"));
}
</script>

<template>
  <div class="des-tool">
    <div class="mode-row">
      <div class="mode-switch">
        <button
          class="switch-btn"
          :class="{ active: mode === 'encrypt' }"
          @click="mode = 'encrypt'"
        >
          {{ t("toolbox.tools.des.encrypt") }}
        </button>
        <button
          class="switch-btn"
          :class="{ active: mode === 'decrypt' }"
          @click="mode = 'decrypt'"
        >
          {{ t("toolbox.tools.des.decrypt") }}
        </button>
      </div>
    </div>

    <el-scrollbar class="view-scroll">
      <el-input
        v-model="key"
        :placeholder="t('toolbox.tools.des.keyPh')"
        maxlength="32"
        clearable
        class="key-input"
      >
        <template #prefix>
          <el-icon><Lock /></el-icon>
        </template>
      </el-input>
      <el-input
        v-model="input"
        type="textarea"
        :rows="4"
        :placeholder="mode === 'encrypt' ? t('toolbox.tools.des.plainPh') : t('toolbox.tools.des.cipherPh')"
        class="text-input"
      />
      <div class="action-row">
        <el-button type="primary" size="small" @click="convert">
          {{ mode === "encrypt" ? t("toolbox.tools.des.encrypt") : t("toolbox.tools.des.decrypt") }}
        </el-button>
      </div>
      <el-input
        v-model="output"
        type="textarea"
        :rows="4"
        readonly
        :placeholder="t('toolbox.tools.des.outputPh')"
      />
      <div v-if="output" class="result-actions">
        <el-button size="small" text type="primary" :icon="DocumentCopy" @click="copyResult">
          {{ t("toolbox.tools.des.copyResult") }}
        </el-button>
      </div>
    </el-scrollbar>
  </div>
</template>

<style scoped>
.des-tool {
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

.key-input {
  margin-bottom: 10px;
}

.text-input {
  margin-bottom: 10px;
}

.action-row {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}

.result-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 6px;
}
</style>
