<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { CSSProperties } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Close, Delete, Switch } from "@element-plus/icons-vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import { textDiff } from "@/api/toolbox";
import type { DiffRow, DiffSegment, TextDiffResult } from "@/types";

const { t } = useI18n();

const TEXTDIFF_MIN = { width: 760, height: 560 };

/* ---------- 外观同步 ---------- */
async function syncAppearance() {
  try {
    const settings = await getSettings();
    applyTheme(settings.theme);
    applyAccentColor(settings.accent_color);
    if (settings.locale === "zh-CN" || settings.locale === "en") {
      setAppLocale(settings.locale);
    }
  } catch {
    // 读取失败时保持默认外观
  }
}

async function closeWindow() {
  try {
    await getCurrentWindow().close();
  } catch {
    // 窗口可能已被销毁
  }
}

function startResize(direction: ResizeDirection, event: PointerEvent) {
  startWindowResize(direction, event, TEXTDIFF_MIN);
}

onMounted(() => {
  void syncAppearance();
  // 聚焦时重同步主题，保持与主窗一致
  window.addEventListener("focus", syncAppearance);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", syncAppearance);
});

/* ---------- 对比状态 ---------- */

/** 语言下拉选项：plain 走 i18n，其余用语言本名。 */
const LANGUAGES: { value: string; label: string }[] = [
  { value: "plain", label: "" },
  { value: "rust", label: "Rust" },
  { value: "typescript", label: "TypeScript" },
  { value: "tsx", label: "TSX" },
  { value: "javascript", label: "JavaScript" },
  { value: "python", label: "Python" },
  { value: "json", label: "JSON" },
  { value: "go", label: "Go" },
  { value: "java", label: "Java" },
  { value: "c", label: "C" },
  { value: "cpp", label: "C++" },
  { value: "css", label: "CSS" },
  { value: "html", label: "HTML" },
  { value: "yaml", label: "YAML" },
];

type ViewMode = "unified" | "split";

const left = ref("");
const right = ref("");
const language = ref("plain");
const viewMode = ref<ViewMode>("unified");
const result = ref<TextDiffResult | null>(null);
const diffing = ref(false);

/** 渲染行数上限：超大 diff 截断展示，避免 DOM 过大卡顿。 */
const MAX_RENDER_ROWS = 5000;

// 恢复上次使用的语言/视图偏好
onMounted(() => {
  const savedLang = localStorage.getItem("textdiff.language");
  if (savedLang && LANGUAGES.some((l) => l.value === savedLang)) {
    language.value = savedLang;
  }
  const savedView = localStorage.getItem("textdiff.viewMode");
  if (savedView === "unified" || savedView === "split") {
    viewMode.value = savedView;
  }
});
watch(language, (v) => localStorage.setItem("textdiff.language", v));
watch(viewMode, (v) => localStorage.setItem("textdiff.viewMode", v));

/** 语法 token → 颜色（One Dark 系，兼容深浅主题的 diff 底色）。 */
const TOKEN_COLORS: Record<string, string> = {
  keyword: "#c678dd",
  string: "#98c379",
  comment: "#8b949e",
  number: "#d19a66",
  constant: "#d19a66",
  function: "#61afef",
  type: "#e5c07b",
  property: "#e06c75",
  operator: "#56b6c2",
  punctuation: "#8b949e",
};

function langLabel(l: { value: string; label: string }): string {
  return l.value === "plain" ? t("toolbox.tools.textdiff.plainText") : l.label;
}

async function compare() {
  if (!left.value.trim() && !right.value.trim()) {
    ElMessage.warning(t("toolbox.tools.textdiff.emptyWarn"));
    return;
  }
  diffing.value = true;
  try {
    result.value = await textDiff(left.value, right.value, language.value === "plain" ? null : language.value);
  } catch (e) {
    // 失败丢弃旧结果：输入可能已交换/修改，旧结果方向不再可信
    result.value = null;
    ElMessage.error(String(e));
  } finally {
    diffing.value = false;
  }
}

function swap() {
  if (diffing.value) return; // 防竞态：避免旧请求返回后覆盖交换后的结果
  const l = left.value;
  left.value = right.value;
  right.value = l;
  if (result.value) {
    void compare();
  }
}

function clear() {
  if (diffing.value) return; // 防竞态：清空后旧请求仍会写入结果
  left.value = "";
  right.value = "";
  result.value = null;
}

const identical = computed(
  () => !!result.value && result.value.added === 0 && result.value.removed === 0,
);

const visibleRows = computed<DiffRow[]>(() =>
  (result.value?.rows ?? []).slice(0, MAX_RENDER_ROWS),
);

const truncated = computed(
  () => (result.value?.rows.length ?? 0) > MAX_RENDER_ROWS,
);

/** 并排视图的行配对：相邻 delete/insert 组逐行 zip，equal 行左右同现。 */
interface SplitRow {
  left: DiffRow | null;
  right: DiffRow | null;
}

const splitRows = computed<SplitRow[]>(() => {
  const rows = visibleRows.value;
  const out: SplitRow[] = [];
  let i = 0;
  while (i < rows.length) {
    const row = rows[i];
    if (row.kind === "equal") {
      out.push({ left: row, right: row });
      i += 1;
      continue;
    }
    const dels: DiffRow[] = [];
    const ins: DiffRow[] = [];
    while (i < rows.length && rows[i].kind === "delete") dels.push(rows[i++]);
    while (i < rows.length && rows[i].kind === "insert") ins.push(rows[i++]);
    for (let k = 0; k < Math.max(dels.length, ins.length); k += 1) {
      out.push({ left: dels[k] ?? null, right: ins[k] ?? null });
    }
  }
  return out;
});

function rowBg(kind: DiffRow["kind"]): string {
  if (kind === "delete") return "bg-[rgba(248,81,73,0.07)]";
  if (kind === "insert") return "bg-[rgba(63,185,80,0.07)]";
  return "";
}

function segClass(seg: DiffSegment, kind: DiffRow["kind"]): string {
  if (!seg.changed) return "";
  return kind === "delete" ? "rounded-[2px] bg-[rgba(248,81,73,0.28)]" : "rounded-[2px] bg-[rgba(63,185,80,0.28)]";
}

function segStyle(seg: DiffSegment): CSSProperties {
  const style: CSSProperties = {};
  const color = seg.token ? TOKEN_COLORS[seg.token] : undefined;
  if (color) style.color = color;
  if (seg.token === "comment") style.fontStyle = "italic";
  return style;
}

function marker(kind: DiffRow["kind"]): string {
  return kind === "insert" ? "+" : kind === "delete" ? "-" : "";
}
</script>

<template>
  <div class="w-screen h-screen bg-[var(--el-bg-color)] text-dp-primary overflow-hidden select-none flex flex-col">
    <!-- 顶栏(拖拽区) -->
    <div
      class="h-12 shrink-0 flex items-center gap-3 px-4 bg-dp-card-bg border-b border-dp-divider"
      data-tauri-drag-region
    >
      <span class="text-sm font-semibold text-dp-primary" data-tauri-drag-region>
        {{ t("toolbox.tools.textdiff.title") }}
      </span>
      <span class="text-[11px] text-dp-tertiary truncate" data-tauri-drag-region>
        {{ t("toolbox.tools.textdiff.desc") }}
      </span>
      <el-button size="small" :icon="Close" circle text class="ml-auto" @click="closeWindow" />
    </div>

    <!-- 工具行：语言选择 + 视图切换 + 交换/清空 + 对比 -->
    <div class="shrink-0 flex items-center gap-2 px-3 pt-3">
      <el-select v-model="language" size="default" class="w-[120px] shrink-0">
        <el-option v-for="l in LANGUAGES" :key="l.value" :value="l.value" :label="langLabel(l)" />
      </el-select>
      <div class="flex gap-1">
        <button
          class="cursor-pointer rounded-full px-3 py-[4px] text-xs transition-all duration-150 ease-[ease]"
          :class="
            viewMode === 'unified'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
          @click="viewMode = 'unified'"
        >
          {{ t("toolbox.tools.textdiff.unified") }}
        </button>
        <button
          class="cursor-pointer rounded-full px-3 py-[4px] text-xs transition-all duration-150 ease-[ease]"
          :class="
            viewMode === 'split'
              ? 'border-dp-accent bg-dp-accent text-white'
              : 'border-dp-card-border bg-dp-card-bg text-dp-muted'
          "
          @click="viewMode = 'split'"
        >
          {{ t("toolbox.tools.textdiff.split") }}
        </button>
      </div>
      <div class="ml-auto flex items-center gap-0.5">
        <el-button size="small" text :icon="Switch" :disabled="diffing" :title="t('toolbox.tools.textdiff.swap')" @click="swap" />
        <el-button size="small" text :icon="Delete" :disabled="diffing" :title="t('toolbox.tools.textdiff.clear')" @click="clear" />
        <el-button
          type="primary"
          size="small"
          class="ml-1.5"
          :loading="diffing"
          @click="compare"
        >
          {{ t("toolbox.tools.textdiff.compare") }}
        </el-button>
      </div>
    </div>

    <!-- 双栏输入：左右各半，⌘/Ctrl+Enter 触发对比 -->
    <div class="shrink-0 flex gap-3 px-3 pt-2.5" style="height: 200px">
      <div class="flex-1 min-w-0 h-full p-2 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <el-input
          v-model="left"
          type="textarea"
          resize="none"
          :spellcheck="false"
          :placeholder="t('toolbox.tools.textdiff.leftPh')"
          class="h-full [&_.el-textarea\_\_inner]:h-full [&_.el-textarea\_\_inner]:text-[12px] [&_.el-textarea\_\_inner]:font-mono [&_.el-textarea\_\_inner]:leading-[1.6] [&_.el-textarea\_\_inner]:select-text"
          @keydown.meta.enter.prevent="compare"
          @keydown.ctrl.enter.prevent="compare"
        />
      </div>
      <div class="flex-1 min-w-0 h-full p-2 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
        <el-input
          v-model="right"
          type="textarea"
          resize="none"
          :spellcheck="false"
          :placeholder="t('toolbox.tools.textdiff.rightPh')"
          class="h-full [&_.el-textarea\_\_inner]:h-full [&_.el-textarea\_\_inner]:text-[12px] [&_.el-textarea\_\_inner]:font-mono [&_.el-textarea\_\_inner]:leading-[1.6] [&_.el-textarea\_\_inner]:select-text"
          @keydown.meta.enter.prevent="compare"
          @keydown.ctrl.enter.prevent="compare"
        />
      </div>
    </div>

    <!-- 结果区：占满剩余高度 -->
    <div class="flex-1 min-h-0 px-3 pt-2.5 pb-3">
      <el-scrollbar class="h-full dp-scroll select-text">
        <template v-if="result">
          <!-- 统计行 -->
          <div class="mb-1.5 flex items-center gap-2.5 text-[11px]">
            <span class="font-bold text-[#3fb950]">+{{ result.added }}</span>
            <span class="font-bold text-[#f85149]">-{{ result.removed }}</span>
            <span v-if="identical" class="text-dp-muted">
              {{ t("toolbox.tools.textdiff.identical") }}
            </span>
            <span v-if="result.language" class="ml-auto rounded-[4px] bg-dp-btn-bg px-1.5 py-[1px] text-[10px] uppercase tracking-wide text-dp-tertiary">
              {{ result.language }}
            </span>
          </div>

          <!-- 统一视图 -->
          <div
            v-if="viewMode === 'unified'"
            class="overflow-x-auto rounded-[10px] border border-dp-card-border bg-dp-card-bg py-1 font-mono text-[12px] leading-[1.6]"
          >
            <div
              v-for="(row, idx) in visibleRows"
              :key="idx"
              class="flex min-w-fit items-start"
              :class="rowBg(row.kind)"
            >
              <span class="w-10 shrink-0 select-none pr-1.5 text-right text-[10px] text-dp-tertiary">
                {{ row.oldLineno ?? "" }}
              </span>
              <span class="w-10 shrink-0 select-none pr-1.5 text-right text-[10px] text-dp-tertiary">
                {{ row.newLineno ?? "" }}
              </span>
              <span class="w-4 shrink-0 select-none text-center" :class="row.kind === 'delete' ? 'text-[#f85149]' : row.kind === 'insert' ? 'text-[#3fb950]' : 'text-dp-tertiary'">
                {{ marker(row.kind) }}
              </span>
              <span class="min-w-0 flex-1 whitespace-pre pr-3 text-dp-secondary">
                <span
                  v-for="(seg, si) in row.segments"
                  :key="si"
                  :class="segClass(seg, row.kind)"
                  :style="segStyle(seg)"
                >{{ seg.text }}</span>
              </span>
            </div>
          </div>

          <!-- 并排视图：左右列各 50% 固定占比（跨行严格对齐），长行折行展示 -->
          <div
            v-else
            class="rounded-[10px] border border-dp-card-border bg-dp-card-bg py-1 font-mono text-[12px] leading-[1.6]"
          >
            <div
              v-for="(pair, idx) in splitRows"
              :key="idx"
              class="flex items-stretch"
            >
              <div class="flex w-1/2 shrink-0 items-start overflow-hidden" :class="pair.left ? rowBg(pair.left.kind) : ''">
                <template v-if="pair.left">
                  <span class="w-10 shrink-0 select-none pr-1.5 text-right text-[10px] text-dp-tertiary">
                    {{ pair.left.oldLineno ?? "" }}
                  </span>
                  <span class="w-3 shrink-0 select-none text-center" :class="pair.left.kind === 'delete' ? 'text-[#f85149]' : 'text-dp-tertiary'">
                    {{ marker(pair.left.kind) }}
                  </span>
                  <span class="min-w-0 flex-1 whitespace-pre-wrap break-words pr-2 text-dp-secondary">
                    <span
                      v-for="(seg, si) in pair.left.segments"
                      :key="si"
                      :class="segClass(seg, pair.left!.kind)"
                      :style="segStyle(seg)"
                    >{{ seg.text }}</span>
                  </span>
                </template>
                <span v-else class="flex-1" />
              </div>
              <div class="flex w-1/2 shrink-0 items-start overflow-hidden border-l border-dp-card-border" :class="pair.right ? rowBg(pair.right.kind) : ''">
                <template v-if="pair.right">
                  <span class="w-10 shrink-0 select-none pr-1.5 text-right text-[10px] text-dp-tertiary">
                    {{ pair.right.newLineno ?? "" }}
                  </span>
                  <span class="w-3 shrink-0 select-none text-center" :class="pair.right.kind === 'insert' ? 'text-[#3fb950]' : 'text-dp-tertiary'">
                    {{ marker(pair.right.kind) }}
                  </span>
                  <span class="min-w-0 flex-1 whitespace-pre-wrap break-words pr-2 text-dp-secondary">
                    <span
                      v-for="(seg, si) in pair.right.segments"
                      :key="si"
                      :class="segClass(seg, pair.right!.kind)"
                      :style="segStyle(seg)"
                    >{{ seg.text }}</span>
                  </span>
                </template>
                <span v-else class="flex-1" />
              </div>
            </div>
          </div>

          <!-- 截断提示 -->
          <div v-if="truncated" class="mt-1.5 text-center text-[11px] text-dp-muted">
            {{ t("toolbox.tools.textdiff.truncated", { n: MAX_RENDER_ROWS }) }}
          </div>
        </template>

        <!-- 空态 -->
        <div v-else class="h-[240px] flex items-center justify-center px-4 text-center">
          <span class="text-[12px] leading-[1.8] text-dp-tertiary">
            {{ t("toolbox.tools.textdiff.hint") }}
          </span>
        </div>
      </el-scrollbar>
    </div>

    <!-- 无边框窗口的边缘/四角缩放手柄 -->
    <div class="pointer-events-none fixed inset-0 z-50" aria-hidden="true">
      <div class="rs-hint left-0 right-0 top-0 h-[5px] cursor-ns-resize" @pointerdown.prevent="startResize('North', $event)" />
      <div class="rs-hint left-0 right-0 bottom-0 h-[5px] cursor-ns-resize" @pointerdown.prevent="startResize('South', $event)" />
      <div class="rs-hint bottom-0 left-0 top-0 w-[5px] cursor-ew-resize" @pointerdown.prevent="startResize('West', $event)" />
      <div class="rs-hint bottom-0 right-0 top-0 w-[5px] cursor-ew-resize" @pointerdown.prevent="startResize('East', $event)" />
      <div class="rs-hint left-0 top-0 h-[18px] w-[18px] cursor-nwse-resize" @pointerdown.prevent="startResize('NorthWest', $event)" />
      <div class="rs-hint right-0 top-0 h-[18px] w-[18px] cursor-nesw-resize" @pointerdown.prevent="startResize('NorthEast', $event)" />
      <div class="rs-hint bottom-0 left-0 h-[18px] w-[18px] cursor-nesw-resize" @pointerdown.prevent="startResize('SouthWest', $event)" />
      <div class="rs-hint bottom-0 right-0 h-[18px] w-[18px] cursor-nwse-resize" @pointerdown.prevent="startResize('SouthEast', $event)" />
    </div>
  </div>
</template>
