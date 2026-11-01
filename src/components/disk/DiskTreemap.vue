<script setup lang="ts">
import { onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import type { LayoutTile } from "@/types";
import { DISK_CATEGORY_COLORS, withAlpha } from "@/lib/diskPalette";
import { isDark } from "@/lib/theme";

const props = defineProps<{
  tiles: LayoutTile[];
  selectedPath: string;
  filesLabel: string;
  othersLabel: (count: number) => string;
  sizeLabel: (bytes: number) => string;
}>();

const emit = defineEmits<{
  (event: "select", path: string): void;
  (event: "drill", path: string): void;
}>();

const wrapper = ref<HTMLDivElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const hoverPath = ref("");

let observer: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;

/** 视图变换（文档 5.5：布局在基础坐标系完成，缩放/平移只改视图变换，不重新布局） */
const view = reactive({ scale: 1, x: 0, y: 0 });
const MIN_SCALE = 1;
const MAX_SCALE = 16;

/** 命中测试：tiles 为先序遍历（父在子前），倒查取最深的。坐标为基础坐标系。 */
function tileAt(x: number, y: number): LayoutTile | null {
  for (let index = props.tiles.length - 1; index >= 0; index -= 1) {
    const tile = props.tiles[index];
    if (x >= tile.x && x <= tile.x + tile.w && y >= tile.y && y <= tile.y + tile.h) {
      return tile;
    }
  }
  return null;
}

function tileLabel(tile: LayoutTile): string {
  if (tile.kind === "files") {
    return props.filesLabel;
  }
  if (tile.kind === "others") {
    return props.othersLabel(tile.count ?? 0);
  }
  return tile.name;
}

function tileColor(tile: LayoutTile, alpha: number): string {
  return withAlpha(DISK_CATEGORY_COLORS[tile.category], alpha);
}

/** 选中描边用主题色。 */
function accentColor(): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue("--dp-accent").trim() ||
    "#3d8bfd"
  );
}

/** 文本放不下时按像素宽度截断（带省略号）。 */
function fitText(ctx: CanvasRenderingContext2D, label: string, maxWidth: number): string {
  if (ctx.measureText(label).width <= maxWidth) {
    return label;
  }
  let shown = "";
  for (const ch of label) {
    if (ctx.measureText(`${shown}${ch}…`).width > maxWidth) {
      break;
    }
    shown += ch;
  }
  return `${shown}…`;
}

function draw(): void {
  const canvasEl = canvas.value;
  const wrapperEl = wrapper.value;
  if (!canvasEl || !wrapperEl) {
    return;
  }
  const dpr = window.devicePixelRatio || 1;
  const width = wrapperEl.clientWidth;
  const height = wrapperEl.clientHeight;
  if (width <= 0 || height <= 0) {
    return;
  }
  canvasEl.width = Math.round(width * dpr);
  canvasEl.height = Math.round(height * dpr);
  canvasEl.style.width = `${width}px`;
  canvasEl.style.height = `${height}px`;
  const ctx = canvasEl.getContext("2d");
  if (!ctx) {
    return;
  }
  // 先按设备像素清屏（覆盖全画布），再切到"视图变换"绘制基础坐标系内容
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  const dark = isDark();
  ctx.fillStyle = dark ? "#14181f" : "#e7edf6";
  ctx.fillRect(0, 0, canvasEl.width, canvasEl.height);
  ctx.setTransform(
    dpr * view.scale,
    0,
    0,
    dpr * view.scale,
    dpr * view.x,
    dpr * view.y,
  );
  ctx.textBaseline = "middle";
  const accent = accentColor();
  const font = "-apple-system, 'PingFang SC', sans-serif";
  // 描边宽度按视图缩放折算，保持屏幕上的视觉粗细恒定
  const strokeScale = 1 / view.scale;

  for (const tile of props.tiles) {
    const headerH = tile.header ? tile.header[3] : 0;
    // 层次感：每深一层淡一档，父块实、子块虚
    const depthFade = Math.pow(0.85, tile.depth);
    const contentAlpha = (dark ? 0.44 : 0.58) * depthFade;

    // 内容区：内缩 0.5px，让底色透出 1px 间隙、色块彼此分明
    ctx.fillStyle = tileColor(tile, contentAlpha);
    ctx.fillRect(
      tile.x + 0.5,
      tile.y + headerH + 0.5,
      Math.max(0, tile.w - 1),
      Math.max(0, tile.h - headerH - 1),
    );

    if (tile.header) {
      const [hx, hy, hw, hh] = tile.header;
      // header：比内容更饱和的同色
      ctx.fillStyle = tileColor(tile, Math.min(0.92, contentAlpha + 0.36));
      ctx.fillRect(hx + 0.5, hy + 0.5, Math.max(0, hw - 1), Math.max(0, hh - 1));
      ctx.fillStyle = "#ffffff";
      ctx.font = `600 ${10.5 / strokeScale}px ${font}`;
      ctx.save();
      ctx.beginPath();
      ctx.rect(hx + 1, hy, hw - 2, hh);
      ctx.clip();
      ctx.fillText(fitText(ctx, tileLabel(tile), hw - 10 / strokeScale), hx + 5, hy + hh / 2);
      ctx.restore();
      // 尺寸标注：header 之下、面积足够时显示
      if (tile.kind === "dir" && tile.w > 96 && tile.h - hh > 44) {
        ctx.fillStyle = dark ? "rgba(255,255,255,0.72)" : "rgba(255,255,255,0.9)";
        ctx.font = `500 ${10 / strokeScale}px ${font}`;
        ctx.save();
        ctx.beginPath();
        ctx.rect(tile.x + 1, tile.y + hh, tile.w - 2, tile.h - hh);
        ctx.clip();
        ctx.fillText(props.sizeLabel(tile.bytes), tile.x + 6, tile.y + hh + 13 / strokeScale);
        ctx.restore();
      }
    } else if (tile.w > 40 && tile.h > 24) {
      // 无 header 的小 tile / 文件 / Others：只画标签（避免与 header 重复）
      ctx.fillStyle = dark ? "rgba(255,255,255,0.88)" : "rgba(255,255,255,0.95)";
      ctx.font = `600 ${10 / strokeScale}px ${font}`;
      ctx.save();
      ctx.beginPath();
      ctx.rect(tile.x + 1, tile.y, tile.w - 2, tile.h);
      ctx.clip();
      ctx.fillText(fitText(ctx, tileLabel(tile), tile.w - 10), tile.x + 5, tile.y + tile.h / 2);
      ctx.restore();
    }
  }

  // 选中/悬停描边：选中走主题色
  for (const [path, color, width] of [
    [props.selectedPath, accent, 2],
    [hoverPath.value, dark ? "rgba(255,255,255,0.55)" : "rgba(31,41,55,0.45)", 1.5],
  ] as const) {
    if (!path) {
      continue;
    }
    const tile = props.tiles.find((item) => item.path === path);
    if (!tile) {
      continue;
    }
    ctx.strokeStyle = color;
    ctx.lineWidth = width * strokeScale;
    ctx.strokeRect(
      tile.x + (width * strokeScale) / 2,
      tile.y + (width * strokeScale) / 2,
      tile.w - width * strokeScale,
      tile.h - width * strokeScale,
    );
  }
}

/** 屏幕坐标（CSS px，相对画布）→ 基础坐标（逆视图变换）。 */
function toBase(clientX: number, clientY: number): { x: number; y: number } {
  const rect = canvas.value?.getBoundingClientRect();
  if (!rect) {
    return { x: 0, y: 0 };
  }
  return {
    x: (clientX - rect.left - view.x) / view.scale,
    y: (clientY - rect.top - view.y) / view.scale,
  };
}

function clampView(): void {
  const width = wrapper.value?.clientWidth ?? 0;
  const height = wrapper.value?.clientHeight ?? 0;
  view.scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, view.scale));
  // 平移范围钳制：内容始终覆盖画布，不会拖出空白
  view.x = Math.min(0, Math.max(width - width * view.scale, view.x));
  view.y = Math.min(0, Math.max(height - height * view.scale, view.y));
}

/** 以画布上某点（CSS px）为中心缩放，保持该点内容不动。 */
function zoomAt(px: number, py: number, factor: number): void {
  const prev = view.scale;
  const next = Math.min(MAX_SCALE, Math.max(MIN_SCALE, prev * factor));
  if (next === prev) {
    return;
  }
  const applied = next / prev;
  view.x = px - (px - view.x) * applied;
  view.y = py - (py - view.y) * applied;
  view.scale = next;
  clampView();
  draw();
}

function zoomIn(): void {
  const width = wrapper.value?.clientWidth ?? 0;
  const height = wrapper.value?.clientHeight ?? 0;
  zoomAt(width / 2, height / 2, 1.25);
}

function zoomOut(): void {
  const width = wrapper.value?.clientWidth ?? 0;
  const height = wrapper.value?.clientHeight ?? 0;
  zoomAt(width / 2, height / 2, 1 / 1.25);
}

function resetView(): void {
  view.scale = 1;
  view.x = 0;
  view.y = 0;
  draw();
}

// ---- 指针交互：移动/悬停/点选/双击下钻/拖拽平移 ----

let panning = false;
let panStart: { x: number; y: number; vx: number; vy: number } | null = null;
/** 拖拽平移发生过时吞掉随后的 click，避免平移结束误选中 */
let panMoved = false;

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) {
    return;
  }
  panMoved = false;
  panStart = { x: event.clientX, y: event.clientY, vx: view.x, vy: view.y };
  try {
    canvas.value?.setPointerCapture(event.pointerId);
  } catch {
    // 捕获失败不影响交互
  }
}

function onPointerMove(event: PointerEvent) {
  const base = toBase(event.clientX, event.clientY);
  const hit = tileAt(base.x, base.y);
  hoverPath.value = hit?.path ?? "";
  if (canvas.value) {
    canvas.value.style.cursor = panning
      ? "grabbing"
      : hit && hit.kind === "dir" && hit.hasChildren
        ? "pointer"
        : "default";
  }
  if (!panStart) {
    return;
  }
  const dx = event.clientX - panStart.x;
  const dy = event.clientY - panStart.y;
  if (!panning && Math.hypot(dx, dy) > 3) {
    panning = true;
    panMoved = true;
  }
  if (panning) {
    view.x = panStart.vx + dx;
    view.y = panStart.vy + dy;
    clampView();
    draw();
  }
}

function onPointerUp(event: PointerEvent) {
  if (panning) {
    panning = false;
    if (canvas.value) {
      canvas.value.style.cursor = "default";
    }
  }
  panStart = null;
  try {
    canvas.value?.releasePointerCapture(event.pointerId);
  } catch {
    // 指针可能已释放
  }
}

function onClick(event: MouseEvent) {
  if (panMoved) {
    // 平移刚结束：吞掉这次 click，避免误选中
    panMoved = false;
    return;
  }
  const base = toBase(event.clientX, event.clientY);
  const hit = tileAt(base.x, base.y);
  emit("select", hit?.path ?? "");
}

function onDoubleClick(event: MouseEvent) {
  const base = toBase(event.clientX, event.clientY);
  const hit = tileAt(base.x, base.y);
  if (hit && hit.kind === "dir" && hit.hasChildren) {
    emit("drill", hit.path);
  }
}

/** 滚轮缩放：向指针位置缩放；Ctrl+滚轮（触板捏合）同样处理。 */
function onWheel(event: WheelEvent): void {
  event.preventDefault();
  const rect = canvas.value?.getBoundingClientRect();
  if (!rect) {
    return;
  }
  const factor = event.ctrlKey
    ? Math.exp(-event.deltaY * 0.01)
    : event.deltaY < 0
      ? 1.15
      : 1 / 1.15;
  zoomAt(event.clientX - rect.left, event.clientY - rect.top, factor);
}

function observe() {
  const wrapperEl = wrapper.value;
  if (!wrapperEl || typeof ResizeObserver === "undefined") {
    return;
  }
  observer = new ResizeObserver(() => {
    clampView();
    draw();
  });
  observer.observe(wrapperEl);
}

onMounted(() => {
  observe();
  draw();
  // 主题类切换（明暗）即时重绘
  themeObserver = new MutationObserver(() => draw());
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "style"],
  });
  // wheel 需要非被动监听才能 preventDefault（阻止页面级缩放/滚动）
  canvas.value?.addEventListener("wheel", onWheel, { passive: false });
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
  themeObserver?.disconnect();
  themeObserver = null;
  canvas.value?.removeEventListener("wheel", onWheel);
});

watch(
  () => [props.tiles, props.selectedPath, hoverPath.value],
  () => draw(),
  { deep: true },
);

defineExpose({ draw, zoomIn, zoomOut, resetView });
</script>

<template>
  <div ref="wrapper" class="relative h-full w-full overflow-hidden">
    <canvas
      ref="canvas"
      class="block"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @click="onClick"
      @dblclick="onDoubleClick"
    />
  </div>
</template>
