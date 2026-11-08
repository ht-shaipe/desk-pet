<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Close, Lock } from "@element-plus/icons-vue";
import { currentMonitor, getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { monitorSample } from "@/api/monitor";
import { fmtRate, levelOf } from "@/lib/monitorFormat";
import type { MonitorSample } from "@/types";

const { t } = useI18n();

const win = getCurrentWindow();

const EDGE_MARGIN = 16;
const DEFAULT_BAR_WIDTH = 220;
const POS_KEY = "monitorbar:pos";

const sample = ref<MonitorSample | null>(null);
const clickThrough = ref(false);
/** 刷新间隔（与主窗口设置实时同步，工具页/悬浮条/菜单栏共用） */
const refreshMs = ref(2000);
/** 分区开关（与主窗口设置实时同步） */
const showCpu = ref(true);
const showMem = ref(true);
const showNet = ref(true);

const barEl = ref<HTMLElement | null>(null);

// 序号守卫：慢请求返回时若已发生更新采样，直接丢弃过期结果
let seq = 0;
let timer: ReturnType<typeof setInterval> | undefined;
let moveTimer: ReturnType<typeof setTimeout> | undefined;
const unlisteners: UnlistenFn[] = [];

const usage = computed(() => sample.value?.cpu.usage ?? 0);
const memPercent = computed(() => {
  const memory = sample.value?.memory;
  if (!memory || memory.total <= 0) return 0;
  return Math.min(100, (memory.used / memory.total) * 100);
});

/** 数值颜色：常态白，超阈值转三色报警（≥85 红 / ≥60 黄）。 */
function valueClass(percent: number): string {
  const level = levelOf(percent);
  if (level === "danger") return "text-[#f56c6c]";
  if (level === "warn") return "text-[#e6a23c]";
  return "text-white";
}

async function refresh() {
  const mySeq = ++seq;
  try {
    const data = await monitorSample();
    if (mySeq === seq) {
      sample.value = data;
    }
  } catch {
    // 悬浮条静默失败，下轮重试（不打 ElMessage，避免小窗刷提示）
  }
}

/** 按当前间隔重启轮询。 */
function startTimer() {
  if (timer !== undefined) {
    clearInterval(timer);
    timer = undefined;
  }
  timer = setInterval(() => void refresh(), refreshMs.value);
}

/** 收到主窗口推送的刷新间隔变化。 */
async function applyRefreshInterval(ms: number) {
  refreshMs.value = ms > 0 ? ms : 2000;
  startTimer();
}

/* ---------- 尺寸自适应：分区开关/穿透指示变化后按实际渲染尺寸收紧窗口 ---------- */

async function syncSize() {
  await nextTick();
  const el = barEl.value;
  if (!el) {
    return;
  }
  const w = Math.ceil(el.offsetWidth);
  const h = Math.ceil(el.offsetHeight);
  if (w > 0 && h > 0) {
    try {
      await win.setSize(new LogicalSize(w, h));
    } catch {
      // 窗口可能已被销毁
    }
  }
}
watch([showCpu, showMem, showNet, clickThrough], () => {
  void syncSize();
});

/* ---------- 位置记忆（localStorage，逻辑坐标） ---------- */

function clampPos(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(0, max));
}

async function restorePosition() {
  let saved: { x: number; y: number } | null = null;
  try {
    const raw = localStorage.getItem(POS_KEY);
    if (raw) saved = JSON.parse(raw);
  } catch {
    saved = null;
  }
  const monitor = await currentMonitor();
  const scale = monitor?.scaleFactor || 1;
  const monW = (monitor?.size.width ?? 0) / scale;
  const monH = (monitor?.size.height ?? 0) / scale;
  const barW = barEl.value?.offsetWidth ?? DEFAULT_BAR_WIDTH;
  const x = saved && Number.isFinite(saved.x) ? saved.x : monW - barW - EDGE_MARGIN;
  const y = saved && Number.isFinite(saved.y) ? saved.y : EDGE_MARGIN + 24;
  const clampedX = clampPos(x, 0, monW - barW);
  const clampedY = clampPos(y, 0, monH - (barEl.value?.offsetHeight ?? 36));
  await win.setPosition(new LogicalPosition(Math.round(clampedX), Math.round(clampedY)));
}

/** 移动结束（防抖）后把逻辑坐标存回 localStorage。 */
function scheduleSavePosition(payload: { x: number; y: number }) {
  if (moveTimer !== undefined) {
    clearTimeout(moveTimer);
  }
  moveTimer = setTimeout(async () => {
    try {
      const monitor = await currentMonitor();
      const scale = monitor?.scaleFactor || 1;
      localStorage.setItem(
        POS_KEY,
        JSON.stringify({ x: Math.round(payload.x / scale), y: Math.round(payload.y / scale) }),
      );
    } catch {
      // 取不到显示器信息时跳过本次记录
    }
  }, 400);
}

/* ---------- 点击穿透 ---------- */

async function applyClickThrough(value: boolean) {
  clickThrough.value = value;
  try {
    await win.setIgnoreCursorEvents(value);
  } catch {
    // 权限或时序问题：保持可点击
    clickThrough.value = false;
  }
}

/* ---------- 关闭（先广播状态，主窗口据此持久化开关） ---------- */

async function closeBar() {
  try {
    await emit("monitorbar://state", false);
  } catch {
    // 主窗口可能不可达，仍然关闭自身
  }
  try {
    await win.close();
  } catch {
    // 窗口可能已被销毁
  }
}

onMounted(async () => {
  // 外观与配置同步：语言、穿透、分区开关（只读设置，不回写）
  try {
    const settings = await getSettings();
    if (settings.locale === "zh-CN" || settings.locale === "en") {
      setAppLocale(settings.locale);
    }
    showCpu.value = settings.monitor_bar_cpu !== false;
    showMem.value = settings.monitor_bar_mem !== false;
    showNet.value = settings.monitor_bar_net !== false;
    refreshMs.value = settings.monitor_refresh_ms == null ? 2000 : settings.monitor_refresh_ms;
    void applyClickThrough(settings.monitor_bar_click_through === true);
  } catch {
    // 读取失败时保持默认外观
  }
  unlisteners.push(
    await listen<boolean>("monitorbar://click-through", (event) => {
      void applyClickThrough(event.payload);
    }),
    await listen<{ cpu: boolean; mem: boolean; net: boolean }>("monitorbar://sections", (event) => {
      showCpu.value = event.payload.cpu;
      showMem.value = event.payload.mem;
      showNet.value = event.payload.net;
    }),
    await listen<number>("monitorbar://refresh", (event) => {
      void applyRefreshInterval(event.payload);
    }),
    await listen<string>("i18n://locale-changed", (event) => {
      if (event.payload === "zh-CN" || event.payload === "en") {
        setAppLocale(event.payload);
      }
    }),
  );
  win.onMoved((event) => scheduleSavePosition(event.payload));
  await restorePosition();
  void syncSize();
  void refresh();
  startTimer();
});

onBeforeUnmount(() => {
  if (timer !== undefined) {
    clearInterval(timer);
    timer = undefined;
  }
  if (moveTimer !== undefined) {
    clearTimeout(moveTimer);
    moveTimer = undefined;
  }
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
});
</script>

<template>
  <div class="flex h-screen w-screen items-center overflow-hidden">
    <!-- 悬浮监控条：紧凑单行卡片，分区可配置；整块可拖动 -->
    <div
      ref="barEl"
      data-tauri-drag-region
      class="group relative flex items-center gap-1.5 rounded-[8px] border border-white/10 bg-[rgba(40,39,36,0.72)] py-[3px] px-2 shadow-[0_2px_10px_rgba(0,0,0,0.25)] backdrop-blur-md select-none"
    >
      <el-icon v-if="clickThrough" class="shrink-0 text-white/40" :size="11" data-tauri-drag-region>
        <Lock />
      </el-icon>

      <template v-if="showCpu">
        <div data-tauri-drag-region class="flex min-w-[34px] flex-col items-center">
          <span
            class="text-[12px] font-semibold leading-[1.2] tabular-nums transition-colors duration-300 motion-reduce:transition-none"
            :class="sample ? valueClass(usage) : 'text-white/40'"
          >
            <template v-if="sample">{{ Math.round(usage) }}<span class="ml-px text-[8.5px] font-medium text-white/50">%</span></template>
            <template v-else>--</template>
          </span>
          <span class="text-[8.5px] leading-[1.2] tracking-[0.08em] text-white/45">CPU</span>
        </div>
        <div v-if="showMem || showNet" class="h-5 w-px shrink-0 bg-white/10" />
      </template>

      <template v-if="showMem">
        <div data-tauri-drag-region class="flex min-w-[34px] flex-col items-center">
          <span
            class="text-[12px] font-semibold leading-[1.2] tabular-nums transition-colors duration-300 motion-reduce:transition-none"
            :class="sample ? valueClass(memPercent) : 'text-white/40'"
          >
            <template v-if="sample">{{ Math.round(memPercent) }}<span class="ml-px text-[8.5px] font-medium text-white/50">%</span></template>
            <template v-else>--</template>
          </span>
          <span class="text-[8.5px] leading-[1.2] tracking-[0.08em] text-white/45">MEM</span>
        </div>
        <div v-if="showNet" class="h-5 w-px shrink-0 bg-white/10" />
      </template>

      <template v-if="showNet">
        <div data-tauri-drag-region class="flex min-w-[72px] flex-col justify-center gap-px">
          <span class="text-[9px] leading-[1.3] text-white/85 tabular-nums">
            <span class="text-[#8ab4f8]">↑</span> {{ fmtRate(sample?.network.upRate ?? 0) }}
          </span>
          <span class="text-[9px] leading-[1.3] text-white/85 tabular-nums">
            <span class="text-[#81c995]">↓</span> {{ fmtRate(sample?.network.downRate ?? 0) }}
          </span>
        </div>
      </template>

      <!-- 关闭按钮：悬停浮现，悬浮在右缘（不占布局空间，条宽不含它） -->
      <button
        class="absolute top-1/2 right-[3px] z-10 flex h-5 w-5 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-0 bg-transparent p-0 text-white/40 opacity-0 transition-opacity duration-150 hover:bg-white/10 hover:text-white/90 group-hover:opacity-100 motion-reduce:transition-none"
        :title="t('common.close')"
        @click="closeBar"
      >
        <el-icon :size="11"><Close /></el-icon>
      </button>
    </div>
  </div>
</template>
