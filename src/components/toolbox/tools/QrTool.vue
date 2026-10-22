<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { CopyDocument, DocumentChecked, Download, Picture, Upload } from "@element-plus/icons-vue";
import QRCode from "qrcode";
import jsQR from "jsqr";
import { copyToClipboard } from "@/api/clipboard";
import { getClipboardImage, saveQrPng, setClipboardImage } from "@/api/toolbox";
import type { ImagePayload } from "@/types";

const { t } = useI18n();

type QrMode = "generate" | "parse";
const mode = ref<QrMode>("generate");

/* ---------- 生成二维码 ---------- */
const qrText = ref("");
const qrSize = ref(240);
const canvasRef = ref<HTMLCanvasElement | null>(null);
const generated = ref(false);

async function generate() {
  const text = qrText.value.trim();
  if (!text) {
    ElMessage.warning(t("toolbox.tools.qr.emptyWarn"));
    return;
  }
  const canvas = canvasRef.value;
  if (!canvas) {
    return;
  }
  try {
    await QRCode.toCanvas(canvas, text, { width: qrSize.value, margin: 2 });
    generated.value = true;
  } catch (e) {
    ElMessage.error(t("toolbox.tools.qr.generateFail", { err: String(e) }));
  }
}

async function copyImage() {
  const canvas = canvasRef.value;
  if (!canvas || !generated.value) {
    return;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
  await setClipboardImage({
    width: canvas.width,
    height: canvas.height,
    rgba: Array.from(data.data),
  });
  ElMessage.success(t("toolbox.tools.qr.imageCopied"));
}

async function saveImage() {
  const canvas = canvasRef.value;
  if (!canvas || !generated.value) {
    return;
  }
  try {
    await saveQrPng(canvas.toDataURL("image/png"));
    ElMessage.success(t("toolbox.tools.qr.imageSaved"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 解析二维码 ---------- */
const previewCanvasRef = ref<HTMLCanvasElement | null>(null);
const fileInputRef = ref<HTMLInputElement | null>(null);
const parseResult = ref("");
const parseLoaded = ref(false);

async function decodeImageData(imgData: ImageData): Promise<void> {
  const code = jsQR(imgData.data, imgData.width, imgData.height);
  if (code && code.data) {
    parseResult.value = code.data;
    ElMessage.success(t("toolbox.tools.qr.decodeOk"));
  } else {
    parseResult.value = "";
    ElMessage.warning(t("toolbox.tools.qr.decodeFail"));
  }
}

function drawPreview(source: CanvasImageSource, w: number, h: number) {
  const canvas = previewCanvasRef.value;
  if (!canvas) {
    return;
  }
  const max = 140;
  const scale = Math.min(1, max / Math.max(w, h));
  canvas.width = Math.max(1, Math.round(w * scale));
  canvas.height = Math.max(1, Math.round(h * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.drawImage(source, 0, 0, canvas.width, canvas.height);
}

async function handlePayload(img: ImagePayload) {
  const clamped = new Uint8ClampedArray(img.rgba);
  const imageData = new ImageData(clamped, img.width, img.height);
  // ImageData 不能直接作为 drawImage 源，先落到离屏 canvas
  const off = document.createElement("canvas");
  off.width = img.width;
  off.height = img.height;
  const offCtx = off.getContext("2d");
  if (offCtx) {
    offCtx.putImageData(imageData, 0, 0);
    drawPreview(off, img.width, img.height);
  }
  parseLoaded.value = true;
  await decodeImageData(imageData);
}

async function pasteFromClipboard() {
  try {
    const img = await getClipboardImage();
    if (!img) {
      ElMessage.warning(t("toolbox.tools.qr.noImage"));
      return;
    }
    await handlePayload(img);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) {
    return;
  }
  const url = URL.createObjectURL(file);
  const image = new Image();
  image.onload = () => {
    const off = document.createElement("canvas");
    off.width = image.naturalWidth;
    off.height = image.naturalHeight;
    const ctx = off.getContext("2d");
    if (ctx) {
      ctx.drawImage(image, 0, 0);
      const data = ctx.getImageData(0, 0, off.width, off.height);
      drawPreview(image, off.width, off.height);
      parseLoaded.value = true;
      void decodeImageData(data);
    }
    URL.revokeObjectURL(url);
  };
  image.onerror = () => {
    URL.revokeObjectURL(url);
    ElMessage.error(t("toolbox.tools.qr.loadFail"));
  };
  image.src = url;
  input.value = "";
}

async function copyResult() {
  await copyToClipboard(parseResult.value);
  ElMessage.success(t("toolbox.tools.qr.textCopied"));
}
</script>

<template>
  <div class="qr-tool">
    <div class="mode-row">
      <div class="mode-switch">
        <button
          class="switch-btn"
          :class="{ active: mode === 'generate' }"
          @click="mode = 'generate'"
        >
          {{ t("toolbox.tools.qr.generate") }}
        </button>
        <button class="switch-btn" :class="{ active: mode === 'parse' }" @click="mode = 'parse'">
          {{ t("toolbox.tools.qr.parse") }}
        </button>
      </div>
    </div>

    <el-scrollbar v-if="mode === 'generate'" class="view-scroll">
      <el-input
        v-model="qrText"
        type="textarea"
        :rows="3"
        :placeholder="t('toolbox.tools.qr.inputPh')"
        maxlength="500"
      />
      <div class="size-row">
        <span class="size-label">{{ t("toolbox.tools.qr.size") }}</span>
        <el-radio-group v-model="qrSize" size="small">
          <el-radio-button :value="180">180</el-radio-button>
          <el-radio-button :value="240">240</el-radio-button>
          <el-radio-button :value="320">320</el-radio-button>
        </el-radio-group>
        <el-button class="gen-btn" type="primary" size="small" @click="generate">
          {{ t("toolbox.tools.qr.generateBtn") }}
        </el-button>
      </div>
      <div v-show="generated" class="qr-preview">
        <canvas ref="canvasRef" class="qr-canvas" />
        <div class="qr-actions">
          <el-button size="small" :icon="CopyDocument" @click="copyImage">
            {{ t("toolbox.tools.qr.copyImage") }}
          </el-button>
          <el-button size="small" :icon="Download" @click="saveImage">
            {{ t("toolbox.tools.qr.saveImage") }}
          </el-button>
        </div>
      </div>
    </el-scrollbar>

    <el-scrollbar v-else class="view-scroll">
      <div class="parse-actions">
        <el-button size="small" type="primary" :icon="Picture" @click="pasteFromClipboard">
          {{ t("toolbox.tools.qr.paste") }}
        </el-button>
        <el-button size="small" :icon="Upload" @click="fileInputRef?.click()">
          {{ t("toolbox.tools.qr.choose") }}
        </el-button>
        <input
          ref="fileInputRef"
          type="file"
          accept="image/*"
          class="hidden-input"
          @change="onFileChange"
        />
      </div>
      <div v-show="parseLoaded" class="parse-preview">
        <canvas ref="previewCanvasRef" class="preview-canvas" />
      </div>
      <div v-if="parseResult" class="parse-result">
        <div class="result-label">{{ t("toolbox.tools.qr.result") }}</div>
        <div class="result-text">{{ parseResult }}</div>
        <el-button size="small" text type="primary" :icon="DocumentChecked" @click="copyResult">
          {{ t("toolbox.tools.qr.copyText") }}
        </el-button>
      </div>
      <el-empty v-if="!parseLoaded" :description="t('toolbox.tools.qr.parseHint')" :image-size="60" />
    </el-scrollbar>
  </div>
</template>

<style scoped>
.qr-tool {
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
  border: 1px solid #e0e3e8;
  background: #ffffff;
  color: #6b7280;
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 999px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.switch-btn.active {
  background: #409eff;
  border-color: #409eff;
  color: #ffffff;
}

.size-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 10px;
}

.size-label {
  font-size: 12px;
  color: #6b7280;
}

.gen-btn {
  margin-left: auto;
}

.qr-preview {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}

.qr-canvas {
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(31, 41, 55, 0.12);
}

.qr-actions {
  display: flex;
  gap: 8px;
}

.parse-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.hidden-input {
  display: none;
}

.parse-preview {
  display: flex;
  justify-content: center;
  margin-top: 14px;
}

.preview-canvas {
  max-width: 140px;
  max-height: 140px;
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(31, 41, 55, 0.12);
}

.parse-result {
  margin-top: 14px;
  padding: 10px;
  background: #ffffff;
  border: 1px solid #eef0f3;
  border-radius: 10px;
}

.result-label {
  font-size: 11px;
  color: #9ca3af;
  margin-bottom: 6px;
}

.result-text {
  font-size: 13px;
  color: #1f2937;
  line-height: 1.5;
  word-break: break-all;
  white-space: pre-wrap;
  margin-bottom: 8px;
}
</style>
