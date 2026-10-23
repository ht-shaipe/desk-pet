<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { CopyDocument, Download, Picture, Upload } from "@element-plus/icons-vue";
import { getClipboardImage, saveImage, setClipboardImage } from "@/api/toolbox";
import type { ImagePayload } from "@/types";

const { t } = useI18n();

const fileInputRef = ref<HTMLInputElement | null>(null);

/* ---------- 原图状态 ---------- */
const sourceCanvas = ref<HTMLCanvasElement | null>(null);
const origWidth = ref(0);
const origHeight = ref(0);
/** 原图 PNG 等效大小（字节），用于和压缩后的 JPEG 对比 */
const origPngBytes = ref(0);

/* ---------- 压缩参数与结果 ---------- */
const quality = ref(0.7);
const maxDim = ref(0); // 0 = 不限制长边
const resultUrl = ref("");
const resultBytes = ref(0);
const resultWidth = ref(0);
const resultHeight = ref(0);

const loaded = computed(() => sourceCanvas.value !== null);
const savedPercent = computed(() => {
  if (!origPngBytes.value || !resultBytes.value) {
    return 0;
  }
  return Math.max(0, Math.round((1 - resultBytes.value / origPngBytes.value) * 100));
});

function kb(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function loadFromCanvas(img: CanvasImageSource, w: number, h: number) {
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    ElMessage.error(t("toolbox.tools.imgcompress.loadFail"));
    return;
  }
  ctx.drawImage(img, 0, 0, w, h);
  sourceCanvas.value = canvas;
  origWidth.value = w;
  origHeight.value = h;
  origPngBytes.value = Math.round(canvas.toDataURL("image/png").length * 0.75);
  resultUrl.value = "";
  resultBytes.value = 0;
  compress();
}

function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) {
    return;
  }
  if (!file.type.startsWith("image/")) {
    ElMessage.error(t("toolbox.tools.imgcompress.notImage"));
    input.value = "";
    return;
  }
  const url = URL.createObjectURL(file);
  const image = new Image();
  image.onload = () => {
    loadFromCanvas(image, image.naturalWidth, image.naturalHeight);
    URL.revokeObjectURL(url);
  };
  image.onerror = () => {
    URL.revokeObjectURL(url);
    ElMessage.error(t("toolbox.tools.imgcompress.loadFail"));
  };
  image.src = url;
  input.value = "";
}

async function pasteFromClipboard() {
  try {
    const img = await getClipboardImage();
    if (!img) {
      ElMessage.warning(t("toolbox.tools.imgcompress.noImage"));
      return;
    }
    await loadFromPayload(img);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function loadFromPayload(img: ImagePayload) {
  const clamped = new Uint8ClampedArray(img.rgba);
  const imageData = new ImageData(clamped, img.width, img.height);
  const off = document.createElement("canvas");
  off.width = img.width;
  off.height = img.height;
  const offCtx = off.getContext("2d");
  if (!offCtx) {
    return;
  }
  offCtx.putImageData(imageData, 0, 0);
  loadFromCanvas(off, img.width, img.height);
}

function compress() {
  const source = sourceCanvas.value;
  if (!source) {
    return;
  }
  let w = source.width;
  let h = source.height;
  if (maxDim.value > 0 && Math.max(w, h) > maxDim.value) {
    const scale = maxDim.value / Math.max(w, h);
    w = Math.max(1, Math.round(w * scale));
    h = Math.max(1, Math.round(h * scale));
  }
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  // 白底，避免透明区域压缩成黑色
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, w, h);
  ctx.drawImage(source, 0, 0, w, h);

  resultUrl.value = canvas.toDataURL("image/jpeg", quality.value);
  resultBytes.value = Math.round(resultUrl.value.length * 0.75);
  resultWidth.value = w;
  resultHeight.value = h;
}

async function copyResult() {
  const source = sourceCanvas.value;
  if (!source || !resultUrl.value) {
    return;
  }
  // 用结果尺寸重新绘制，取出 RGBA 写入系统剪贴板
  const canvas = document.createElement("canvas");
  canvas.width = resultWidth.value;
  canvas.height = resultHeight.value;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(source, 0, 0, canvas.width, canvas.height);
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
  await setClipboardImage({
    width: canvas.width,
    height: canvas.height,
    rgba: Array.from(data.data),
  });
  ElMessage.success(t("toolbox.tools.imgcompress.copied"));
}

async function saveResult() {
  if (!resultUrl.value) {
    return;
  }
  try {
    await saveImage(resultUrl.value);
    ElMessage.success(t("toolbox.tools.imgcompress.savedDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}
</script>

<template>
  <div class="view-root">
    <div class="flex gap-2 mb-2.5">
      <el-button size="small" type="primary" :icon="Picture" @click="pasteFromClipboard">
        {{ t("toolbox.tools.imgcompress.paste") }}
      </el-button>
      <el-button size="small" :icon="Upload" @click="fileInputRef?.click()">
        {{ t("toolbox.tools.imgcompress.choose") }}
      </el-button>
      <input
        ref="fileInputRef"
        type="file"
        accept="image/*"
        class="hidden"
        @change="onFileChange"
      />
    </div>

    <el-scrollbar class="view-scroll">
      <template v-if="loaded">
        <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px] mb-3">
          <div class="flex items-center gap-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.quality") }}</span>
            <el-slider v-model="quality" :min="0.1" :max="0.95" :step="0.05" class="flex-1 min-w-0" />
            <span class="text-xs text-dp-accent shrink-0 min-w-[34px] text-right">{{ Math.round(quality * 100) }}%</span>
          </div>
          <div class="flex items-center gap-2.5 mt-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.maxDim") }}</span>
            <el-radio-group v-model="maxDim" size="small">
              <el-radio-button :value="0">{{ t("toolbox.tools.imgcompress.noLimit") }}</el-radio-button>
              <el-radio-button :value="1920">1920</el-radio-button>
              <el-radio-button :value="1280">1280</el-radio-button>
              <el-radio-button :value="720">720</el-radio-button>
            </el-radio-group>
            <el-button size="small" type="primary" class="ml-auto" @click="compress">
              {{ t("toolbox.tools.imgcompress.compressBtn") }}
            </el-button>
          </div>
        </div>

        <div v-if="resultUrl" class="flex flex-col items-center">
          <img
            :src="resultUrl"
            class="max-w-full max-h-[150px] rounded-lg shadow-[0_4px_16px_rgba(31,41,55,0.12)]"
            alt="compressed"
          />
          <div class="w-full mt-3 px-2.5 py-2 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
            <div class="flex justify-between text-xs leading-[1.8]">
              <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.original") }}</span>
              <span class="text-dp-muted">
                {{ origWidth }}×{{ origHeight }} · {{ kb(origPngBytes) }}
              </span>
            </div>
            <div class="flex justify-between text-xs leading-[1.8]">
              <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.compressed") }}</span>
              <span class="text-dp-accent font-medium">
                {{ resultWidth }}×{{ resultHeight }} · {{ kb(resultBytes) }}
              </span>
            </div>
            <div
              v-if="savedPercent > 0"
              class="mt-1 text-[11px] text-[var(--el-color-success,#67c23a)] text-center"
            >
              {{ t("toolbox.tools.imgcompress.saved", { n: savedPercent }) }}
            </div>
          </div>
          <div class="flex gap-2 mt-2.5">
            <el-button size="small" :icon="CopyDocument" @click="copyResult">
              {{ t("toolbox.tools.imgcompress.copyImage") }}
            </el-button>
            <el-button size="small" :icon="Download" @click="saveResult">
              {{ t("toolbox.tools.imgcompress.saveImage") }}
            </el-button>
          </div>
        </div>
      </template>
      <el-empty
        v-else
        :description="t('toolbox.tools.imgcompress.hint')"
        :image-size="60"
      />
    </el-scrollbar>
  </div>
</template>

