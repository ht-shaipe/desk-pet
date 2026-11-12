<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Refresh, RefreshRight } from "@element-plus/icons-vue";
import { aiUsageQuery, aiUsageRefresh } from "@/api/aiUsage";
import { formatCompactTokens, localDayStartMs } from "@/stores/aiUsage";
import type { AiUsageRangeResult, AiUsageRow } from "@/types";

const { t } = useI18n();

type RangeKey = "today" | "yesterday" | "week" | "month";

const range = ref<RangeKey>("today");
const refreshing = ref(false);
const result = ref<AiUsageRangeResult | null>(null);

// 序号守卫：切范围/刷新时丢弃过期响应
let seq = 0;

/** 本周一 00:00（本地时区） */
function localWeekStartMs(): number {
  const now = new Date();
  const day = (now.getDay() + 6) % 7; // 周一=0
  return localDayStartMs(new Date(now.getFullYear(), now.getMonth(), now.getDate() - day));
}

function rangeFromTo(key: RangeKey): { from: number; to: number } {
  const todayStart = localDayStartMs();
  const day = 24 * 3600 * 1000;
  switch (key) {
    case "today":
      return { from: todayStart, to: todayStart + day };
    case "yesterday":
      return { from: todayStart - day, to: todayStart };
    case "week":
      return { from: localWeekStartMs(), to: Date.now() };
    case "month":
      return {
        from: localDayStartMs(new Date(new Date().getFullYear(), new Date().getMonth(), 1)),
        to: Date.now(),
      };
  }
}

/** 估算成本展示：未知价格为 "—"（绝不冒充成本） */
function fmtUsd(cost: number | null): string {
  if (cost === null) return "—";
  return `$${cost.toFixed(4)}`;
}

function tokensOf(row: AiUsageRow): number {
  return row.input + row.output + row.cache_read + row.cache_write;
}

async function load() {
  const mySeq = ++seq;
  try {
    const { from, to } = rangeFromTo(range.value);
    const data = await aiUsageQuery(from, to);
    if (mySeq === seq) result.value = data;
  } catch (e) {
    if (mySeq === seq) {
      result.value = null;
      ElMessage.error(t("toolbox.tools.aiusage.loadFailed", { err: String(e) }));
    }
  }
}

/** 手动刷新：先增量扫描日志再查询 */
async function onRefresh() {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    await aiUsageRefresh();
    await load();
  } catch (e) {
    ElMessage.error(t("toolbox.tools.aiusage.loadFailed", { err: String(e) }));
  } finally {
    refreshing.value = false;
  }
}

function onRangeChange(value: string | number | boolean | undefined) {
  range.value = value as RangeKey;
  void load();
}

const totals = computed(() => result.value?.totals ?? null);
const byTool = computed(() => result.value?.by_tool ?? []);
const byModel = computed(() => result.value?.by_model ?? []);
const byProject = computed(() => result.value?.by_project ?? []);
/** 空态区分：库里无任何事件（未检测到日志） */
const noData = computed(() => result.value !== null && !result.value.has_any);
/** 该时段无用量（有数据但区间为空） */
const noRangeData = computed(
  () =>
    result.value !== null &&
    result.value.has_any &&
    totals.value !== null &&
    tokensOf(totals.value) === 0,
);

const rangeOptions: { key: RangeKey; labelKey: string }[] = [
  { key: "today", labelKey: "toolbox.tools.aiusage.today" },
  { key: "yesterday", labelKey: "toolbox.tools.aiusage.yesterday" },
  { key: "week", labelKey: "toolbox.tools.aiusage.week" },
  { key: "month", labelKey: "toolbox.tools.aiusage.month" },
];

// 后端工具键 -> i18n 键；未知键原样回显
const TOOL_NAME_KEYS: Record<string, string> = {
  claude_code: "toolbox.tools.aiusage.toolName.claude",
  codex_cli: "toolbox.tools.aiusage.toolName.codex",
  opencode: "toolbox.tools.aiusage.toolName.opencode",
};
const toolNameKey = (key: string) => TOOL_NAME_KEYS[key] ?? key;

onMounted(() => {
  void onRefresh();
});
</script>

<template>
  <div class="view-root">
    <!-- 手动刷新传送到工具箱标题栏右侧 -->
    <Teleport to="#toolbox-toolbar-extra">
      <el-button
        size="small"
        text
        class="!px-1.5"
        :icon="refreshing ? Refresh : RefreshRight"
        :loading="refreshing"
        @click="onRefresh"
      >
        {{ t("toolbox.tools.aiusage.refresh") }}
      </el-button>
    </Teleport>

    <el-scrollbar class="view-scroll">
      <div class="px-2.5 pb-2">
        <!-- 时间范围 -->
        <section class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]">
          <el-radio-group
            size="small"
            :model-value="range"
            @change="(v: string | number | boolean | undefined) => onRangeChange(v)"
          >
            <el-radio-button
              v-for="opt in rangeOptions"
              :key="opt.key"
              :value="opt.key"
            >
              {{ t(opt.labelKey) }}
            </el-radio-button>
          </el-radio-group>
        </section>

        <!-- 未检测到日志 -->
        <section
          v-if="noData"
          class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-6 text-center"
        >
          <div class="text-[11.5px] text-dp-secondary">
            {{ t("toolbox.tools.aiusage.empty") }}
          </div>
          <div class="mt-1.5 text-[10px] leading-[1.7] text-dp-muted">
            {{ t("toolbox.tools.aiusage.emptyHint") }}
          </div>
        </section>

        <template v-else>
          <!-- 总量 -->
          <section
            class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]"
          >
            <div class="flex items-center justify-between">
              <span class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
                {{ t("toolbox.tools.aiusage.total") }}
              </span>
              <span class="text-[10.5px] text-dp-muted tabular-nums">
                {{ t("toolbox.tools.aiusage.cost") }}
                {{ totals ? fmtUsd(totals.cost_usd) : "--" }}
              </span>
            </div>
            <div
              class="mt-0.5 text-lg font-semibold text-dp-primary tabular-nums transition-all duration-500 motion-reduce:transition-none"
            >
              {{ totals ? formatCompactTokens(tokensOf(totals)) : "--" }}
            </div>
            <div class="mt-1.5 grid grid-cols-2 gap-2">
              <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
                <div class="text-[10px] text-dp-tertiary">
                  {{ t("toolbox.tools.aiusage.input") }}
                </div>
                <div class="text-[13px] font-semibold text-dp-primary tabular-nums">
                  {{ totals ? formatCompactTokens(totals.input) : "--" }}
                </div>
              </div>
              <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
                <div class="text-[10px] text-dp-tertiary">
                  {{ t("toolbox.tools.aiusage.output") }}
                </div>
                <div class="text-[13px] font-semibold text-dp-primary tabular-nums">
                  {{ totals ? formatCompactTokens(totals.output) : "--" }}
                </div>
              </div>
              <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
                <div class="text-[10px] text-dp-tertiary">
                  {{ t("toolbox.tools.aiusage.cacheRead") }}
                </div>
                <div class="text-[13px] font-semibold text-dp-primary tabular-nums">
                  {{ totals ? formatCompactTokens(totals.cache_read) : "--" }}
                </div>
              </div>
              <div class="rounded-[8px] bg-dp-panel-bg px-2 py-1.5">
                <div class="text-[10px] text-dp-tertiary">
                  {{ t("toolbox.tools.aiusage.cacheWrite") }}
                </div>
                <div class="text-[13px] font-semibold text-dp-primary tabular-nums">
                  {{ totals ? formatCompactTokens(totals.cache_write) : "--" }}
                </div>
              </div>
            </div>
          </section>

          <!-- 该时段无用量 -->
          <section
            v-if="noRangeData"
            class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-4 text-center"
          >
            <div class="text-[11px] text-dp-muted">
              {{ t("toolbox.tools.aiusage.noRange") }}
            </div>
          </section>

          <!-- 分工具 -->
          <section
            v-if="byTool.length"
            class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]"
          >
            <div class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.aiusage.byTool") }}
            </div>
            <div
              v-for="row in byTool"
              :key="row.key"
              class="mt-1.5 flex items-center justify-between gap-2"
            >
              <span class="min-w-0 truncate text-[11px] text-dp-secondary">
                {{ t(toolNameKey(row.key)) }}
              </span>
              <span class="shrink-0 text-[10.5px] text-dp-muted tabular-nums">
                {{ formatCompactTokens(tokensOf(row)) }}
                <span class="mx-1 text-dp-tertiary">·</span>
                {{ fmtUsd(row.cost_usd) }}
              </span>
            </div>
          </section>

          <!-- 模型明细 -->
          <section
            v-if="byModel.length"
            class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]"
          >
            <div class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.aiusage.byModel") }}
            </div>
            <div
              v-for="row in byModel"
              :key="row.key"
              class="mt-1.5"
            >
              <div class="flex items-center justify-between gap-2">
                <span class="min-w-0 truncate text-[11px] text-dp-secondary" :title="row.key">
                  {{ row.key }}
                </span>
                <span class="shrink-0 text-[10.5px] text-dp-muted tabular-nums">
                  {{ fmtUsd(row.cost_usd) }}
                </span>
              </div>
              <div class="mt-0.5 flex justify-between text-[9.5px] text-dp-muted tabular-nums">
                <span>
                  {{ t("toolbox.tools.aiusage.input") }}
                  {{ formatCompactTokens(row.input) }}
                  /
                  {{ t("toolbox.tools.aiusage.output") }}
                  {{ formatCompactTokens(row.output) }}
                </span>
                <span>{{ formatCompactTokens(tokensOf(row)) }}</span>
              </div>
            </div>
          </section>

          <!-- 项目 Top5 -->
          <section
            v-if="byProject.length"
            class="mb-2 rounded-[10px] border border-dp-card-border bg-dp-card-bg px-2.5 py-[9px]"
          >
            <div class="text-[10.5px] font-semibold tracking-[0.03em] text-dp-tertiary">
              {{ t("toolbox.tools.aiusage.byProject") }}
            </div>
            <div
              v-for="row in byProject.slice(0, 5)"
              :key="row.key"
              class="mt-1.5 flex items-center justify-between gap-2"
            >
              <span class="min-w-0 truncate text-[11px] text-dp-secondary" :title="row.key">
                {{ row.key || "-" }}
              </span>
              <span class="shrink-0 text-[10.5px] text-dp-muted tabular-nums">
                {{ formatCompactTokens(tokensOf(row)) }}
              </span>
            </div>
          </section>
        </template>
      </div>
    </el-scrollbar>
  </div>
</template>
