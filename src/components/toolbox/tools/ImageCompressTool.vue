<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import {
  CopyDocument,
  Download,
  FolderOpened,
  MagicStick,
  Picture,
  Upload,
} from "@element-plus/icons-vue";
import { getClipboardImage, saveImage, setClipboardImage, compressImageData } from "@/api/toolbox";
import { imagesOpen } from "@/api/images";
import type { CompressImageParams } from "@/types";

const { t } = useI18n();

const fileInputRef = ref<HTMLInputElement | null>(null);

/* ---------- 门户:高级压缩 / 资料库(独立窗口) ---------- */
async function openAdvanced() {
  try {
    await imagesOpen("compress");
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function openManager() {
  try {
    await imagesOpen();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 原图状态 ---------- */
const origWidth = ref(0);
const origHeight = ref(0);
/** 原图大小（字节）：上传为原文件大小，剪贴板为 PNG 等效大小 */
const origBytes = ref(0);
/** 原图编码数据（上传为原始文件，剪贴板为 PNG 编码），压缩输入与“保持原图”复用 */
const origDataUrl = ref("");

/* ---------- 快速压缩参数 ---------- */
/** 压缩方案：智能平衡 82 / 高画质 92 / 极小体积 45 / 自定义 */
const PRESET_QUALITY: Record<string, number> = {
  balanced: 82,
  high: 92,
  small: 45,
};
const preset = ref<keyof typeof PRESET_QUALITY | "custom">("balanced");
/** 自定义画质 1-100 */
const quality = ref(82);
const effectiveQuality = computed(() =>
  preset.value === "custom" ? quality.value : PRESET_QUALITY[preset.value],
);
const maxDim = ref(0); // 0 = 不限制长边
/** 输出格式：auto = 智能择优（多格式竞速取最小） */
const format = ref("auto");

const FMT_LABELS: Record<string, string> = {
  jpeg: "JPEG",
  png: "PNG",
  webp: "WebP",
  gif: "GIF",
  bmp: "BMP",
};

/** 前端格式值 → API format 参数 */
function fmtToApi(v: string): CompressImageParams["format"] {
  if (v === "auto" || v === "keep") {
    return v;
  }
  return v.replace("image/", "") as CompressImageParams["format"];
}

function buildParams(): CompressImageParams {
  return {
    quality: effectiveQuality.value,
    maxDim: maxDim.value,
    format: fmtToApi(format.value),
    targetKb: 0,
  };
}

/** 参数签名：单点 watch，参数任一变化即触发重压缩 */
const paramsSig = computed(() => JSON.stringify(buildParams()));

/* ---------- 压缩结果 ---------- */
const resultUrl = ref("");
const resultBytes = ref(0);
const resultFormat = ref("");
const fmtLabel = computed(() => FMT_LABELS[resultFormat.value] ?? "PNG");
const resultWidth = ref(0);
const resultHeight = ref(0);
/** 当前结果是否因重编码未减小而直接沿用原图 */
const resultKept = ref(false);
const resultAnimated = ref(false);
const compressing = ref(false);
let compressToken = 0;

const loaded = computed(() => origDataUrl.value !== "");
const savedPercent = computed(() => {
  if (!origBytes.value || !resultBytes.value) {
    return 0;
  }
  return Math.max(0, Math.round((1 - resultBytes.value / origBytes.value) * 100));
});

function kb(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function resetResult() {
  resultUrl.value = "";
  resultFormat.value = "";
  resultBytes.value = 0;
  resultKept.value = false;
  resultAnimated.value = false;
}

function loadSource(w: number, h: number, src: { bytes: number; dataUrl: string }) {
  origWidth.value = w;
  origHeight.value = h;
  origBytes.value = src.bytes;
  origDataUrl.value = src.dataUrl;
  resetResult();
  void compress();
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
  const reader = new FileReader();
  reader.onload = () => {
    const dataUrl = String(reader.result ?? "");
    const image = new Image();
    image.onload = () => {
      loadSource(image.naturalWidth, image.naturalHeight, {
        bytes: file.size,
        dataUrl,
      });
    };
    image.onerror = () => ElMessage.error(t("toolbox.tools.imgcompress.loadFail"));
    image.src = dataUrl;
  };
  reader.onerror = () => ElMessage.error(t("toolbox.tools.imgcompress.loadFail"));
  reader.readAsDataURL(file);
  input.value = "";
}

async function pasteFromClipboard() {
  try {
    const img = await getClipboardImage();
    if (!img) {
      ElMessage.warning(t("toolbox.tools.imgcompress.noImage"));
      return;
    }
    const clamped = new Uint8ClampedArray(img.rgba);
    const canvas = document.createElement("canvas");
    canvas.width = img.width;
    canvas.height = img.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    ctx.putImageData(new ImageData(clamped, img.width, img.height), 0, 0);
    // 剪贴板为 RGBA 数据：以 PNG 编码为原始输入交给 Rust 压缩
    const pngUrl = canvas.toDataURL("image/png");
    loadSource(img.width, img.height, {
      bytes: Math.round(pngUrl.length * 0.75),
      dataUrl: pngUrl,
    });
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** 单图压缩：调用 Rust 原生引擎（解码/缩放/编码择优全部在 Rust 完成） */
async function compress() {
  if (!origDataUrl.value) {
    return;
  }
  const token = ++compressToken;
  compressing.value = true;
  try {
    const r = await compressImageData(origDataUrl.value, buildParams());
    if (token !== compressToken) {
      return;
    }
    resultUrl.value = r.dataUrl ?? "";
    resultBytes.value = r.size;
    resultFormat.value = r.format;
    resultWidth.value = r.width;
    resultHeight.value = r.height;
    resultKept.value = r.kept;
    resultAnimated.value = r.animated;
  } catch (e) {
    if (token === compressToken) {
      ElMessage.error(String(e));
    }
  } finally {
    if (token === compressToken) {
      compressing.value = false;
    }
  }
}

function decodeImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("decode fail"));
    image.src = src;
  });
}

async function copyResult() {
  if (!resultUrl.value) {
    return;
  }
  try {
    // 解码实际压缩结果（含缩放），取出 RGBA 写入系统剪贴板
    const img = await decodeImage(resultUrl.value);
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    ctx.drawImage(img, 0, 0);
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
    await setClipboardImage({
      width: canvas.width,
      height: canvas.height,
      rgba: Array.from(data.data),
    });
    ElMessage.success(t("toolbox.tools.imgcompress.copied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
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

let compressTimer: ReturnType<typeof setTimeout> | undefined;

watch(paramsSig, () => {
  if (loaded.value) {
    clearTimeout(compressTimer);
    compressTimer = setTimeout(() => void compress(), 200);
  }
});
</script>

<template>
  <div class="view-root">
    <!-- 门户行:高级压缩 / 资料库(独立窗口) -->
    <div class="flex gap-2 mb-2.5">
      <button
        class="flex-1 flex items-center gap-2 px-2.5 py-2 rounded-[10px] bg-dp-card-bg border border-dp-card-border hover:border-dp-accent hover:bg-dp-hover-bg transition-colors text-left min-w-0"
        @click="openAdvanced"
      >
        <el-icon :size="17" class="text-dp-accent shrink-0"><MagicStick /></el-icon>
        <span class="flex flex-col min-w-0">
          <span class="text-xs font-medium text-dp-primary leading-[1.5]">
            {{ t("images.openAdvanced") }}
          </span>
          <span class="text-[10px] text-dp-tertiary leading-[1.4] truncate">
            {{ t("images.portalAdvancedSub") }}
          </span>
        </span>
      </button>
      <button
        class="flex-1 flex items-center gap-2 px-2.5 py-2 rounded-[10px] bg-dp-card-bg border border-dp-card-border hover:border-dp-accent hover:bg-dp-hover-bg transition-colors text-left min-w-0"
        @click="openManager"
      >
        <el-icon :size="17" class="text-dp-accent shrink-0"><FolderOpened /></el-icon>
        <span class="flex flex-col min-w-0">
          <span class="text-xs font-medium text-dp-primary leading-[1.5]">
            {{ t("images.openManager") }}
          </span>
          <span class="text-[10px] text-dp-tertiary leading-[1.4] truncate">
            {{ t("images.portalManagerSub") }}
          </span>
        </span>
      </button>
    </div>

    <input
      ref="fileInputRef"
      type="file"
      accept="image/*"
      class="hidden"
      @change="onFileChange"
    />

    <el-scrollbar class="view-scroll">
      <!-- 空状态:大点击区 hero -->
      <div
        v-if="!loaded"
        class="mx-0.5 p-4 bg-dp-card-bg border border-dashed border-dp-card-border rounded-[10px] flex flex-col items-center gap-1.5 cursor-pointer hover:border-dp-accent transition-colors"
        @click="fileInputRef?.click()"
      >
        <el-icon :size="28" class="text-dp-tertiary mt-1"><Picture /></el-icon>
        <span class="text-xs text-dp-muted">{{ t("images.heroTitle") }}</span>
        <span class="text-[10px] text-dp-tertiary">{{ t("images.heroHint") }}</span>
        <div class="flex gap-2 mt-1.5" @click.stop>
          <el-button size="small" type="primary" :icon="Picture" @click="pasteFromClipboard">
            {{ t("toolbox.tools.imgcompress.paste") }}
          </el-button>
          <el-button size="small" :icon="Upload" @click="fileInputRef?.click()">
            {{ t("toolbox.tools.imgcompress.choose") }}
          </el-button>
        </div>
      </div>

      <template v-else>
        <!-- 已加载:快速操作行 -->
        <div class="flex gap-2 mb-2.5">
          <el-button size="small" type="primary" :icon="Picture" @click="pasteFromClipboard">
            {{ t("toolbox.tools.imgcompress.paste") }}
          </el-button>
          <el-button size="small" :icon="Upload" @click="fileInputRef?.click()">
            {{ t("toolbox.tools.imgcompress.choose") }}
          </el-button>
        </div>
        <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px] mb-3">
          <div class="flex items-center gap-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.preset") }}</span>
            <el-select v-model="preset" size="small" class="flex-1 min-w-0">
              <el-option :label="t('toolbox.tools.imgcompress.presetBalanced')" value="balanced" />
              <el-option :label="t('toolbox.tools.imgcompress.presetHigh')" value="high" />
              <el-option :label="t('toolbox.tools.imgcompress.presetSmall')" value="small" />
              <el-option :label="t('toolbox.tools.imgcompress.presetCustom')" value="custom" />
            </el-select>
            <el-button size="small" type="primary" @click="compress">
              {{ t("toolbox.tools.imgcompress.compressBtn") }}
            </el-button>
          </div>
          <div class="flex items-center gap-2.5 mt-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.maxDim") }}</span>
            <el-radio-group v-model="maxDim" size="small">
              <el-radio-button :value="0">{{ t("toolbox.tools.imgcompress.noLimit") }}</el-radio-button>
              <el-radio-button :value="1920">1920</el-radio-button>
              <el-radio-button :value="1280">1280</el-radio-button>
              <el-radio-button :value="720">720</el-radio-button>
            </el-radio-group>
          </div>
          <div
            v-if="preset === 'custom'"
            class="flex items-center gap-2.5 mt-2.5"
          >
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.quality") }}</span>
            <el-slider
              v-model="quality"
              :min="1"
              :max="100"
              :step="1"
              class="flex-1 min-w-0"
            />
            <span class="text-xs text-dp-accent shrink-0 min-w-[36px] text-right">
              {{ quality }}%
            </span>
          </div>
          <div class="flex items-center gap-2.5 mt-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.format") }}</span>
            <el-select v-model="format" size="small" class="w-[104px]">
              <el-option :label="t('toolbox.tools.imgcompress.fmtAuto')" value="auto" />
              <el-option :label="t('toolbox.tools.imgcompress.fmtOriginal')" value="keep" />
              <el-option label="JPEG" value="image/jpeg" />
              <el-option label="PNG" value="image/png" />
              <el-option label="WebP" value="image/webp" />
            </el-select>
          </div>
        </div>

        <!-- 结果预览:透明区域棋盘底,醒目展示压缩产物 -->
        <div
          class="relative w-full rounded-[10px] border border-dp-card-border bg-dp-card-bg bg-[repeating-conic-gradient(var(--dp-divider)_0%_25%,transparent_0%_50%)] bg-[length:14px_14px] bg-center bg-no-repeat p-3 flex items-center justify-center min-h-[130px]"
        >
          <img
            v-if="resultUrl"
            :src="resultUrl"
            class="max-w-full max-h-[170px] rounded-md shadow-[0_4px_16px_rgba(31,41,55,0.18)]"
            alt="compressed"
          />
          <div v-else-if="compressing" class="flex flex-col items-center gap-1.5 py-4">
            <el-icon class="is-loading text-dp-tertiary" :size="20"><Loading /></el-icon>
            <span class="text-[10.5px] text-dp-tertiary">{{ t("toolbox.tools.imgcompress.compressing") }}</span>
          </div>
          <span
            v-if="resultAnimated && resultUrl"
            class="absolute top-2 right-2 px-1.5 py-[1px] rounded bg-black/55 text-white text-[9.5px] leading-[1.4]"
          >
            {{ t("toolbox.tools.imgcompress.animated") }}
          </span>
        </div>

        <!-- 对比卡 -->
        <div class="mt-2.5 px-2.5 py-2 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
          <div class="flex items-center justify-between text-xs leading-[1.9]">
            <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.original") }}</span>
            <span class="text-dp-muted">
              {{ origWidth }}×{{ origHeight }} · {{ kb(origBytes) }}
            </span>
          </div>
          <div class="flex items-center justify-between text-xs leading-[1.9]">
            <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.compressed") }}</span>
            <span class="text-dp-accent font-medium">
              {{ resultWidth }}×{{ resultHeight }} · {{ kb(resultBytes) }} · {{ fmtLabel }}
            </span>
          </div>
          <div class="flex justify-center mt-1">
            <span
              v-if="resultKept"
              class="px-2 py-0.5 rounded-full bg-dp-btn-bg text-dp-tertiary text-[10.5px]"
            >
              {{ t("toolbox.tools.imgcompress.keptOriginal") }}
            </span>
            <span
              v-else-if="savedPercent > 0"
              class="px-2.5 py-0.5 rounded-full bg-[var(--el-color-success-light-9,#f0f9eb)] text-[var(--el-color-success,#67c23a)] text-[11px] font-medium"
            >
              {{ t("toolbox.tools.imgcompress.saved", { n: savedPercent }) }}
            </span>
          </div>
        </div>

        <div class="flex gap-2 mt-2.5 mb-1">
          <el-button size="small" class="flex-1" :icon="CopyDocument" @click="copyResult">
            {{ t("toolbox.tools.imgcompress.copyImage") }}
          </el-button>
          <el-button size="small" class="flex-1" type="primary" plain :icon="Download" @click="saveResult">
            {{ t("toolbox.tools.imgcompress.saveImage") }}
          </el-button>
        </div>
      </template>
    </el-scrollbar>
  </div>
</template>
