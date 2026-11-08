<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Monitor, VideoPause, VideoPlay } from "@element-plus/icons-vue";
import { monitorSample, monitorbarToggle } from "@/api/monitor";
import { useConfigStore } from "@/stores/config";
import { fmtBytes, fmtRate, levelOf } from "@/lib/monitorFormat";
import type { MonitorSample } from "@/types";

const { t } = useI18n();
const config = useConfigStore();

const paused = ref(false);
const loaded = ref(false);
const sample = ref<MonitorSample | null>(null);

/** 刷新间隔：工具页、悬浮条、菜单栏共用同一设置。 */
const refreshMs = computed(() => config.settings.monitor_refresh_ms ?? 2000);

async function onRefreshMsChange(value: string | number | boolean | undefined) {
  await config.setMonitorRefreshMs(Number(value));
}

let timer: ReturnType<typeof setInterval> | undefined;
// 序号守卫：慢请求返回时若已发生更新采样，直接丢弃过期结果
let seq = 0;

/** 数值文本颜色：互斥三元绑定，避免 uno class 叠加顺序不确定。 */
function levelTextClass(percent: number): string {
  const level = levelOf(percent);
  if (level === "danger") return "text-[#f56c6c]";
  if (level === "warn") return "text-[#e6a23c]";
  return "text-dp-primary";
}
/** 进度条填充颜色。 */
function levelBarClass(percent: number): string {
  const level = levelOf(percent);
  if (level === "danger") return "bg-[#f56c6c]";
  if (level === "warn") return "bg-[#e6a23c]";
  return "bg-[linear-gradient(90deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))]";
}

/** 悬浮监控条开关（调用 monitorbar_toggle 实际开/关窗口，状态经 monitorbar://state 事件同步）。 */
const barOpen = computed(() => config.settings.monitor_bar_open === true);

/** 分区开关可编辑条件：悬浮条或菜单栏任一开启即可（分区配置为两处共用）。 */
const sectionsEditable = computed(
  () => barOpen.value || config.settings.monitor_menubar_open === true
);

async function onBarChange() {
  try {
    const open = await monitorbarToggle();
    config.setMonitorBar(open);
  } catch (e) {
    ElMessage.error(t("toolbox.tools.monitor.loadFailed", { err: String(e) }));
  }
}

/** 鼠标穿透开关。 */
async function onBarClickThrough(value: string | number | boolean) {
  await config.setMonitorBarClickThrough(Boolean(value));
}

/** 分区开关（CPU/内存/网速独立配置）。 */
async function onBarSection(key: "cpu" | "mem" | "net", value: string | number | boolean) {
  await config.setMonitorBarSections({ [key]: Boolean(value) });
}

/** 菜单栏监控文字开关（仅 macOS：独立小字体状态项承载文字）。 */
const menubarAvailable = computed(() => config.platform === "macos");

async function onMenubarChange(value: string | number | boolean) {
  await config.setMonitorMenubar(Boolean(value));
}

async function onMenubarIcon(value: string | number | boolean) {
  await config.setMonitorMenubarIcon(Boolean(value));
}

const memoryPercent = computed(() => {
  const memory = sample.value?.memory;
  if (!memory || memory.total <= 0) return 0;
  return Math.min(100, (memory.used / memory.total) * 100);
});

const volumeRows = computed(() =>
  (sample.value?.volumes ?? []).map((v) => ({
    ...v,
    label: v.name && v.name !== v.mount ? `${v.name} · ${v.mount}` : v.mount,
    percent: v.total > 0 ? Math.min(100, ((v.total - v.available) / v.total) * 100) : 0,
  })),
);

function stopPolling() {
  if (timer !== undefined) {
    clearInterval(timer);
    timer = undefined;
  }
}
function startPolling() {
  stopPolling();
  timer = setInterval(() => void refresh(), refreshMs.value);
}

async function refresh() {
  const mySeq = ++seq;
  try {
    const data = await monitorSample();
    if (mySeq === seq) {
      sample.value = data;
      loaded.value = true;
    }
  } catch (e) {
    if (mySeq !== seq) return;
    // 采样失败即暂停轮询，避免错误提示刷屏
    paused.value = true;
    stopPolling();
    ElMessage.error(t("toolbox.tools.monitor.loadFailed", { err: String(e) }));
  }
}

function togglePause() {
  paused.value = !paused.value;
  if (paused.value) {
    stopPolling();
  } else {
    void refresh();
    startPolling();
  }
}

watch(refreshMs, () => {
  if (!paused.value) startPolling();
});

onMounted(() => {
  void refresh();
  startPolling();
});
onBeforeUnmount(() => stopPolling());
</script>

<template>
  <div class="view-root">
    <!-- 暂停/继续传送到工具箱标题栏右侧 -->
    <Teleport to="#toolbox-toolbar-extra">
      <span v-if="paused" class="text-[10px] text-dp-tertiary">
        {{ t("toolbox.tools.monitor.paused") }}
      </span>
      <el-button
        size="small"
        text
        class="!px-1.5"
        :icon="paused ? VideoPlay : VideoPause"
        @click="togglePause"
      >
        {{ t(paused ? "toolbox.tools.monitor.resume" : "toolbox.tools.monitor.pause") }}
      </el-button>
    </Teleport>

    <el-scrollbar class="view-scroll">
      <div class="px-2.5 pb-2">
        <!-- 处理器 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="flex items-center justify-between">
            <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.monitor.cpu") }}
            </span>
            <span
              class="text-lg font-semibold tabular-nums transition-all duration-500 motion-reduce:transition-none"
              :class="loaded && sample ? levelTextClass(sample.cpu.usage) : 'text-dp-tertiary'"
            >
              {{ loaded && sample ? `${sample.cpu.usage.toFixed(1)}%` : "--" }}
            </span>
          </div>
          <div class="mt-1 h-1.5 overflow-hidden rounded-[99px] bg-dp-btn-bg">
            <div
              class="h-full rounded-[99px] transition-[width] duration-500 ease-out motion-reduce:transition-none motion-reduce:opacity-70"
              :class="loaded && sample ? levelBarClass(sample.cpu.usage) : ''"
              :style="{ width: `${sample ? sample.cpu.usage : 0}%` }"
            />
          </div>
          <div class="mt-2 flex items-center gap-1.5">
            <span class="text-[10px] text-dp-tertiary">
              {{ t("toolbox.tools.monitor.perCore") }}
            </span>
            <span class="text-[10px] text-dp-muted tabular-nums">
              {{ sample?.cpu.cores.length ?? 0 }}
            </span>
          </div>
          <div class="mt-1 flex gap-1">
            <div
              v-for="(core, i) in sample?.cpu.cores ?? []"
              :key="i"
              class="h-9 min-w-0 flex-1 flex items-end overflow-hidden rounded-[3px] bg-dp-btn-bg"
            >
              <div
                class="w-full transition-[height] duration-500 ease-out motion-reduce:transition-none motion-reduce:opacity-70"
                :class="levelBarClass(core)"
                :style="{ height: `${Math.max(core, 3)}%` }"
              />
            </div>
          </div>
        </section>

        <!-- 内存 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="flex items-center justify-between">
            <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.monitor.memory") }}
            </span>
            <span
              class="text-lg font-semibold tabular-nums transition-all duration-500 motion-reduce:transition-none"
              :class="loaded ? levelTextClass(memoryPercent) : 'text-dp-tertiary'"
            >
              {{ loaded ? `${memoryPercent.toFixed(1)}%` : "--" }}
            </span>
          </div>
          <div class="mt-1 h-1.5 overflow-hidden rounded-[99px] bg-dp-btn-bg">
            <div
              class="h-full rounded-[99px] transition-[width] duration-500 ease-out motion-reduce:transition-none motion-reduce:opacity-70"
              :class="loaded ? levelBarClass(memoryPercent) : ''"
              :style="{ width: `${memoryPercent}%` }"
            />
          </div>
          <div class="mt-1 flex justify-between text-[10px] text-dp-muted tabular-nums">
            <span>{{ t("toolbox.tools.monitor.used") }} {{ fmtBytes(sample?.memory.used ?? 0) }}</span>
            <span>{{ t("toolbox.tools.monitor.total") }} {{ fmtBytes(sample?.memory.total ?? 0) }}</span>
          </div>
        </section>

        <!-- 网络 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
            {{ t("toolbox.tools.monitor.network") }}
          </div>
          <div class="mt-1.5 grid grid-cols-2 gap-2">
            <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
              <div class="text-[10px] text-dp-tertiary">↓ {{ t("toolbox.tools.monitor.down") }}</div>
              <div
                class="text-[15px] font-semibold text-dp-primary tabular-nums transition-all duration-500 motion-reduce:transition-none"
              >
                {{ fmtRate(sample?.network.downRate ?? 0) }}
              </div>
              <div class="mt-0.5 text-[9.5px] text-dp-muted tabular-nums">
                {{ t("toolbox.tools.monitor.totalDown") }} {{ fmtBytes(sample?.network.totalDown ?? 0) }}
              </div>
            </div>
            <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
              <div class="text-[10px] text-dp-tertiary">↑ {{ t("toolbox.tools.monitor.up") }}</div>
              <div
                class="text-[15px] font-semibold text-dp-primary tabular-nums transition-all duration-500 motion-reduce:transition-none"
              >
                {{ fmtRate(sample?.network.upRate ?? 0) }}
              </div>
              <div class="mt-0.5 text-[9.5px] text-dp-muted tabular-nums">
                {{ t("toolbox.tools.monitor.totalUp") }} {{ fmtBytes(sample?.network.totalUp ?? 0) }}
              </div>
            </div>
          </div>
        </section>

        <!-- 磁盘 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
            {{ t("toolbox.tools.monitor.volumes") }}
          </div>
          <div v-for="v in volumeRows" :key="v.mount" class="mt-1.5">
            <div class="flex items-center justify-between gap-2">
              <span class="min-w-0 truncate text-[11px] text-dp-secondary" :title="v.mount">
                {{ v.label }}
              </span>
              <span
                class="shrink-0 text-[10.5px] tabular-nums transition-all duration-500 motion-reduce:transition-none"
                :class="levelTextClass(v.percent)"
              >
                {{ v.percent.toFixed(0) }}%
              </span>
            </div>
            <div class="mt-0.5 h-1 overflow-hidden rounded-[99px] bg-dp-btn-bg">
              <div
                class="h-full rounded-[99px] transition-[width] duration-500 ease-out motion-reduce:transition-none motion-reduce:opacity-70"
                :class="levelBarClass(v.percent)"
                :style="{ width: `${v.percent}%` }"
              />
            </div>
            <div class="mt-0.5 text-[9.5px] text-dp-muted tabular-nums">
              {{ t("toolbox.tools.monitor.used") }} {{ fmtBytes(v.total - v.available) }}
              / {{ t("toolbox.tools.monitor.total") }} {{ fmtBytes(v.total) }}
            </div>
          </div>
        </section>

        <!-- 刷新间隔（工具页 / 悬浮条 / 菜单栏共用） -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="flex items-center justify-between">
            <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.monitor.interval") }}
            </span>
            <el-radio-group
              size="small"
              :model-value="refreshMs"
              @change="(v: string | number | boolean | undefined) => onRefreshMsChange(v)"
            >
              <el-radio-button :value="1000">1s</el-radio-button>
              <el-radio-button :value="2000">2s</el-radio-button>
              <el-radio-button :value="5000">5s</el-radio-button>
            </el-radio-group>
          </div>
        </section>

        <!-- 悬浮监控条 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <div class="flex items-center justify-between gap-2">
            <div class="flex min-w-0 items-center gap-1.5">
              <el-icon class="shrink-0 text-dp-accent" :size="14"><Monitor /></el-icon>
              <span class="text-[11.5px] text-dp-secondary">
                {{ t("toolbox.tools.monitor.barTitle") }}
              </span>
            </div>
            <el-switch size="small" :model-value="barOpen" @change="onBarChange" />
          </div>
          <div class="mt-1 text-[10px] leading-[1.7] text-dp-muted">
            {{ t("toolbox.tools.monitor.barDesc") }}
          </div>
          <div
            class="mt-1.5 flex items-center justify-between gap-2 transition-opacity duration-200"
            :class="barOpen ? '' : 'pointer-events-none opacity-45'"
          >
            <span
              class="text-[10.5px] text-dp-secondary"
              :title="t('toolbox.tools.monitor.clickThroughHint')"
            >
              {{ t("toolbox.tools.monitor.clickThrough") }}
            </span>
            <el-switch
              size="small"
              :model-value="config.settings.monitor_bar_click_through === true"
              @change="(v: string | number | boolean) => onBarClickThrough(v)"
            />
          </div>
          <div
            class="mt-1.5 flex items-center gap-3 transition-opacity duration-200"
            :class="sectionsEditable ? '' : 'pointer-events-none opacity-45'"
          >
            <label
              v-for="part in (['cpu', 'mem', 'net'] as const)"
              :key="part"
              class="flex cursor-pointer items-center gap-1.5"
            >
              <span class="text-[10.5px] text-dp-secondary">
                {{ t(`toolbox.tools.monitor.bar${part === "cpu" ? "Cpu" : part === "mem" ? "Mem" : "Net"}`) }}
              </span>
              <el-switch
                size="small"
                :model-value="config.settings[`monitor_bar_${part}`] !== false"
                @change="(v: string | number | boolean) => onBarSection(part, v)"
              />
            </label>
          </div>
          <!-- 菜单栏显示（仅 macOS）：小字体状态项显示在系统菜单栏 -->
          <div
            v-if="menubarAvailable"
            class="mt-1.5 flex items-center justify-between gap-2"
          >
            <div class="min-w-0">
              <div class="text-[10.5px] text-dp-secondary">
                {{ t("toolbox.tools.monitor.menubarTitle") }}
              </div>
              <div class="mt-px text-[9.5px] leading-[1.5] text-dp-muted">
                {{ t("toolbox.tools.monitor.menubarDesc") }}
              </div>
            </div>
            <el-switch
              size="small"
              :model-value="config.settings.monitor_menubar_open === true"
              @change="(v: string | number | boolean) => onMenubarChange(v)"
            />
          </div>
          <div
            v-if="menubarAvailable && config.settings.monitor_menubar_open === true"
            class="mt-1.5 flex items-center justify-between gap-2"
          >
            <span class="text-[10.5px] text-dp-secondary">
              {{ t("toolbox.tools.monitor.menubarIcon") }}
            </span>
            <el-switch
              size="small"
              :model-value="config.settings.monitor_menubar_show_icon !== false"
              @change="(v: string | number | boolean) => onMenubarIcon(v)"
            />
          </div>
        </section>
      </div>
    </el-scrollbar>
  </div>
</template>
