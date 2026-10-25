<script setup lang="ts">
import { computed, onMounted, ref, type Component } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { CaretBottom, CaretRight, Close, Plus, Setting, MagicStick, Monitor, Cpu, Grid, SetUp, Connection, Coin, Link, Lock, Tickets, Memo, DataAnalysis, Cellphone, Platform, Aim, Film, Key, Files, More, Collection, CollectionTag, Document, VideoCamera, Goods, ChatDotRound, Picture } from "@element-plus/icons-vue";
import { useFavoritesStore } from "@/stores/favorites";
import { useConfigStore } from "@/stores/config";
import { autoPullFavorites, pullFavorites, pushFavorites, refreshRepoStats } from "@/composables/useStashCloud";
import { launchItem } from "@/api/launcher";
import {
  buildFavoriteFromUrl,
  exportFavorites,
  FAV_CATEGORIES,
  FAV_TYPES,
  favTypeColor,
  groupKeyOf,
  isNoteItem,
  languageColor,
  parsePayload,
  platformLabelOf,
  isoNow,
} from "@/lib/stash";
import { readFavoriteImportFile, writeFavoriteExportFile } from "@/api/favorite";
import type { Favorite, FavType } from "@/types";

const { t } = useI18n();
const store = useFavoritesStore();
const config = useConfigStore();

/** 便签色板（沿用旧便签模块的五种底色）。 */
const NOTE_PALETTE = ["#fef3c7", "#dcfce7", "#dbeafe", "#fce7f3", "#ede9fe"] as const;
const NOTE_DEFAULT_COLOR: string = NOTE_PALETTE[0];

/** 预设分类图标与主题色（未知/自定义分类回落 More 灰色）。 */
const CATEGORY_META: Record<string, { icon: Component; color: string }> = {
  "AI/Agent": { icon: MagicStick, color: "#8b5cf6" },
  "前端框架": { icon: Monitor, color: "#3b82f6" },
  "后端框架": { icon: Cpu, color: "#10b981" },
  "UI 组件库": { icon: Grid, color: "#f59e0b" },
  "工具链/构建": { icon: SetUp, color: "#64748b" },
  "DevOps / CI/CD": { icon: Connection, color: "#06b6d4" },
  "数据库": { icon: Coin, color: "#f97316" },
  "网络/HTTP": { icon: Link, color: "#0ea5e9" },
  "安全": { icon: Lock, color: "#ef4444" },
  "测试": { icon: Tickets, color: "#84cc16" },
  "文档/知识管理": { icon: Memo, color: "#a855f7" },
  "运维/监控": { icon: DataAnalysis, color: "#14b8a6" },
  "移动端": { icon: Cellphone, color: "#ec4899" },
  "桌面端": { icon: Platform, color: "#6366f1" },
  "游戏": { icon: Aim, color: "#eab308" },
  "音视频": { icon: Film, color: "#8b5cf6" },
  "区块链": { icon: Key, color: "#fbbf24" },
  "编程语言": { icon: Files, color: "#22c55e" },
  "其他": { icon: More, color: "#9ca3af" },
};

function categoryIcon(name: string | null | undefined): Component {
  return (name && CATEGORY_META[name]?.icon) || More;
}

function categoryTint(name: string | null | undefined): string {
  return (name && CATEGORY_META[name]?.color) || "#9ca3af";
}

/** 类型分组头标签前的图标（书签/文章等纯类型组没有平台 SVG）。 */
const FAV_TYPE_ICONS: Record<FavType, Component> = {
  repo: Collection,
  article: Document,
  video: VideoCamera,
  product: Goods,
  social: ChatDotRound,
  image: Picture,
  bookmark: CollectionTag,
  note: Memo,
};

function groupIcon(g: { key: string }): Component {
  return g.key.startsWith("type:") ? FAV_TYPE_ICONS[g.key.slice(5) as FavType] ?? Memo : Memo;
}

function noteColorOf(item: Favorite): string {
  const color = parsePayload(item.payload).color;
  return typeof color === "string" && /^#[0-9a-fA-F]{6}$/.test(color) ? color : NOTE_DEFAULT_COLOR;
}

const keyword = ref("");
const categoryFilter = ref<string>("");

const categoryOptions = computed(() =>
  // 只显示现有条目用到的分类；下拉可清空 = 全部分类
  [...new Set(store.entries.map((e) => e.category_name).filter(Boolean))]
);

/** fav_type -> i18n 键（键名为 typeRepo/typeArticle… camelCase 形式）。 */
function typeKey(type: string): string {
  return `stash.type${type.charAt(0).toUpperCase()}${type.slice(1)}`;
}

/** fav_type -> 本地化标签；未知类型直接显示原始值。 */
function typeLabel(type: string): string {
  const key = typeKey(type);
  const label = t(key);
  return label === key ? type : label;
}

/** 关键字 + 分类双条件过滤（全文命中 name/描述/URL/标签/语言/笔记正文）。 */
const filtered = computed(() => {
  const k = keyword.value.trim().toLowerCase();
  return store.entries.filter((e) => {
    if (categoryFilter.value && e.category_name !== categoryFilter.value) {
      return false;
    }
    if (!k) {
      return true;
    }
    return (
      e.name.toLowerCase().includes(k) ||
      e.description.toLowerCase().includes(k) ||
      e.url.toLowerCase().includes(k) ||
      e.category_name.toLowerCase().includes(k) ||
      e.tags.some((tag) => tag.toLowerCase().includes(k)) ||
      e.languages.some((lang) => lang.toLowerCase().includes(k)) ||
      (parsePayload(e.payload).content ?? "").toLowerCase().includes(k)
    );
  });
});

interface FavGroup {
  key: string;
  label: string;
  iconSvg: string;
  color: string;
  entries: Favorite[];
  isNote: boolean;
}

/** 平台/笔记图标（与 stash 一致的静态 SVG 字面量，v-html 安全）。 */
const PLATFORM_SVG: Record<string, string> = {
  "github.com":
    '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/></svg>',
  note: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4h16v10l-6 6H4z"/><path d="M20 14h-6v6"/></svg>',
  default:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2v-6l-6 6-4-4 6-6z"/><path d="M13 3l6 6-1.4 1.4-6-6z"/></svg>',
};

const DEFAULT_CATEGORY = FAV_CATEGORIES[FAV_CATEGORIES.length - 1];

/** 建组：笔记单独成组最前、其他平台殿后，其余按条数降序。 */
const groups = computed<FavGroup[]>(() => {
  const buckets = new Map<string, Favorite[]>();
  for (const e of filtered.value) {
    const key = groupKeyOf(e);
    const list = buckets.get(key);
    if (list) {
      list.push(e);
    } else {
      buckets.set(key, [e]);
    }
  }
  const result: FavGroup[] = [];
  for (const [key, entries] of buckets) {
    let label: string;
    let iconSvg = "";
    let color: string;
    if (key === "note") {
      label = t("stash.groupNotes");
      iconSvg = PLATFORM_SVG.note;
      color = "#64748b";
    } else if (key === "default") {
      label = t("stash.groupOthers");
      iconSvg = PLATFORM_SVG.default;
      color = "#666666";
    } else if (key.startsWith("type:")) {
      const type = key.slice(5) as FavType;
      label = typeLabel(type);
      color = favTypeColor(type);
    } else {
      label = platformLabelOf(key);
      iconSvg = PLATFORM_SVG[key] ?? PLATFORM_SVG.default;
      color = "#666666";
    }
    const sorted = [...entries].sort((a, b) => {
      const ta = isNoteItem(a) ? a.update_time : a.create_time;
      const tb = isNoteItem(b) ? b.update_time : b.create_time;
      return tb.localeCompare(ta); // 新的在前；相等时 0，保持稳定排序
    });
    result.push({ key, label, iconSvg, color, entries: sorted, isNote: key === "note" });
  }
  result.sort((a, b) => {
    if (a.key === "note") {
      return -1;
    }
    if (b.key === "note") {
      return 1;
    }
    if (a.key === "default") {
      return 1;
    }
    if (b.key === "default") {
      return -1;
    }
    return b.entries.length - a.entries.length;
  });
  return result;
});

/** 组默认折叠，组头点击展开；搜索/筛选命中时自动全展开（否则结果会被折叠藏住）。 */
const openGroups = ref<Set<string>>(new Set());

const searching = computed(() => keyword.value.trim().length > 0 || Boolean(categoryFilter.value));

function isClosed(key: string): boolean {
  return !searching.value && !openGroups.value.has(key);
}

function toggleGroup(key: string) {
  const next = new Set(openGroups.value);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
  }
  openGroups.value = next;
}

function formatCount(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k` : String(n);
}

/** 笔记卡片预览（超长截断，完整内容在编辑器查看）。 */
function notePreview(item: Favorite, max = 90): string {
  const content = parsePayload(item.payload).content || item.description || "";
  return content.length > max ? content.slice(0, max) + "…" : content || t("stash.noContent");
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    return "";
  }
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const hm = `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return d.getFullYear() === now.getFullYear() ? hm : `${d.getFullYear()}-${hm}`;
}

function thumbOf(item: Favorite): string {
  const payload = parsePayload(item.payload);
  return payload.image || payload.src || "";
}

/** 收藏条目点击：笔记开编辑器，其余用系统浏览器打开。 */
async function onOpen(item: Favorite) {
  if (isNoteItem(item)) {
    openNoteEditor(item);
    return;
  }
  try {
    await launchItem({ id: "fav-link", name: item.name, icon: "", item_type: "url", value: item.url, color: "" });
  } catch (e) {
    ElMessage.error(t("stash.openFail", { err: String(e) }));
  }
}

async function onRemove(item: Favorite) {
  try {
    await store.remove(item.url);
    ElMessage.success(t("stash.removed"));
  } catch (e) {
    ElMessage.error(t("stash.removeFail", { err: String(e) }));
  }
}

// ---------- 添加 / 编辑 ----------

const editVisible = ref(false);
const editing = ref<Favorite | null>(null);

function blankFavorite(): Favorite {
  const now = new Date().toISOString();
  return {
    url: "",
    name: "",
    description: "",
    fav_type: "bookmark",
    payload: "{}",
    languages: [],
    stars: 0,
    forks: 0,
    tags: [],
    category: FAV_CATEGORIES.indexOf(DEFAULT_CATEGORY),
    category_name: DEFAULT_CATEGORY,
    create_time: now,
    update_time: now,
  };
}

function openAddDialog() {
  typeTouched.value = false;
  editing.value = blankFavorite();
  editVisible.value = true;
}

/** 用户在弹窗里手动选过类型后，URL 推断不再覆盖 fav_type。 */
const typeTouched = ref(false);

/** URL 输入时智能预填：托管平台识别为 repo 并推荐 owner/repo 名称。 */
function onUrlInput(url: string) {
  const item = editing.value;
  if (!item || !/^https?:\/\//i.test(url)) {
    return;
  }
  const base = buildFavoriteFromUrl(url);
  if (base.name) {
    item.name = item.name || base.name;
  }
  if (!typeTouched.value) {
    item.fav_type = base.fav_type;
  }
}

const tagsText = computed({
  get: () => editing.value?.tags.join(", ") ?? "",
  set: (v: string) => {
    if (editing.value) {
      editing.value.tags = v.split(/[,，]/).map((s) => s.trim()).filter(Boolean);
    }
  },
});

const editingIsNote = computed(() => editing.value?.fav_type === "note");

async function saveEdit() {
  const item = editing.value;
  if (!item) {
    return;
  }
  if (item.fav_type === "note") {
    // 便签：正文入 payload.content（与 stash 扩展约定一致），颜色入 payload.color，
    // url 用 stash://note/<id> 伪地址兜底（收藏主键不可为空）
    const payload = parsePayload(item.payload);
    payload.content = item.description;
    if (typeof payload.color !== "string") {
      payload.color = NOTE_DEFAULT_COLOR;
    }
    if (!item.name.trim()) {
      item.name = firstLineName(item.description);
    }
    if (!item.url.trim()) {
      item.url = `stash://note/n-${Date.now()}-${Math.floor(Math.random() * 1000)}`;
    }
    item.update_time = isoNow();
    try {
      await store.upsert({ ...item, url: item.url.trim(), description: "", payload: JSON.stringify(payload) });
    } catch (e) {
      ElMessage.error(t("stash.saveFail", { err: String(e) }));
      return;
    }
    editVisible.value = false;
    ElMessage.success(t("common.saved"));
    return;
  }
  if (!/^https?:\/\//i.test(item.url.trim())) {
    ElMessage.warning(t("stash.urlInvalid"));
    return;
  }
  const url = item.url.trim();
  // 同址覆盖需确认，且沿用原条目的创建时间（不把 create_time 冲成 now）
  const dup = store.entries.find((e) => e.url === url);
  if (dup) {
    try {
      await ElMessageBox.confirm(
        t("stash.dupConfirm", { name: dup.name }),
        t("stash.dupTitle"),
        {
          type: "warning",
          confirmButtonText: t("common.confirm"),
          cancelButtonText: t("common.cancel"),
        },
      );
    } catch {
      return;
    }
    item.create_time = dup.create_time;
  }
  item.category = categoryIndex(item.category_name);
  item.update_time = isoNow();
  try {
    await store.upsert({ ...item, url });
  } catch (e) {
    ElMessage.error(t("stash.saveFail", { err: String(e) }));
    return;
  }
  editVisible.value = false;
  ElMessage.success(t("common.saved"));
}

/** category 与 category_name 双写一致（显示层全用 name，数字序号供导入/云端还原用）。 */
function categoryIndex(name: string | null | undefined): number {
  const idx = (FAV_CATEGORIES as readonly string[]).indexOf(name ?? "");
  return idx >= 0 ? idx : FAV_CATEGORIES.length - 1;
}

/** 便签默认名：取首行非空文本，最多 24 字（与旧便签迁移规则一致）。 */
function firstLineName(content: string): string {
  const line = content.split("\n").map((s) => s.trim()).find(Boolean) ?? "";
  return line.slice(0, 24) || t("stash.typeNote");
}

// ---------- 笔记编辑器 ----------

const noteVisible = ref(false);
const noteEditing = ref<Favorite | null>(null);

/** 笔记正文双向绑定（payload JSON 的 content 字段）。 */
const notePayloadText = computed<string>({
  get: () => (noteEditing.value ? parsePayload(noteEditing.value.payload).content ?? "" : ""),
  set: (v: string) => {
    if (noteEditing.value) {
      const payload = parsePayload(noteEditing.value.payload);
      payload.content = v;
      noteEditing.value.payload = JSON.stringify(payload);
    }
  },
});

/** 便签颜色双向绑定（payload JSON 的 color 字段）。 */
const noteColor = computed<string>({
  get: () => (noteEditing.value ? noteColorOf(noteEditing.value) : NOTE_DEFAULT_COLOR),
  set: (v: string) => {
    if (noteEditing.value) {
      const payload = parsePayload(noteEditing.value.payload);
      payload.color = v;
      noteEditing.value.payload = JSON.stringify(payload);
    }
  },
});

function openNoteEditor(item: Favorite) {
  noteEditing.value = { ...item };
  noteVisible.value = true;
}

async function saveNote() {
  const item = noteEditing.value;
  if (!item) {
    return;
  }
  const payload = parsePayload(item.payload);
  payload.content = payload.content ?? "";
  if (!payload.content.trim()) {
    ElMessage.warning(t("stash.noteEmpty"));
    return;
  }
  if (typeof payload.color !== "string") {
    payload.color = noteColorOf(item);
  }
  try {
    await store.upsert({ ...item, payload: JSON.stringify(payload), update_time: isoNow() });
  } catch (e) {
    ElMessage.error(t("stash.saveFail", { err: String(e) }));
    return;
  }
  noteVisible.value = false;
  ElMessage.success(t("common.saved"));
}

async function deleteNote() {
  const item = noteEditing.value;
  if (!item) {
    return;
  }
  noteVisible.value = false;
  await onRemove(item);
}

// ---------- 导入 / 导出 / 清空 ----------

async function onImport() {
  const picked = await openFileDialog({
    multiple: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof picked !== "string" || !picked) {
    return;
  }
  try {
    const text = await readFavoriteImportFile(picked);
    const count = await store.importJson(text);
    ElMessage.success(t("stash.importOk", { n: count }));
  } catch (e) {
    ElMessage.error(t("stash.importFail", { err: String(e) }));
  }
}

async function onExport() {
  const date = new Date().toISOString().slice(0, 10);
  const picked = await saveFileDialog({ defaultPath: `stash_projects_${date}.json` });
  if (!picked || typeof picked !== "string") {
    return;
  }
  try {
    await writeFavoriteExportFile(picked, exportFavorites(store.entries));
    ElMessage.success(t("stash.exportOk", { path: picked }));
  } catch (e) {
    ElMessage.error(t("stash.exportFail", { err: String(e) }));
  }
}

async function onClearAll() {
  if (store.entries.length === 0) {
    ElMessage.info(t("stash.nothingToClear"));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("stash.clearConfirmText", { n: store.entries.length }),
      t("stash.clearAll"),
      {
        confirmButtonText: t("common.confirm"),
        cancelButtonText: t("common.cancel"),
        type: "warning",
      }
    );
  } catch {
    return;
  }
  try {
    await store.clearAll();
  } catch (e) {
    ElMessage.error(t("stash.clearFail", { err: String(e) }));
    return;
  }
  ElMessage.success(t("stash.cleared"));
}

type MenuCommand = "import" | "export" | "refresh" | "push" | "pull" | "syncSettings" | "clear";

function onMenuCommand(cmd: MenuCommand) {
  if (cmd === "import") {
    void onImport();
  } else if (cmd === "export") {
    void onExport();
  } else if (cmd === "refresh") {
    void onRefreshStats();
  } else if (cmd === "push") {
    void onPush();
  } else if (cmd === "pull") {
    void onPull();
  } else if (cmd === "syncSettings") {
    syncVisible.value = true;
  } else if (cmd === "clear") {
    void onClearAll();
  }
}

// ---------- 云同步 / 仓库刷新 ----------

const syncVisible = ref(false);
const syncing = ref(false);
const pulling = ref(false);
const refreshing = ref(false);

const cloudEnabled = computed(() => config.settings.favorite_sync === true);

const syncModel = computed({
  get: () => cloudEnabled.value,
  set: (enabled: boolean) => void onToggleSync(enabled),
});

function checkToken(): boolean {
  if (!config.settings.auth_token) {
    ElMessage.warning(t("stash.loginFirst"));
    return false;
  }
  return true;
}

async function onToggleSync(enabled: boolean) {
  if (enabled && !checkToken()) {
    return;
  }
  await config.setFavoriteSync(enabled);
  ElMessage.success(enabled ? t("stash.syncOn") : t("stash.syncOff"));
}

async function onPush() {
  if (!checkToken()) {
    return;
  }
  if (store.entries.length === 0) {
    ElMessage.info(t("stash.pullNoData"));
    return;
  }
  syncing.value = true;
  try {
    const n = await pushFavorites();
    ElMessage.success(t("stash.pushOk", { n }));
  } catch (e) {
    ElMessage.error(t("stash.syncFail", { err: String(e) }));
  } finally {
    syncing.value = false;
  }
}

async function onPull() {
  if (!checkToken()) {
    return;
  }
  pulling.value = true;
  try {
    const { merged, deleted } = await pullFavorites();
    if (merged + deleted === 0) {
      ElMessage.info(t("stash.pullNothing"));
    } else {
      ElMessage.success(t("stash.pullOk", { merged, deleted }));
    }
  } catch (e) {
    ElMessage.error(t("stash.syncFail", { err: String(e) }));
  } finally {
    pulling.value = false;
  }
}

async function onRefreshStats() {
  refreshing.value = true;
  try {
    const { updated, failed } = await refreshRepoStats();
    if (updated === 0 && failed === 0) {
      ElMessage.info(t("stash.refreshNone"));
    } else if (failed === 0) {
      ElMessage.success(t("stash.refreshOk", { n: updated }));
    } else {
      ElMessage.warning(t("stash.refreshPartial", { n: updated, failed }));
    }
  } catch (e) {
    ElMessage.error(t("stash.refreshFail", { err: String(e) }));
  } finally {
    refreshing.value = false;
  }
}

const lastSyncText = computed(() => {
  const raw = config.settings.favorite_last_sync;
  if (!raw) {
    return t("stash.neverSync");
  }
  return formatTime(raw.replace(" ", "T"));
});

onMounted(() => {
  // 先等本地库加载完再静默自动拉取，避免首次打开时列表闪空
  void store.init()
    .then(() => autoPullFavorites())
    .catch((err) => console.error("[favorites] init failed:", err));
});
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <el-input
        v-model="keyword"
        size="small"
        clearable
        :placeholder="t('stash.searchPh')"
        class="min-w-0 [&_.el-input\_\_inner]:text-[11.5px]"
      />
      <el-select
        v-model="categoryFilter"
        size="small"
        clearable
        :placeholder="t('stash.catAll')"
        class="shrink-0 w-[96px] [&_.el-input\_\_inner]:text-[11.5px]"
        :teleported="false"
      >
        <el-option v-for="c in categoryOptions" :key="c" :label="c" :value="c">
          <span class="inline-flex items-center gap-1.5">
            <el-icon :size="11" :style="{ color: categoryTint(c) }">
              <component :is="categoryIcon(c)" />
            </el-icon>
            {{ c }}
          </span>
        </el-option>
      </el-select>
      <el-tooltip
        :content="t('stash.add')"
        placement="bottom"
        :show-after="300"
        :teleported="false"
        effect="light"
        popper-class="dp-tip"
      >
        <el-button size="small" text class="!p-0 !pl-1.5" :icon="Plus" @click="openAddDialog" />
      </el-tooltip>
      <el-dropdown trigger="click" @command="(c: MenuCommand) => onMenuCommand(c)">
        <el-button size="small" text class="!p-0 !pr-1.5" :icon="Setting" />
        <template #dropdown>
          <el-dropdown-menu class="!text-[12px]">
            <el-dropdown-item command="import">{{ t("stash.menuImport") }}</el-dropdown-item>
            <el-dropdown-item command="export">{{ t("stash.menuExport") }}</el-dropdown-item>
            <el-dropdown-item command="push" divided>{{ t("stash.menuPush") }}</el-dropdown-item>
            <el-dropdown-item command="pull">{{ t("stash.menuPull") }}</el-dropdown-item>
            <el-dropdown-item command="refresh" divided>{{ t("stash.menuRefresh") }}</el-dropdown-item>
            <el-dropdown-item command="syncSettings">{{ t("stash.settings") }}</el-dropdown-item>
            <el-dropdown-item command="clear" divided class="!text-[#f56c6c]">{{ t("stash.clearAll") }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
    <el-scrollbar class="view-scroll">
      <el-empty
        v-if="groups.length === 0"
        :description="store.entries.length > 0 ? t('stash.noMatch') : t('stash.empty')"
        :image-size="60"
      />
      <div v-for="g in groups" :key="g.key">
        <!-- 分组头：笔记 / 平台 / 类型 -->
        <div
          class="flex items-center gap-1.5 px-1 py-1.5 mb-1 text-[11px] cursor-pointer select-none rounded-md transition-colors duration-150 ease-[ease] hover:bg-dp-hover-bg"
          @click="toggleGroup(g.key)"
        >
          <span
            v-if="g.iconSvg"
            class="inline-flex items-center justify-center w-[13px] h-[13px] text-dp-tertiary [&>svg]:w-[13px] [&>svg]:h-[13px]"
            v-html="g.iconSvg"
          />
          <!-- 分组头图标统一灰阶（平台 logo 本身单色，类型彩图标会与 SVG 组不一致） -->
          <el-icon v-else :size="11" class="text-dp-tertiary">
            <component :is="groupIcon(g)" />
          </el-icon>
          <span class="font-medium text-dp-secondary">{{ g.label }}</span>
          <span class="px-1.5 py-[1px] rounded-full text-[10px] leading-none bg-dp-tag-soft text-dp-tertiary">
            {{ g.entries.length }}
          </span>
          <el-icon :size="10" class="ml-auto text-dp-tertiary transition-all duration-150 ease-[ease]">
            <CaretRight v-if="isClosed(g.key)" />
            <CaretBottom v-else />
          </el-icon>
        </div>
        <div v-show="!isClosed(g.key)">
          <div
            v-for="entry in g.entries"
            :key="entry.url"
            class="fav-card relative bg-dp-card-bg border border-dp-card-border rounded-[10px] pt-2 pr-2.5 pb-1.5 pl-3 mb-1.5 cursor-pointer overflow-hidden transition-[border-color,box-shadow] duration-150 ease-[ease] hover:border-dp-card-hover-border hover:shadow-dp-card-hover"
            @click="onOpen(entry)"
          >
            <div class="flex items-center gap-1.5 min-w-0">
              <span
                class="shrink-0 px-[5px] py-0.5 rounded-[4px] text-[9.5px] leading-none font-medium"
                :style="g.isNote
                  ? { background: noteColorOf(entry), color: '#1f2937' }
                  : { background: favTypeColor(entry.fav_type) + '1a', color: favTypeColor(entry.fav_type) }"
              >
                {{ typeLabel(entry.fav_type) }}
              </span>
              <span class="flex-1 min-w-0 truncate text-[12.5px] font-medium text-dp-primary" :title="entry.name || entry.url">
                {{ entry.name || entry.url }}
              </span>
              <span class="shrink-0 text-[10px] text-dp-tertiary">{{ formatTime(isNoteItem(entry) ? entry.update_time : entry.create_time) }}</span>
              <span
                class="shrink-0 inline-flex items-center p-[3px] rounded-[5px] opacity-0 transition-opacity duration-150 ease-[ease] [.fav-card:hover_&]:opacity-100 hover:text-[#f56c6c] hover:bg-dp-danger-soft"
                :title="t('common.delete')"
                @click.stop="onRemove(entry)"
              >
                <el-icon :size="12" color="#f56c6c"><Close /></el-icon>
              </span>
            </div>
            <img
              v-if="thumbOf(entry)"
              class="mt-1.5 w-full h-[64px] object-cover rounded-md border border-dp-divider"
              :src="thumbOf(entry)"
              loading="lazy"
              referrerpolicy="no-referrer"
              @error="($event.target as HTMLImageElement).style.display = 'none'"
            />
            <div
              v-if="g.isNote || entry.description"
              class="mt-1 text-[11.5px] leading-[1.5] text-dp-secondary whitespace-pre-wrap break-all line-clamp-2"
            >
              {{ g.isNote ? notePreview(entry) : entry.description }}
            </div>
            <div class="flex items-center flex-wrap gap-x-2 gap-y-0.5 mt-[5px] text-[10.5px] text-dp-tertiary">
              <span
                v-if="entry.category_name && entry.category_name !== DEFAULT_CATEGORY"
                class="inline-flex items-center gap-[3px] px-[5px] py-0.5 rounded-[4px] bg-dp-tag-soft leading-none"
              >
                <el-icon :size="10" :style="{ color: categoryTint(entry.category_name) }">
                  <component :is="categoryIcon(entry.category_name)" />
                </el-icon>{{ entry.category_name }}
              </span>
              <span v-if="entry.fav_type === 'repo' && entry.stars > 0" class="inline-flex items-center gap-[2px]">★ {{ formatCount(entry.stars) }}</span>
              <span v-if="entry.fav_type === 'repo' && entry.forks > 0" class="inline-flex items-center gap-[2px]">⑂ {{ formatCount(entry.forks) }}</span>
              <span
                v-for="lang in (entry.fav_type === 'repo' ? entry.languages : []).slice(0, 3)"
                :key="lang"
                class="inline-flex items-center gap-[3px]"
              >
                <span class="w-[6px] h-[6px] rounded-full" :style="{ background: languageColor(lang) }" />{{ lang }}
              </span>
              <span v-for="tag in entry.tags.slice(0, 3)" :key="tag" class="px-[4px] py-0.5 rounded-[4px] bg-dp-tag-soft leading-none">
                #{{ tag }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </el-scrollbar>

    <!-- 添加 / 编辑 -->
    <el-dialog v-model="editVisible" :title="t('stash.addTitle')" width="352" :teleported="false" class="dp-dialog">
      <el-form v-if="editing" label-position="top" size="small" class="dp-form">
        <el-form-item :label="t('stash.fieldUrl')">
          <el-input v-model="editing.url" :placeholder="t('stash.urlPh')" @input="onUrlInput" />
        </el-form-item>
        <el-form-item :label="t('stash.fieldName')">
          <el-input v-model="editing.name" :placeholder="t('stash.namePh')" />
        </el-form-item>
        <div class="grid grid-cols-2 gap-x-2">
          <el-form-item :label="t('stash.fieldType')">
            <el-select v-model="editing.fav_type" class="w-full" :teleported="false" @change="typeTouched = true">
              <el-option v-for="tp in FAV_TYPES" :key="tp.value" :value="tp.value" :label="typeLabel(tp.value)" />
            </el-select>
          </el-form-item>
          <el-form-item :label="t('stash.fieldCategory')">
            <el-select v-model="editing.category_name" class="w-full" :teleported="false">
              <el-option v-for="c in FAV_CATEGORIES" :key="c" :value="c" :label="c">
                <span class="inline-flex items-center gap-1.5">
                  <el-icon :size="11" :style="{ color: categoryTint(c) }">
                    <component :is="categoryIcon(c)" />
                  </el-icon>
                  {{ c }}
                </span>
              </el-option>
            </el-select>
          </el-form-item>
        </div>
        <el-form-item :label="editingIsNote ? t('stash.notePh') : t('stash.fieldDescription')">
          <el-input v-model="editing.description" type="textarea" :rows="2" :placeholder="editingIsNote ? t('stash.notePh') : t('stash.descPh')" />
        </el-form-item>
        <el-form-item :label="t('stash.fieldTags')">
          <el-input v-model="tagsText" :placeholder="t('stash.tagsPh')" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button size="small" @click="editVisible = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" @click="saveEdit">{{ t("common.confirm") }}</el-button>
      </template>
    </el-dialog>

    <!-- 便签编辑器 -->
    <el-dialog v-model="noteVisible" :title="t('stash.editNote')" width="352" :teleported="false" class="dp-dialog">
      <template v-if="noteEditing">
        <div class="flex items-center gap-1.5 mb-2">
          <el-input v-model="noteEditing.name" size="small" :placeholder="t('stash.namePh')" class="flex-1 min-w-0" />
          <!-- 色板：沿用旧便签模块五色 -->
          <span
            v-for="c in NOTE_PALETTE"
            :key="c"
            class="w-3.5 h-3.5 rounded-full cursor-pointer shrink-0 transition-transform duration-150 ease-[ease] hover:scale-110"
            :class="noteColor === c ? 'border-2 border-dp-accent' : 'border border-dp-dot-border'"
            :style="{ background: c }"
            :title="c"
            @click="noteColor = c"
          />
        </div>
        <el-input
          v-model="notePayloadText"
          type="textarea"
          :rows="8"
          :placeholder="t('stash.notePh')"
          class="[&_.el-textarea\_\_inner]:text-[12px] [&_.el-textarea\_\_inner]:font-mono"
        />
      </template>
      <template #footer>
        <el-button size="small" type="danger" text @click="deleteNote">{{ t("common.delete") }}</el-button>
        <el-button size="small" @click="noteVisible = false">{{ t("common.cancel") }}</el-button>
        <el-button size="small" type="primary" @click="saveNote">{{ t("common.confirm") }}</el-button>
      </template>
    </el-dialog>

    <!-- 收藏设置（云同步 / 仓库刷新 / 存储概况） -->
    <el-dialog v-model="syncVisible" :title="t('stash.settings')" width="352" :teleported="false" class="dp-dialog">
      <div class="dp-dlg-text flex flex-col gap-3">
        <div class="flex items-center justify-between">
          <span>{{ t("stash.cloudSync") }}</span>
          <el-switch v-model="syncModel" size="small" />
        </div>
        <div class="text-[11px] leading-[1.6]">
          <template v-if="config.settings.auth_token">
            <span class="text-dp-tertiary">{{ t("stash.account", { user: config.settings.auth_user || "?" }) }}</span>
          </template>
          <template v-else>
            <span class="text-dp-tertiary">{{ t("stash.notLogged") }}</span>
          </template>
        </div>
        <div class="grid grid-cols-1 gap-2">
          <el-button size="small" :loading="syncing" @click="onPush">{{ t("stash.menuPush") }}</el-button>
          <el-button size="small" :loading="pulling" @click="onPull">{{ t("stash.menuPull") }}</el-button>
          <el-button size="small" :loading="refreshing" @click="onRefreshStats">{{ t("stash.menuRefresh") }}</el-button>
        </div>
        <div class="text-[10.5px] text-dp-tertiary leading-[1.6]">
          <div>{{ t("stash.localStats", { n: store.entries.length }) }}</div>
          <div>{{ t("stash.lastSync", { time: lastSyncText }) }}</div>
        </div>
      </div>
    </el-dialog>
  </div>
</template>
