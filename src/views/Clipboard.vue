<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { ArrowRight, Delete, Link, Memo, Search, Star, TopRight } from "@element-plus/icons-vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useClipboardStore } from "@/stores/clipboard";
import { useFavoritesStore } from "@/stores/favorites";
import { FAV_CATEGORIES, buildFavoriteFromUrl } from "@/lib/stash";
import { copyImageToClipboard, getClipImageDir } from "@/api/clipboard";
import { launchItem } from "@/api/launcher";
import type { ClipEntry } from "@/types";

const { t } = useI18n();
const store = useClipboardStore();
const favorites = useFavoritesStore();
const keyword = ref("");

const clipImageDir = ref("");
onMounted(async () => {
  try {
    clipImageDir.value = await getClipImageDir();
  } catch {
    // 获取图片目录失败时静默忽略
  }
});

function isImage(entry: ClipEntry): boolean {
  return entry.kind === "image";
}

function imageUrl(filename: string): string {
  if (!clipImageDir.value) return "";
  return convertFileSrc(`${clipImageDir.value}/${filename}`);
}

/** 内容类型：普通文本 / 网址 / JSON / SQL / 图片 */
type ClipKind = "text" | "url" | "json" | "sql" | "image";
type TypeFilter = ClipKind | "all";
const type = ref<TypeFilter>("all");

/** 识别 URL 类内容。 */
function isUrl(text: string): boolean {
  return /^https?:\/\//i.test(text.trim());
}

/**
 * 识别对象/数组形式的 JSON 文本。
 * 限定 {、[ 开头且能完整解析：JSON.parse 对纯数字、布尔、引号字符串也会成功，
 * 不加限定会把普通文本误标成 JSON。
 */
function isJson(text: string): boolean {
  const s = text.trim();
  if (!s.startsWith("{") && !s.startsWith("[")) {
    return false;
  }
  try {
    void JSON.parse(s);
    return true;
  } catch {
    return false;
  }
}

const SQL_RE = /^(SELECT|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP|TRUNCATE|WITH|GRANT|REVOKE|MERGE|BEGIN|COMMIT|ROLLBACK|EXPLAIN|PRAGMA|SET)\b/i;

/** 识别 SQL 脚本：跳过行注释后检查首个非空行是否以 SQL 关键字开头。 */
function isSql(text: string): boolean {
  for (const line of text.split("\n")) {
    const s = line.trim();
    if (!s || s.startsWith("--")) {
      continue;
    }
    return SQL_RE.test(s);
  }
  return false;
}

/** 类型分类（过滤与标签共用同一判定）。 */
function classify(text: string): ClipKind {
  if (isUrl(text)) {
    return "url";
  }
  if (isJson(text)) {
    return "json";
  }
  if (isSql(text)) {
    return "sql";
  }
  return "text";
}

/** 条目级分类：图片条目直接返回 image，文本条目按内容判定。 */
function classifyEntry(entry: ClipEntry): ClipKind {
  if (entry.kind === "image") {
    return "image";
  }
  return classify(entry.text);
}

const typeOptions = computed(() => [
  { value: "all" as TypeFilter, label: t("clipboard.typeAll") },
  { value: "text" as TypeFilter, label: t("clipboard.typeText") },
  { value: "url" as TypeFilter, label: t("clipboard.typeUrl") },
  { value: "json" as TypeFilter, label: t("clipboard.typeJson") },
  { value: "sql" as TypeFilter, label: t("clipboard.typeSql") },
  { value: "image" as TypeFilter, label: t("clipboard.typeImage") },
]);

/** 关键字 + 类型双条件过滤。 */
const filtered = computed(() => {
  const k = keyword.value.trim().toLowerCase();
  const tp = type.value;
  return store.entries.filter((e) => {
    if (k && !isImage(e) && !e.text.toLowerCase().includes(k)) {
      return false;
    }
    if (tp !== "all" && classifyEntry(e) !== tp) {
      return false;
    }
    return true;
  });
});

interface DayGroup {
  /** 本地时区的 y-m-d 分组键 */
  key: string;
  label: string;
  entries: ClipEntry[];
}

function dayKey(d: Date): string {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/** 按天分组（entries 已按时间倒序，桶自然按天从新到旧排列）。 */
const groups = computed<DayGroup[]>(() => {
  const buckets = new Map<string, { date: Date; entries: ClipEntry[] }>();
  for (const e of filtered.value) {
    const date = new Date(e.updated_at);
    const key = dayKey(date);
    const bucket = buckets.get(key);
    if (bucket) {
      bucket.entries.push(e);
    } else {
      buckets.set(key, { date, entries: [e] });
    }
  }
  const today = new Date();
  const todayKey = dayKey(today);
  const yestKey = dayKey(new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1));
  const result: DayGroup[] = [];
  for (const [key, bucket] of buckets) {
    let label: string;
    if (key === todayKey) {
      label = t("clipboard.today");
    } else if (key === yestKey) {
      label = t("clipboard.yesterday");
    } else if (bucket.date.getFullYear() === today.getFullYear()) {
      label = t("clipboard.dateFormat", { m: bucket.date.getMonth() + 1, d: bucket.date.getDate() });
    } else {
      label = t("clipboard.dateFormatYear", {
        y: bucket.date.getFullYear(),
        m: bucket.date.getMonth() + 1,
        d: bucket.date.getDate(),
      });
    }
    result.push({ key, label, entries: bucket.entries });
  }
  return result;
});

/** 手动展开过的天 / 手动折叠过的天（互相排斥，同时在场以先命中者为准）。 */
const openDays = ref<Set<string>>(new Set());
const closedDays = ref<Set<string>>(new Set());

/** 默认：最新的一天展开，其余折叠；用户手动操作过的天以手动状态为准。 */
function isDayCollapsed(key: string, index: number): boolean {
  if (openDays.value.has(key)) {
    return false;
  }
  if (closedDays.value.has(key)) {
    return true;
  }
  return index !== 0;
}

function toggleDay(key: string, index: number) {
  const open = new Set(openDays.value);
  const closed = new Set(closedDays.value);
  if (isDayCollapsed(key, index)) {
    open.add(key);
    closed.delete(key);
  } else {
    closed.add(key);
    open.delete(key);
  }
  openDays.value = open;
  closedDays.value = closed;
}

function timeText(ts: number): string {
  const diff = Date.now() - ts;
  const min = Math.floor(diff / 60000);
  if (min < 1) {
    return t("clipboard.justNow");
  }
  if (min < 60) {
    return t("clipboard.minutesAgo", { n: min });
  }
  const hour = Math.floor(min / 60);
  if (hour < 24) {
    return t("clipboard.hoursAgo", { n: hour });
  }
  return t("clipboard.daysAgo", { n: Math.floor(hour / 24) });
}

async function onCopy(text: string) {
  try {
    await store.copy(text);
    ElMessage.success(t("clipboard.copied"));
  } catch (e) {
    ElMessage.error(t("clipboard.copyFail", { err: String(e) }));
  }
}

async function onCopyEntry(entry: ClipEntry) {
  if (entry.kind === "image") {
    try {
      await copyImageToClipboard(entry.text);
      ElMessage.success(t("clipboard.copied"));
    } catch (e) {
      ElMessage.error(t("clipboard.copyFail", { err: String(e) }));
    }
  } else {
    await onCopy(entry.text);
  }
}

/** 一键收藏剪贴板里的网址（托管平台自动识别为仓库类型）。 */
async function onCollect(text: string) {
  try {
    const added = await favorites.addFromUrl(buildFavoriteFromUrl(text.trim()));
    if (added) {
      ElMessage.success(t("clipboard.collected"));
    } else {
      ElMessage.info(t("clipboard.alreadyCollected"));
    }
  } catch (e) {
    ElMessage.error(t("clipboard.collectFail", { err: String(e) }));
  }
}

/** 系统浏览器打开剪贴板里的网址。 */
async function onOpenUrl(text: string) {
  try {
    await launchItem({
      id: "clip-link",
      name: text,
      icon: "",
      item_type: "url",
      value: text,
      color: "",
    });
  } catch (e) {
    ElMessage.error(t("clipboard.openFail", { err: String(e) }));
  }
}

/** 将剪贴板文本转为便签收藏。 */
async function onAddNote(text: string) {
  try {
    const now = new Date().toISOString();
    await favorites.upsert({
      url: `stash://note/n-${Date.now()}-${Math.floor(Math.random() * 1000)}`,
      name: text.slice(0, 24) || t("stash.typeNote"),
      description: "",
      fav_type: "note",
      payload: JSON.stringify({ content: text, color: "#fef3c7" }),
      languages: [],
      stars: 0,
      forks: 0,
      tags: [],
      category: FAV_CATEGORIES.indexOf("其他"),
      category_name: "其他",
      create_time: now,
      update_time: now,
    });
    ElMessage.success(t("common.saved"));
  } catch (e) {
    ElMessage.error(t("stash.saveFail", { err: String(e) }));
  }
}

/** 条件清空的范围（菜单 command）。 */
type ClearScope = TypeFilter | "filtered" | "older";

/** 清空范围 → 菜单文案 / 标题的 i18n 键。 */
const clearLabelKeys = {
  filtered: "clipboard.clearFiltered",
  url: "clipboard.clearUrl",
  json: "clipboard.clearJson",
  sql: "clipboard.clearSql",
  image: "clipboard.clearImage",
  text: "clipboard.clearText",
  older: "clipboard.clearOlder",
  all: "clipboard.clearAll",
} as const;

async function onClear(scope: ClearScope) {
  const cutoff = Date.now() - 7 * 24 * 60 * 60 * 1000;
  const list = scope === "filtered" ? filtered.value : store.entries;
  const ids: string[] = [];
  for (const e of list) {
    if (scope === "older") {
      if (e.updated_at < cutoff) {
        ids.push(e.id);
      }
    } else if (scope === "all" || scope === "filtered") {
      ids.push(e.id);
    } else if (classifyEntry(e) === scope) {
      ids.push(e.id);
    }
  }
  if (ids.length === 0) {
    ElMessage.info(t("clipboard.nothingToClear"));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("clipboard.clearConfirmText", { n: ids.length }),
      t(clearLabelKeys[scope]),
      { confirmButtonText: t("clipboard.clear"), cancelButtonText: t("common.cancel"), type: "warning" }
    );
  } catch {
    return;
  }
  await store.removeMany(ids);
  ElMessage.success(t("clipboard.cleared"));
}
</script>

<template>
  <div class="view-root [&_.view-toolbar]:gap-1.5">
    <div class="view-toolbar">
      <el-input v-model="keyword" size="small" clearable :placeholder="t('clipboard.searchPh')" :prefix-icon="Search" />
      <el-dropdown trigger="click" @command="(s: ClearScope) => onClear(s)">
        <el-button size="small" text type="danger">{{ t("clipboard.clear") }}</el-button>
        <template #dropdown>
          <el-dropdown-menu class="!text-[12px]">
            <el-dropdown-item command="filtered">{{ t("clipboard.clearFiltered") }}</el-dropdown-item>
            <el-dropdown-item command="url" divided>{{ t("clipboard.clearUrl") }}</el-dropdown-item>
            <el-dropdown-item command="json">{{ t("clipboard.clearJson") }}</el-dropdown-item>
            <el-dropdown-item command="sql">{{ t("clipboard.clearSql") }}</el-dropdown-item>
            <el-dropdown-item command="image">{{ t("clipboard.clearImage") }}</el-dropdown-item>
            <el-dropdown-item command="text">{{ t("clipboard.clearText") }}</el-dropdown-item>
            <el-dropdown-item command="older" divided>{{ t("clipboard.clearOlder") }}</el-dropdown-item>
            <el-dropdown-item command="all" divided class="!text-[#f56c6c]">{{ t("clipboard.clearAll") }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
    <!-- 类型过滤：全部 / 文本 / 网址 / JSON -->
    <div class="flex items-center gap-1 px-0.5 mb-1 shrink-0">
      <button
        v-for="opt in typeOptions"
        :key="opt.value"
        class="text-[11px] leading-none py-[3px] px-2 rounded-full border transition-colors duration-150 ease-[ease] cursor-pointer"
        :class="type === opt.value
          ? 'bg-dp-accent border-dp-accent text-white'
          : 'border-dp-divider text-dp-secondary hover:text-dp-primary hover:bg-dp-hover-bg'"
        @click="type = opt.value"
      >
        {{ opt.label }}
      </button>
      <span class="ml-auto text-[10.5px] text-dp-tertiary">{{ t("clipboard.count", { n: filtered.length }) }}</span>
    </div>
    <el-scrollbar class="view-scroll">
      <el-empty
        v-if="filtered.length === 0"
        :description="store.entries.length > 0 ? t('clipboard.noMatch') : t('clipboard.empty')"
        :image-size="60"
      />
      <div v-for="(g, gi) in groups" :key="g.key">
        <!-- 日期分组头：点击折叠/展开 -->
        <div
          class="flex items-center gap-1 px-1 py-1 mb-1 text-[11px] text-dp-tertiary cursor-pointer select-none rounded-md transition-colors duration-150 ease-[ease] hover:text-dp-secondary hover:bg-dp-hover-bg"
          @click="toggleDay(g.key, gi)"
        >
          <el-icon
            :size="10"
            class="transition-transform duration-150 ease-[ease]"
            :class="isDayCollapsed(g.key, gi) ? '' : 'rotate-90'"
          >
            <ArrowRight />
          </el-icon>
          <span class="font-medium">{{ g.label }}</span>
          <span class="px-1.5 py-[1px] rounded-full text-[10px] leading-none bg-dp-tag-soft text-dp-tertiary">
            {{ g.entries.length }}
          </span>
        </div>
        <div v-show="!isDayCollapsed(g.key, gi)">
          <div
            v-for="entry in g.entries"
            :key="entry.id"
            class="clip-item relative bg-dp-card-bg border border-dp-card-border rounded-[10px] pt-2 pr-2.5 pb-1.5 pl-3 mb-1.5 cursor-pointer overflow-hidden transition-[border-color,box-shadow] duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover before:content-[''] before:absolute before:left-0 before:top-0 before:bottom-0 before:w-[3px] before:bg-dp-accent before:opacity-0 before:transition-opacity before:duration-150 before:ease-[ease] hover:before:opacity-100"
            :title="t('clipboard.copyTip')"
            @click="onCopyEntry(entry)"
          >
            <div v-if="isImage(entry)" class="flex items-center gap-[5px]">
              <img
                :src="imageUrl(entry.text)"
                class="w-full max-h-[120px] object-contain rounded-md select-none [-webkit-user-drag:none]"
                loading="lazy"
                draggable="false"
                alt=""
              />
            </div>
            <div v-else class="flex items-start gap-[5px] text-xs text-dp-secondary leading-[1.5]">
              <el-icon v-if="isUrl(entry.text)" class="shrink-0 mt-0.5 text-dp-accent"><Link /></el-icon>
              <span
                class="whitespace-pre-wrap break-all line-clamp-2"
                :class="isUrl(entry.text) ? 'text-dp-accent' : ''"
              >{{ entry.text }}</span>
            </div>
            <div class="flex items-center justify-between mt-[5px] text-[11px] text-dp-tertiary">
              <span class="inline-flex items-center gap-[5px] min-w-0">
                <span v-if="isImage(entry)" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft">IMG</span>
                <span v-else-if="isUrl(entry.text)" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft">URL</span>
                <span v-else-if="isJson(entry.text)" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft" :title="t('clipboard.jsonTag')">JSON</span>
                <span v-else-if="isSql(entry.text)" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft" :title="t('clipboard.sqlTag')">SQL</span>
                <span v-else-if="entry.text.length > 80" class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] text-dp-accent bg-dp-tag-soft">TEXT</span>
                {{ timeText(entry.updated_at) }}
                <span
                  v-if="entry.copy_count > 1"
                  class="shrink-0 text-[10px] leading-none py-0.5 px-[5px] rounded-[4px] bg-dp-btn-bg text-dp-tertiary"
                  :title="t('clipboard.copyCountTip', { n: entry.copy_count })"
                >×{{ entry.copy_count }}</span>
              </span>
              <span class="inline-flex items-center gap-[2px] shrink-0">
                <span
                  v-if="!isImage(entry) && isUrl(entry.text)"
                  class="inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.clip-item:hover_&]:opacity-100 hover:text-dp-accent hover:bg-dp-tag-soft"
                  :title="t('clipboard.openLink')"
                  @click.stop="onOpenUrl(entry.text)"
                >
                  <el-icon :size="12"><TopRight /></el-icon>
                </span>
                <span
                  v-if="!isImage(entry) && !isUrl(entry.text)"
                  class="inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.clip-item:hover_&]:opacity-100 hover:text-dp-accent hover:bg-dp-tag-soft"
                  :title="t('clipboard.toNote')"
                  @click.stop="onAddNote(entry.text)"
                >
                  <el-icon :size="12"><Memo /></el-icon>
                </span>
                <span
                  v-if="!isImage(entry) && isUrl(entry.text)"
                  class="inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.clip-item:hover_&]:opacity-100 hover:text-dp-accent hover:bg-dp-tag-soft"
                  :title="t('clipboard.collect')"
                  @click.stop="onCollect(entry.text)"
                >
                  <el-icon :size="12"><Star /></el-icon>
                </span>
                <span
                  class="inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.clip-item:hover_&]:opacity-100 hover:text-[#f56c6c] hover:bg-dp-danger-soft"
                  :title="t('common.delete')"
                  @click.stop="store.remove(entry.id)"
                >
                  <el-icon :size="12"><Delete /></el-icon>
                </span>
              </span>
            </div>
          </div>
        </div>
      </div>
    </el-scrollbar>
  </div>
</template>
