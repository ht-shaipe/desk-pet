<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import {
  Check,
  CopyDocument,
  Download,
  FolderOpened,
  Loading,
  Picture,
  Upload,
} from "@element-plus/icons-vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getClipboardImage,
  listImageFiles,
  readImageDataUrl,
  revealPath,
  saveCompressedImage,
  saveImage,
  setClipboardImage,
} from "@/api/toolbox";
import type { ImageFileInfo, ImagePayload } from "@/types";

const { t } = useI18n();

const fileInputRef = ref<HTMLInputElement | null>(null);

/* ---------- 原图状态 ---------- */
const sourceCanvas = ref<HTMLCanvasElement | null>(null);
const origWidth = ref(0);
const origHeight = ref(0);
/** 原图大小（字节）：上传为原文件大小，剪贴板为 PNG 等效大小 */
const origBytes = ref(0);
/** 原图编码数据（上传为原始文件，剪贴板为 PNG 编码），重编码不缩小体积时直接复用 */
const origDataUrl = ref("");
/** 原图格式是否能由 canvas 直接编码（GIF/BMP 会转成 PNG） */
const srcEncodable = ref(true);

/* ---------- 压缩参数与结果 ---------- */
/** 默认 100%：不损失画质，需要更小体积时再下调 */
const quality = ref(1);
const qualityLabel = computed(() =>
  quality.value >= 1
    ? t("toolbox.tools.imgcompress.keepQuality")
    : `${Math.round(quality.value * 100)}%`,
);
const maxDim = ref(0); // 0 = 不限制长边
/** 输出格式：original = 保持原类型，否则为具体 MIME */
const format = ref("original");
/** 原始图片 MIME（剪贴板来源为 RGBA 数据，视为 PNG） */
const srcMime = ref("image/png");

/** canvas 支持编码的图片类型 */
const KNOWN_MIMES = ["image/png", "image/jpeg", "image/webp"];
const FMT_LABELS: Record<string, string> = {
  "image/jpeg": "JPEG",
  "image/png": "PNG",
  "image/webp": "WebP",
};

/** 目标输出 MIME：保持原类型时，canvas 不支持的类型（GIF/BMP/SVG 等）回退为 PNG */
const targetMime = computed(() => {
  if (format.value !== "original") {
    return format.value;
  }
  return KNOWN_MIMES.includes(srcMime.value) ? srcMime.value : "image/png";
});
const resultUrl = ref("");
const resultBytes = ref(0);
/** 实际生成当前结果所用的 MIME（用户切换格式后、重新压缩前，与 targetMime 可能不同） */
const resultMime = ref("");
const fmtLabel = computed(() => FMT_LABELS[resultMime.value] ?? "PNG");
const resultWidth = ref(0);
const resultHeight = ref(0);
/** 当前结果是否因重编码反而变大而直接沿用原图 */
const resultKept = ref(false);

const loaded = computed(() => sourceCanvas.value !== null);
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

function normalizeMime(mime: string): string {
  const m = mime.toLowerCase();
  if (m === "image/jpg") {
    return "image/jpeg";
  }
  return KNOWN_MIMES.includes(m) ? m : "image/png";
}

function loadFromCanvas(
  img: CanvasImageSource,
  w: number,
  h: number,
  src?: { bytes: number; dataUrl: string; encodable: boolean },
) {
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
  if (src) {
    origBytes.value = src.bytes;
    origDataUrl.value = src.dataUrl;
    srcEncodable.value = src.encodable;
  } else {
    // 剪贴板：PNG 编码即为原图
    const pngUrl = canvas.toDataURL("image/png");
    origBytes.value = Math.round(pngUrl.length * 0.75);
    origDataUrl.value = pngUrl;
    srcEncodable.value = true;
  }
  resultUrl.value = "";
  resultMime.value = "";
  resultBytes.value = 0;
  resultKept.value = false;
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
  srcMime.value = normalizeMime(file.type);
  const reader = new FileReader();
  reader.onload = () => {
    const dataUrl = String(reader.result ?? "");
    const image = new Image();
    image.onload = () => {
      loadFromCanvas(image, image.naturalWidth, image.naturalHeight, {
        bytes: file.size,
        dataUrl,
        encodable: KNOWN_MIMES.includes(file.type.toLowerCase()),
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
    srcMime.value = "image/png";
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
  const mime = targetMime.value;
  const r = renderToDataUrl(source, source.width, source.height, mime);
  if (!r.url) {
    return;
  }
  // 保持原格式且未缩放时，如果重编码反而变大，直接沿用原图，避免“压缩”后体积增加
  const noResize = r.w === origWidth.value && r.h === origHeight.value;
  if (
    mime === srcMime.value &&
    srcEncodable.value &&
    noResize &&
    origDataUrl.value &&
    r.bytes >= origBytes.value
  ) {
    resultUrl.value = origDataUrl.value;
    resultBytes.value = origBytes.value;
    resultMime.value = srcMime.value;
    resultWidth.value = origWidth.value;
    resultHeight.value = origHeight.value;
    resultKept.value = true;
    return;
  }
  resultKept.value = false;
  resultUrl.value = r.url;
  resultBytes.value = r.bytes;
  resultMime.value = mime;
  resultWidth.value = r.w;
  resultHeight.value = r.h;
}

/** 按当前画质/长边设置将图像渲染并编码为 dataURL（单图与批量压缩共用） */
function renderToDataUrl(
  source: CanvasImageSource,
  sw: number,
  sh: number,
  mime: string,
): { url: string; bytes: number; w: number; h: number } {
  let w = sw;
  let h = sh;
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
    return { url: "", bytes: 0, w, h };
  }
  // JPEG 不支持透明，铺白底避免透明区域变黑；PNG/WebP 保留透明
  if (mime === "image/jpeg") {
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, w, h);
  }
  ctx.drawImage(source, 0, 0, w, h);
  const url = canvas.toDataURL(mime, quality.value);
  return { url, bytes: Math.round(url.length * 0.75), w, h };
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
  // 与展示结果保持一致：JPEG 补白底，PNG/WebP 保留透明（剪贴板 RGBA 支持 alpha）
  if (resultMime.value === "image/jpeg") {
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  }
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

/* ---------- 目录批量压缩 ---------- */
const batchDir = ref("");
const batchFiles = ref<ImageFileInfo[]>([]);
const batchBusy = ref(false);
const batchEstimating = ref(false);
/** 当前预估结果是否已执行过批量压缩（用于底部完成信息） */
const batchRan = ref(false);
/** 单行：预估结果（失败时仅保留错误信息） */
interface BatchRow {
  name: string;
  mime: string;
  srcBytes: number;
  outBytes: number;
  /** 预计节省百分比，负值表示变大 */
  percent: number;
  estimating: boolean;
  saved: boolean;
  /** 重编码变大，保留原图不写入 */
  kept: boolean;
  failed: boolean;
  error: string;
  outName: string;
}
const batchRows = ref<BatchRow[]>([]);

/** 目录模式下参数设置卡需要可见（与单图共用画质/长边/格式） */
const settingsVisible = computed(() => loaded.value || batchDir.value !== "");

/** 输出全为 PNG 时画质不生效（用于提示） */
const pngNoQuality = computed(() => {
  if (format.value === "image/png") {
    return true;
  }
  if (format.value !== "original") {
    return false;
  }
  if (batchDir.value) {
    return batchFiles.value.every((f) => batchTargetMime(f.name) === "image/png");
  }
  return targetMime.value === "image/png";
});

const batchSavedCount = computed(() => batchRows.value.filter((r) => r.saved).length);
const batchKeptCount = computed(() => batchRows.value.filter((r) => r.kept).length);
/** 已有预估结果的行（估算中与失败的不计入汇总） */
const batchReadyRows = computed(() =>
  batchRows.value.filter((r) => !r.failed && !r.estimating),
);
const batchSrcTotal = computed(() =>
  batchReadyRows.value.reduce((s, r) => s + r.srcBytes, 0),
);
const batchOutTotal = computed(() =>
  batchReadyRows.value.reduce((s, r) => s + r.outBytes, 0),
);
/** 预计节省百分比，负值表示压缩后变大 */
const batchPercent = computed(() =>
  batchSrcTotal.value
    ? Math.round((1 - batchOutTotal.value / batchSrcTotal.value) * 100)
    : 0,
);
/** 全部图片都已按当前设置保持原图 */
const batchAllOptimal = computed(
  () => batchReadyRows.value.length > 0 && batchReadyRows.value.every((r) => r.kept),
);

/** 批量输出文件名的扩展名，与 files.rs decode_data_url 的映射保持一致（jpeg→jpg） */
function extOfMime(mime: string): string {
  return mime === "image/jpeg" ? "jpg" : (mime.split("/")[1] ?? "jpg");
}

/** 批量模式下每张图的目标 MIME：显式选择用同一格式；保持原格式时按文件扩展名 */
function batchTargetMime(name: string): string {
  if (format.value !== "original") {
    return format.value;
  }
  const ext = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
  return normalizeMime(`image/${ext}`);
}

/** 可被 canvas 直接编码的源格式（GIF/BMP 不计入，因为输出会换成 PNG） */
function encodableSourceMime(name: string): string | null {
  const ext = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
  if (ext === "jpg" || ext === "jpeg") {
    return "image/jpeg";
  }
  if (ext === "png") {
    return "image/png";
  }
  if (ext === "webp") {
    return "image/webp";
  }
  return null;
}

function decodeImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("decode fail"));
    image.src = src;
  });
}

function newBatchRow(file: ImageFileInfo): BatchRow {
  const mime = batchTargetMime(file.name);
  const stem = file.name.replace(/\.[^.]*$/, "");
  return {
    name: file.name,
    mime,
    srcBytes: file.size,
    outBytes: 0,
    percent: 0,
    estimating: true,
    saved: false,
    kept: false,
    failed: false,
    error: "",
    outName: `${stem}.min.${extOfMime(mime)}`,
  };
}

/** 估算代号：参数变化或重新选目录时，旧任务自动作废 */
let estimateToken = 0;

async function estimateRow(row: BatchRow, token: number) {
  const dir = batchDir.value;
  try {
    const dataUrl = await readImageDataUrl(`${dir}/${row.name}`);
    if (token !== estimateToken) {
      return;
    }
    const img = await decodeImage(dataUrl);
    if (token !== estimateToken) {
      return;
    }
    const r = renderToDataUrl(img, img.naturalWidth, img.naturalHeight, row.mime);
    // 保持原格式且未缩放时，若重编码反而变大，保留原图（不写入）
    const sameFormat = encodableSourceMime(row.name) === row.mime;
    const noResize = r.w === img.naturalWidth && r.h === img.naturalHeight;
    if (sameFormat && noResize && r.bytes >= row.srcBytes) {
      row.kept = true;
      row.outBytes = row.srcBytes;
      row.percent = 0;
    } else {
      row.outBytes = r.bytes;
      row.percent =
        row.srcBytes > 0 && r.bytes > 0
          ? Math.round((1 - r.bytes / row.srcBytes) * 100)
          : 0;
    }
  } catch (e) {
    row.failed = true;
    row.error = String(e);
  }
  row.estimating = false;
}

/** 按当前画质/长边/格式预估目录内每张图的压缩后体积（3 路并发读取解码） */
async function estimateAll() {
  if (!batchDir.value || batchBusy.value) {
    return;
  }
  const token = ++estimateToken;
  batchEstimating.value = true;
  batchRan.value = false;
  const rows = batchFiles.value.map(newBatchRow);
  batchRows.value = rows;
  let cursor = 0;
  const worker = async () => {
    while (cursor < rows.length && token === estimateToken) {
      await estimateRow(rows[cursor++], token);
      if (token !== estimateToken) {
        return;
      }
      batchRows.value = [...rows];
    }
  };
  await Promise.all([worker(), worker(), worker()]);
  if (token === estimateToken) {
    batchEstimating.value = false;
  }
}

let estimateTimer: ReturnType<typeof setTimeout> | undefined;

/** 参数拖动时延迟重新预估，避免编码过密 */
function scheduleEstimate() {
  if (!batchDir.value || batchBusy.value) {
    return;
  }
  clearTimeout(estimateTimer);
  estimateTimer = setTimeout(() => void estimateAll(), 300);
}

let compressTimer: ReturnType<typeof setTimeout> | undefined;

watch([quality, maxDim, format], () => {
  if (batchDir.value) {
    scheduleEstimate();
  }
  if (loaded.value) {
    clearTimeout(compressTimer);
    compressTimer = setTimeout(() => compress(), 200);
  }
});

async function pickBatchDir() {
  if (batchBusy.value) {
    return;
  }
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      title: t("toolbox.tools.imgcompress.batchDirTitle"),
    });
    if (typeof picked !== "string" || !picked) {
      return;
    }
    const files = await listImageFiles(picked);
    // 作废上一目录未完成的预估任务
    estimateToken++;
    batchEstimating.value = false;
    batchRan.value = false;
    batchDir.value = picked;
    batchFiles.value = files;
    batchRows.value = [];
    if (!files.length) {
      ElMessage.warning(t("toolbox.tools.imgcompress.batchEmpty"));
      return;
    }
    await estimateAll();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function runBatch() {
  const dir = batchDir.value;
  if (!dir || batchBusy.value || batchEstimating.value) {
    return;
  }
  const rows = batchRows.value.filter((r) => !r.failed);
  if (!rows.length) {
    return;
  }
  batchBusy.value = true;
  // 保存期间参数变化不再重新预估，待完成后由 watch 接手
  estimateToken++;
  for (const row of rows) {
    // 预估已确定“保持原图”的行不写入，避免生成更大的副本
    if (row.kept) {
      continue;
    }
    try {
      const dataUrl = await readImageDataUrl(`${dir}/${row.name}`);
      const img = await decodeImage(dataUrl);
      const r = renderToDataUrl(img, img.naturalWidth, img.naturalHeight, row.mime);
      row.outBytes = r.bytes;
      row.percent =
        row.srcBytes > 0 && r.bytes > 0
          ? Math.round((1 - r.bytes / row.srcBytes) * 100)
          : 0;
      await saveCompressedImage(dir, row.name, r.url);
      row.saved = true;
    } catch (e) {
      row.failed = true;
      row.error = String(e);
    }
    batchRows.value = [...batchRows.value];
  }
  const saved = batchSavedCount.value;
  const kept = batchKeptCount.value;
  const ok = saved + kept;
  batchBusy.value = false;
  batchRan.value = true;
  if (ok > 0) {
    if (saved > 0) {
      revealPath(`${dir}/compressed`).catch(() => undefined);
    }
    ElMessage.success(
      kept > 0
        ? t("toolbox.tools.imgcompress.batchDoneKept", { ok, n: rows.length, k: kept })
        : t("toolbox.tools.imgcompress.batchDone", { ok, n: rows.length }),
    );
  } else {
    ElMessage.error(t("toolbox.tools.imgcompress.batchDone", { ok, n: rows.length }));
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
      <el-button size="small" :icon="FolderOpened" :disabled="batchBusy" @click="pickBatchDir">
        {{ t("toolbox.tools.imgcompress.batchBtn") }}
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
      <template v-if="settingsVisible">
        <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px] mb-3">
          <div class="flex items-center gap-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.quality") }}</span>
            <el-slider
              v-model="quality"
              :min="0.1"
              :max="1"
              :step="0.05"
              class="flex-1 min-w-0"
            />
            <span class="text-xs text-dp-accent shrink-0 min-w-[46px] text-right">{{ qualityLabel }}</span>
          </div>
          <div
            v-if="quality < 1 && pngNoQuality"
            class="mt-1 text-[10px] leading-[1.5] text-dp-tertiary"
          >
            {{ t("toolbox.tools.imgcompress.pngLossless") }}
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
          <div class="flex items-center gap-2.5 mt-2.5">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.format") }}</span>
            <el-select v-model="format" size="small" class="w-[118px]">
              <el-option :label="t('toolbox.tools.imgcompress.fmtOriginal')" value="original" />
              <el-option label="JPEG" value="image/jpeg" />
              <el-option label="PNG" value="image/png" />
              <el-option label="WebP" value="image/webp" />
            </el-select>
            <el-button v-if="loaded" size="small" type="primary" class="ml-auto" @click="compress">
              {{ t("toolbox.tools.imgcompress.compressBtn") }}
            </el-button>
          </div>
        </div>

        <div v-if="loaded" class="flex flex-col items-center">
          <img
            :src="resultUrl"
            class="max-w-full max-h-[150px] rounded-lg shadow-[0_4px_16px_rgba(31,41,55,0.12)]"
            alt="compressed"
          />
          <div class="w-full mt-3 px-2.5 py-2 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
            <div class="flex justify-between text-xs leading-[1.8]">
              <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.original") }}</span>
              <span class="text-dp-muted">
                {{ origWidth }}×{{ origHeight }} · {{ kb(origBytes) }}
              </span>
            </div>
            <div class="flex justify-between text-xs leading-[1.8]">
              <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.compressed") }}</span>
              <span class="text-dp-accent font-medium">
                {{ resultWidth }}×{{ resultHeight }} · {{ kb(resultBytes) }} · {{ fmtLabel }}
              </span>
            </div>
            <div
              v-if="resultKept"
              class="mt-1 text-[11px] text-dp-tertiary text-center"
            >
              {{ t("toolbox.tools.imgcompress.keptOriginal") }}
            </div>
            <div
              v-else-if="savedPercent > 0"
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

        <div v-if="batchDir" class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px] mb-3">
          <div class="flex items-center gap-2">
            <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.batchDir") }}</span>
            <span class="text-xs text-dp-tertiary truncate flex-1 min-w-0" :title="batchDir">
              {{ batchDir }}
            </span>
            <el-button
              size="small"
              type="primary"
              class="ml-auto"
              :loading="batchBusy"
              :disabled="batchEstimating || !batchRows.length"
              @click="runBatch"
            >
              {{ batchBusy ? t("toolbox.tools.imgcompress.batchRunning") : t("toolbox.tools.imgcompress.batchRun") }}
            </el-button>
          </div>
          <div class="mt-1 text-[10.5px] text-dp-tertiary">
            {{ t("toolbox.tools.imgcompress.batchHint") }}
          </div>

          <template v-if="batchRows.length">
            <!-- 预计：当前参数下的总量预估 -->
            <div class="mt-1.5 flex items-center justify-between gap-2 text-[11px]">
              <span class="text-dp-muted shrink-0">
                {{ t("toolbox.tools.imgcompress.batchFiles", { n: batchRows.length }) }}
              </span>
              <span v-if="batchEstimating" class="text-dp-tertiary shrink-0">
                {{ t("toolbox.tools.imgcompress.batchEstimating") }}
              </span>
              <span
                v-else-if="batchPercent > 0"
                class="text-[var(--el-color-success,#67c23a)] font-medium shrink-0"
              >
                {{ t("toolbox.tools.imgcompress.batchEstimateTotal", {
                  s: kb(batchSrcTotal),
                  d: kb(batchOutTotal),
                  n: batchPercent,
                }) }}
              </span>
              <span v-else-if="batchPercent < 0" class="text-dp-tertiary shrink-0">
                {{ t("toolbox.tools.imgcompress.batchEstimateGrow", {
                  s: kb(batchSrcTotal),
                  d: kb(batchOutTotal),
                  n: -batchPercent,
                }) }}
              </span>
              <span v-else-if="batchAllOptimal" class="text-dp-tertiary shrink-0">
                {{ t("toolbox.tools.imgcompress.batchAllOptimal") }}
              </span>
            </div>
            <div
              v-for="row in batchRows"
              :key="row.name"
              class="mt-1 flex items-center gap-1.5 text-[11px] leading-[1.8] min-w-0"
            >
              <el-icon
                v-if="row.saved"
                class="shrink-0 text-[var(--el-color-success,#67c23a)]"
                :size="11"
                :title="t('toolbox.tools.imgcompress.batchSaved')"
              >
                <Check />
              </el-icon>
              <span v-else class="w-[11px] shrink-0" />
              <span class="text-dp-muted truncate flex-1 min-w-0" :title="row.name">
                {{ row.name }}
              </span>
              <template v-if="row.failed">
                <span
                  class="text-[var(--el-color-danger,#f56c6c)] truncate shrink-0 max-w-[50%]"
                  :title="row.error"
                >
                  {{ t("toolbox.tools.imgcompress.batchFail") }}
                </span>
              </template>
              <el-icon v-else-if="row.estimating" class="is-loading shrink-0" :size="11">
                <Loading />
              </el-icon>
              <template v-else-if="row.kept">
                <span class="text-dp-tertiary shrink-0">{{ kb(row.srcBytes) }}</span>
                <span class="text-dp-tertiary shrink-0 text-[10px]">
                  {{ t("toolbox.tools.imgcompress.batchKept") }}
                </span>
              </template>
              <template v-else>
                <span class="text-dp-tertiary shrink-0">{{ kb(row.srcBytes) }}</span>
                <span class="text-dp-tertiary shrink-0">→</span>
                <span class="text-dp-primary font-medium shrink-0">{{ kb(row.outBytes) }}</span>
                <span
                  v-if="row.percent > 0"
                  class="text-[var(--el-color-success,#67c23a)] shrink-0"
                >
                  -{{ row.percent }}%
                </span>
                <span
                  v-else-if="row.percent < 0"
                  class="text-[var(--el-color-danger,#f56c6c)] shrink-0"
                >
                  +{{ -row.percent }}%
                </span>
              </template>
            </div>
            <div
              v-if="!batchBusy && batchRan && (batchSavedCount || batchKeptCount)"
              class="mt-1.5 pt-1.5 border-t border-dp-divider flex justify-between gap-2 text-[11px]"
            >
              <span class="text-dp-muted shrink-0">
                {{ t("toolbox.tools.imgcompress.batchDone", {
                  ok: batchSavedCount + batchKeptCount,
                  n: batchRows.length,
                }) }}
              </span>
              <span v-if="batchKeptCount" class="text-dp-tertiary shrink-0">
                {{ t("toolbox.tools.imgcompress.batchKeptSummary", { k: batchKeptCount }) }}
              </span>
            </div>
          </template>
          <div v-else class="mt-1.5 text-[11px] text-dp-muted">
            {{ t("toolbox.tools.imgcompress.batchFiles", { n: batchFiles.length }) }}
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

