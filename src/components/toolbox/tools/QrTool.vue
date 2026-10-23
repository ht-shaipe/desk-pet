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
  <div class="view-root">
    <div class="flex mb-2.5">
      <div class="flex gap-1">
        <button
          class="text-xs px-3 py-1 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="
            mode === 'generate'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
          @click="mode = 'generate'"
        >
          {{ t("toolbox.tools.qr.generate") }}
        </button>
        <button
          class="text-xs px-3 py-1 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="
            mode === 'parse'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
          @click="mode = 'parse'"
        >
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
      <div class="flex items-center gap-2 mt-2.5">
        <span class="text-xs text-dp-muted">{{ t("toolbox.tools.qr.size") }}</span>
        <el-radio-group v-model="qrSize" size="small">
          <el-radio-button :value="180">180</el-radio-button>
          <el-radio-button :value="240">240</el-radio-button>
          <el-radio-button :value="320">320</el-radio-button>
        </el-radio-group>
        <el-button class="ml-auto" type="primary" size="small" @click="generate">
          {{ t("toolbox.tools.qr.generateBtn") }}
        </el-button>
      </div>
      <div v-show="generated" class="flex flex-col items-center gap-3 mt-3.5">
        <canvas ref="canvasRef" class="rounded-lg shadow-[0_4px_16px_rgba(31,41,55,0.12)]" />
        <div class="flex gap-2">
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
      <div class="flex gap-2 mt-1">
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
          class="hidden"
          @change="onFileChange"
        />
      </div>
      <div v-show="parseLoaded" class="flex justify-center mt-3.5">
        <canvas
          ref="previewCanvasRef"
          class="max-w-[140px] max-h-[140px] rounded-lg shadow-[0_4px_16px_rgba(31,41,55,0.12)]"
        />
      </div>
      <div v-if="parseResult" class="mt-3.5 p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <div class="text-[11px] text-dp-tertiary mb-1.5">{{ t("toolbox.tools.qr.result") }}</div>
        <div class="text-[13px] text-dp-primary leading-[1.5] [word-break:break-all] whitespace-pre-wrap mb-2">
          {{ parseResult }}
        </div>
        <el-button size="small" text type="primary" :icon="DocumentChecked" @click="copyResult">
          {{ t("toolbox.tools.qr.copyText") }}
        </el-button>
      </div>
      <el-empty v-if="!parseLoaded" :description="t('toolbox.tools.qr.parseHint')" :image-size="60" />
    </el-scrollbar>
  </div>
</template>

