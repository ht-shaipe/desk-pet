<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  Check,
  Close,
  CopyDocument,
  Download,
  FolderAdd,
  FolderOpened,
  InfoFilled,
  Loading,
  Picture,
  Plus,
  Refresh,
  Search,
  Star,
  StarFilled,
  TopRight,
  Warning,
} from "@element-plus/icons-vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { setAppLocale } from "@/i18n";
import { getSettings } from "@/api/settings";
import { applyAccentColor, applyTheme } from "@/lib/theme";
import { startWindowResize, type ResizeDirection } from "@/lib/windowCtl";
import {
  compressImageData,
  compressImageFile,
  getClipboardImage,
  listImageFiles,
  saveImage,
  setClipboardImage,
} from "@/api/toolbox";
import {
  imagesAddDir,
  imagesAddTag,
  imagesDeleteItem,
  imagesDeletePaths,
  imagesDeleteTag,
  imagesDetail,
  imagesDedupStart,
  imagesDupGroups,
  imagesListDirs,
  imagesItemTagPairs,
  imagesListTags,
  imagesOpenPath,
  imagesQuery,
  imagesRenameTag,
  imagesRemoveDir,
  imagesRevealPath,
  imagesScanStart,
  imagesTaskCancel,
  imagesSetFav,
  imagesSetItemTag,
  imgSrc,
} from "@/api/images";
import type {
  CompressImageParams,
  ImageDirInfo,
  ImageDupGroup,
  ImageItemInfo,
  ImageScanProgress,
  ImageTagInfo,
} from "@/types";

const { t, locale } = useI18n();

const IMAGES_MIN = { width: 860, height: 600 };
const PAGE_SIZE = 200;

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
  startWindowResize(direction, event, IMAGES_MIN);
}

/* ---------- 数据状态 ---------- */
const dirs = ref<ImageDirInfo[]>([]);
const tags = ref<ImageTagInfo[]>([]);
const view = ref<"photos" | "compress" | "dups">("photos");
// 门户按钮定位:URL &tab=compress 直落压缩视图
if (new URLSearchParams(location.search).get("tab") === "compress") {
  view.value = "compress";
}
/** null = 全部目录 */
const dirFilter = ref<number | null>(null);
const favOnly = ref(false);
const tagFilter = ref<number | null>(null);
const searchInput = ref("");
const search = ref("");

const items = ref<ImageItemInfo[]>([]);
const hasMore = ref(false);
const total = ref(0);
const loading = ref(false);

/** 各目录扫描进行态(事件驱动,聚合展示) */
const scanState = reactive<
  Record<number, { scanned: number; added: number; updated: number; thumbsDone: number; thumbsTotal: number }>
>({});
/** 缩略图生成完成计数:对应网格图刷新缓存版本 */
const thumbTick = reactive<Record<number, number>>({});
const scanning = computed<Record<number, boolean>>(() => {
  const map: Record<number, boolean> = {};
  for (const k of Object.keys(scanState)) {
    map[Number(k)] = true;
  }
  return map;
});
const scanChip = computed(() => {
  let scanned = 0;
  let thumbsDone = 0;
  let thumbsTotal = 0;
  for (const s of Object.values(scanState)) {
    scanned += s.scanned;
    thumbsDone += s.thumbsDone;
    thumbsTotal += s.thumbsTotal;
  }
  return { scanned, thumbsDone, thumbsTotal, active: thumbsDone < thumbsTotal || Object.keys(scanState).length > 0 };
});

async function cancelAllScans() {
  for (const k of Object.keys(scanState)) {
    try {
      await imagesTaskCancel(Number(k));
    } catch {
      // 忽略
    }
  }
}

/** 缩略图缓存版本:生成完成事件后 +1 触发对应 <img> 重新加载 */
function imgV(item: ImageItemInfo): string {
  return `${item.mtime}-${thumbTick[item.id] ?? 0}`;
}
const dedupRunning = ref(false);
const dedupProgress = reactive({ done: 0, total: 0 });

let searchTimer: ReturnType<typeof setTimeout> | undefined;
watch(searchInput, (v) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    search.value = v.trim();
  }, 300);
});

function currentQuery() {
  return {
    dirId: dirFilter.value ?? undefined,
    favOnly: favOnly.value,
    docOnly: false, // 文档视图已移除,仅图片
    tagId: tagFilter.value ?? undefined,
    search: search.value,
    limit: PAGE_SIZE,
    offset: items.value.length,
  };
}

async function loadDirs() {
  try {
    dirs.value = await imagesListDirs();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** item id → 标签 id 列表(缩略图色点用) */
const itemTagsMap = ref<Map<number, number[]>>(new Map());

async function loadItemTagPairs() {
  try {
    const map = new Map<number, number[]>();
    for (const p of await imagesItemTagPairs()) {
      const list = map.get(p.itemId) ?? [];
      list.push(p.tagId);
      map.set(p.itemId, list);
    }
    itemTagsMap.value = map;
  } catch {
    // 忽略:色点属增强展示
  }
}

async function loadTags() {
  try {
    tags.value = await imagesListTags();
    await loadItemTagPairs();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** 照片卡上的标签颜色(最多 3 个) */
function itemTagColors(it: ImageItemInfo): string[] {
  const ids = itemTagsMap.value.get(it.id) ?? [];
  const colors: string[] = [];
  for (const id of ids) {
    const tg = tags.value.find((t) => t.id === id);
    if (tg) {
      colors.push(tg.color || "#409eff");
    }
  }
  return colors;
}

async function refresh() {
  items.value = [];
  hasMore.value = false;
  // 过滤条件切换后回到顶部,避免停留在旧滚动位置看不到新列表头部
  if (mainRef.value) {
    mainRef.value.scrollTop = 0;
  }
  await loadMore();
}

/** 主区滚动容器:切换筛选后复位滚动位置 */
const mainRef = ref<HTMLElement | null>(null);

async function loadMore() {
  if (loading.value || (view.value === "dups")) {
    return;
  }
  loading.value = true;
  try {
    const page = await imagesQuery(currentQuery());
    items.value.push(...page.items);
    hasMore.value = page.hasMore;
    total.value = page.total;
  } catch (e) {
    ElMessage.error(String(e));
  } finally {
    loading.value = false;
  }
}

function onMainScroll(e: Event) {
  const el = e.target as HTMLElement;
  if (hasMore.value && !loading.value && el.scrollTop + el.clientHeight >= el.scrollHeight - 500) {
    void loadMore();
  }
}

watch([view, dirFilter, favOnly, tagFilter, search], () => {
  if (view.value !== "dups") {
    void refresh();
  }
});

/* ---------- 图库目录管理 ---------- */
async function pickDir() {
  try {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string" || !picked) {
      return;
    }
    const dir = await imagesAddDir(picked, false);
    await loadDirs();
    dirFilter.value = dir.id;
    void imagesScanStart(dir.id).catch((e) => ElMessage.error(String(e)));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function addFullDisk() {
  try {
    await ElMessageBox.confirm(t("images.fullDiskConfirm"), t("images.fullDisk"), {
      type: "warning",
      confirmButtonText: t("images.fullDisk"),
    });
  } catch {
    return;
  }
  try {
    const dir = await imagesAddDir("/", true);
    await loadDirs();
    void imagesScanStart(dir.id).catch((e) => ElMessage.error(String(e)));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function removeDir(dir: ImageDirInfo) {
  try {
    await ElMessageBox.confirm(
      t("images.removeDirConfirm", { n: dir.item_count }),
      t("images.removeDir"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  try {
    await imagesRemoveDir(dir.id);
    if (dirFilter.value === dir.id) {
      dirFilter.value = null;
    }
    await loadDirs();
    await refresh();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function rescan(dirId: number) {
  try {
    await imagesScanStart(dirId);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function dirDisplay(path: string): string {
  return path === "/" ? t("images.fullDiskLabel") : path;
}

/* ---------- 事件监听 ---------- */
let unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  await syncAppearance();
  await Promise.all([loadDirs(), loadTags(), refresh()]);

  unlisteners.push(
    await listen<ImageScanProgress>("images://scan-progress", (e) => {
      const p = e.payload;
      if (p.done) {
        delete scanState[p.dirId];
        // 新增/清理都刷新网格:否则扫描出的新照片不出现(计数更新但列表不变)
        if (p.removed > 0 || p.added > 0) {
          void refresh();
        }
        void loadDirs();
        ElMessage.success(
          t("images.scanDone", { n: p.scanned, a: p.added, u: p.updated, r: p.removed }),
        );
      } else {
        scanState[p.dirId] = {
          scanned: p.scanned,
          added: p.added,
          updated: p.updated,
          thumbsDone: p.thumbsDone,
          thumbsTotal: p.thumbsTotal,
        };
      }
    }),
    await listen<{ id: number }>("images://thumb-ready", (e) => {
      thumbTick[e.payload.id] = (thumbTick[e.payload.id] ?? 0) + 1;
    }),
  );
  unlisteners.push(
    await listen<{ total: number; done: number; doneFlag: boolean }>("images://dedup-progress", (e) => {
      dedupProgress.total = e.payload.total;
      dedupProgress.done = e.payload.done;
      if (e.payload.doneFlag) {
        dedupRunning.value = false;
        void loadDupGroups();
      }
    }),
  );
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("focus", syncAppearance);

  // 进入时对所有已配置目录自动触发一轮增量扫描(静默,已在扫描中的会被忽略)
  for (const d of dirs.value) {
    void imagesScanStart(d.id).catch(() => {});
  }
});

onBeforeUnmount(() => {
  unlisteners.forEach((u) => u());
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("focus", syncAppearance);
});

/* ---------- 照片网格(按月分组) ---------- */
const monthFormatter = computed(
  () =>
    new Intl.DateTimeFormat(locale.value === "en" ? "en-US" : "zh-CN", {
      year: "numeric",
      month: "long",
    }),
);

const grouped = computed(() => {
  const groups: { key: string; label: string; items: ImageItemInfo[] }[] = [];
  let current: { key: string; label: string; items: ImageItemInfo[] } | null = null;
  for (const it of items.value) {
    const d = new Date(it.taken_at * 1000);
    const key = `${d.getFullYear()}-${d.getMonth() + 1}`;
    if (!current || current.key !== key) {
      current = { key, label: monthFormatter.value.format(d), items: [] };
      groups.push(current);
    }
    current.items.push(it);
  }
  return groups;
});

const photoItems = computed(() => items.value.filter((i) => !i.doc));

/* ---------- 查看器 ---------- */
const viewer = reactive({
  open: false,
  item: null as ImageItemInfo | null,
  tags: [] as ImageTagInfo[],
  scale: 1,
  tx: 0,
  ty: 0,
  infoOpen: false,
});

function openViewer(item: ImageItemInfo) {
  viewer.open = true;
  viewer.item = item;
  viewer.scale = 1;
  viewer.tx = 0;
  viewer.ty = 0;
  void loadDetail(item.id);
}

async function loadDetail(id: number) {
  try {
    const detail = await imagesDetail(id);
    if (detail && viewer.item?.id === id) {
      viewer.item = detail.item;
      viewer.tags = detail.tags;
      const idx = items.value.findIndex((i) => i.id === id);
      if (idx >= 0) {
        items.value[idx] = detail.item;
      }
    }
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function closeViewer() {
  viewer.open = false;
  viewer.item = null;
}

/** 从查看器跳转到照片所在目录:侧栏选中该目录并关闭查看器 */
function goToItemDir() {
  if (!viewer.item) {
    return;
  }
  const dirId = viewer.item.dir_id;
  closeViewer();
  dirFilter.value = dirId;
  favOnly.value = false;
  tagFilter.value = null;
}

const viewerNav = computed(() => {
  const idx = viewer.item ? items.value.findIndex((i) => i.id === viewer.item?.id) : -1;
  return {
    idx,
    hasPrev: idx > 0,
    hasNext: idx >= 0 && idx < items.value.length - 1,
  };
});

async function viewerStep(delta: number) {
  if (!viewer.item) {
    return;
  }
  const idx = items.value.findIndex((i) => i.id === viewer.item?.id);
  const next = items.value[idx + delta];
  if (next) {
    openViewer(next);
    return;
  }
  // 已到已加载列表末尾且还有下一页:加载后继续翻
  if (delta > 0 && idx === items.value.length - 1 && hasMore.value && !loading.value) {
    await loadMore();
    const after = items.value.findIndex((i) => i.id === viewer.item?.id);
    const upcoming = items.value[after + 1];
    if (upcoming) {
      openViewer(upcoming);
    }
  }
}

function onKeydown(e: KeyboardEvent) {
  if (!viewer.open && view.value === "compress" && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "v") {
    e.preventDefault();
    void compPaste();
    return;
  }
  if (!viewer.open) {
    return;
  }
  if (e.key === "Escape") {
    closeViewer();
  } else if (e.key === "ArrowLeft") {
    void viewerStep(-1);
  } else if (e.key === "ArrowRight") {
    void viewerStep(1);
  } else if (e.key.toLowerCase() === "i") {
    viewer.infoOpen = !viewer.infoOpen;
  }
}

function onWheel(e: WheelEvent) {
  e.preventDefault();
  const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
  const next = Math.min(8, Math.max(1, viewer.scale * factor));
  if (next === 1) {
    viewer.tx = 0;
    viewer.ty = 0;
  }
  viewer.scale = next;
}

let panning = false;
let panStart = { x: 0, y: 0, tx: 0, ty: 0 };

function onPanStart(e: PointerEvent) {
  if (viewer.scale <= 1) {
    return;
  }
  panning = true;
  panStart = { x: e.clientX, y: e.clientY, tx: viewer.tx, ty: viewer.ty };
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}

function onPanMove(e: PointerEvent) {
  if (!panning) {
    return;
  }
  viewer.tx = panStart.tx + (e.clientX - panStart.x);
  viewer.ty = panStart.ty + (e.clientY - panStart.y);
}

function onPanEnd() {
  panning = false;
}

const viewerStyle = computed(() => ({
  transform: `translate(${viewer.tx}px, ${viewer.ty}px) scale(${viewer.scale})`,
  cursor: viewer.scale > 1 ? (panning ? "grabbing" : "grab") : "zoom-in",
}));

function fmtBytes(n: number): string {
  if (n < 1024) {
    return `${n} B`;
  }
  if (n < 1024 * 1024) {
    return `${(n / 1024).toFixed(1)} KB`;
  }
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

const kb = fmtBytes;

function decodeImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("decode fail"));
    image.src = src;
  });
}

function fmtDate(ts: number): string {
  if (!ts) {
    return "-";
  }
  return new Intl.DateTimeFormat(locale.value === "en" ? "en-US" : "zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ts * 1000));
}

/* ---------- 查看器操作 ---------- */
async function toggleFav() {
  if (!viewer.item) {
    return;
  }
  const next = !viewer.item.is_fav;
  try {
    await imagesSetFav(viewer.item.id, next);
    viewer.item.is_fav = next;
    const idx = items.value.findIndex((i) => i.id === viewer.item?.id);
    if (idx >= 0) {
      items.value[idx].is_fav = next;
    }
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function copyImage() {
  if (!viewer.item) {
    return;
  }
  try {
    const url = imgSrc("full", viewer.item.id, viewer.item.mtime);
    const img = new Image();
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("decode fail"));
      img.src = url;
    });
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
    ElMessage.success(t("images.copied"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** 查看器内直接压缩:沿用图片压缩引擎,输出到原图所在目录的 compressed/ 子目录 */
async function compressItem() {
  if (!viewer.item) {
    return;
  }
  const parent = viewer.item.path.slice(0, viewer.item.path.lastIndexOf("/")) || "/";
  try {
    const r = await compressImageFile(
      viewer.item.path,
      { quality: 82, maxDim: 0, format: "auto", targetKb: 0 },
      parent,
      false,
    );
    ElMessage.success(
      r.outPath
        ? t("images.compressedDone", { p: r.outPath })
        : t("images.keptOriginal"),
    );
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function removeCurrent() {
  if (!viewer.item) {
    return;
  }
  try {
    await ElMessageBox.confirm(viewer.item.name, t("images.deleteConfirm"), {
      type: "warning",
    });
  } catch {
    return;
  }
  try {
    const id = viewer.item.id;
    await imagesDeleteItem(id);
    items.value = items.value.filter((i) => i.id !== id);
    const next = items.value[Math.max(0, items.value.findIndex((i) => i.id === id) - 1)];
    if (next) {
      openViewer(next);
    } else {
      closeViewer();
    }
    await Promise.all([loadDirs(), loadTags()]);
    ElMessage.success(t("images.deleteDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 标签 ---------- */
const newTagName = ref("");
const tagColor = ref("#409eff");

async function addTag() {
  const name = newTagName.value.trim();
  if (!name) {
    return;
  }
  try {
    await imagesAddTag(name, tagColor.value);
    newTagName.value = "";
    await loadTags();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function removeTag(tag: ImageTagInfo) {
  try {
    await imagesDeleteTag(tag.id);
    if (tagFilter.value === tag.id) {
      tagFilter.value = null;
    }
    await loadTags();
    await refresh();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function toggleItemTag(tag: ImageTagInfo) {
  if (!viewer.item) {
    return;
  }
  const on = !viewer.tags.some((tg) => tg.id === tag.id);
  try {
    await imagesSetItemTag(viewer.item.id, tag.id, on);
    const d = await imagesDetail(viewer.item.id);
    viewer.tags = d?.tags ?? [];
    await loadTags();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 标签编辑(改名/换色) ---------- */
const tagEdit = reactive({ visible: false, id: 0, name: "", color: "#409eff" });

function openTagEdit(tg: ImageTagInfo) {
  tagEdit.id = tg.id;
  tagEdit.name = tg.name;
  tagEdit.color = tg.color || "#409eff";
  tagEdit.visible = true;
}

async function saveTagEdit() {
  const name = tagEdit.name.trim();
  if (!name) {
    ElMessage.warning(t("images.addTag"));
    return;
  }
  try {
    await imagesRenameTag(tagEdit.id, name, tagEdit.color);
    tagEdit.visible = false;
    await loadTags();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 批量打标签(选择模式) ---------- */
const tagDialog = reactive({ visible: false });
const tagDialogName = ref("");

function openTagDialog() {
  if (!selectedIds.value.size) {
    return;
  }
  tagDialogName.value = "";
  tagDialog.visible = true;
}

/** 把标签加到所有选中照片(只增不减) */
async function applyTagToSelected(tag: ImageTagInfo) {
  try {
    for (const id of selectedIds.value) {
      await imagesSetItemTag(id, tag.id, true);
    }
    await loadTags();
    tagDialog.visible = false;
    ElMessage.success(t("images.tagApplied", { n: selectedIds.value.size, name: tag.name }));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function createAndApplyTag() {
  const name = tagDialogName.value.trim();
  if (!name) {
    return;
  }
  try {
    const tag = await imagesAddTag(name, tagColor.value);
    tagDialogName.value = "";
    await loadTags();
    await applyTagToSelected(tag);
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 重复检测 ---------- */
const dupGroups = ref<ImageDupGroup[]>([]);
const dupLoaded = ref(false);
/** 每组勾选待删除的条目(默认保留每组第一张) */
const dupSelection = reactive<Record<string, number[]>>({});

async function startDedup() {
  try {
    dedupRunning.value = true;
    dedupProgress.done = 0;
    dedupProgress.total = 0;
    await imagesDedupStart();
  } catch (e) {
    dedupRunning.value = false;
    ElMessage.error(String(e));
  }
}

async function loadDupGroups() {
  try {
    dupGroups.value = await imagesDupGroups();
    for (const k of Object.keys(dupSelection)) {
      delete dupSelection[k];
    }
    for (const g of dupGroups.value) {
      dupSelection[g.hash] = g.items.slice(1).map((it) => it.id);
    }
    dupLoaded.value = true;
  } catch (e) {
    ElMessage.error(String(e));
  }
}

watch(view, (v) => {
  if (v === "dups" && !dupLoaded.value && !dedupRunning.value) {
    void loadDupGroups();
  }
});

function toggleDupPick(hash: string, id: number) {
  const list = dupSelection[hash];
  if (!list) {
    return;
  }
  const idx = list.indexOf(id);
  if (idx >= 0) {
    list.splice(idx, 1);
  } else {
    list.push(id);
  }
}

async function deleteSelectedDups() {
  const paths: string[] = [];
  const ids: number[] = [];
  for (const g of dupGroups.value) {
    for (const it of g.items) {
      if (dupSelection[g.hash]?.includes(it.id)) {
        paths.push(it.path);
        ids.push(it.id);
      }
    }
  }
  if (!ids.length) {
    ElMessage.warning(t("images.dupNoneSelected"));
    return;
  }
  // 每组必须保留至少一张
  const invalid = dupGroups.value.some((g) => g.items.every((it) => dupSelection[g.hash]?.includes(it.id)));
  if (invalid) {
    ElMessage.warning(t("images.mustKeep"));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("images.deleteSelectedConfirm", { n: ids.length }),
      t("images.deleteSelected"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  try {
    await imagesDeletePaths(paths, ids);
    await loadDupGroups();
    await Promise.all([loadDirs(), refresh()]);
    ElMessage.success(t("images.deleteDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/* ---------- 高级压缩视图 ---------- */
const COMP_EXTS = ["jpg", "jpeg", "png", "webp", "gif", "bmp"];
const PRESET_QUALITY: Record<string, number> = {
  balanced: 82,
  high: 92,
  small: 45,
};

function fmtToApi(v: string): CompressImageParams["format"] {
  if (v === "auto" || v === "keep") {
    return v;
  }
  return v.replace("image/", "") as CompressImageParams["format"];
}

interface CompJob {
  key: string;
  kind: "file" | "clipboard";
  path?: string;
  dataUrl?: string;
  name: string;
  size: number;
}
interface CompRow {
  key: string;
  name: string;
  kind: "file" | "clipboard";
  srcBytes: number;
  outBytes: number;
  percent: number;
  kept: boolean;
  saved: boolean;
  failed: boolean;
  error: string;
  outPath: string;
  dataUrl: string;
}

const compJobs = ref<CompJob[]>([]);
const compRunning = ref(false);
const compSingle = ref<{
  name: string;
  dataUrl: string;
  size: number;
  width: number;
  height: number;
  format: string;
  kept: boolean;
} | null>(null);
const compRows = ref<CompRow[]>([]);
const compOutDirs = ref<string[]>([]);
const compWmFileInputRef = ref<HTMLInputElement | null>(null);

const compPreset = ref<keyof typeof PRESET_QUALITY | "custom">("balanced");
const compQuality = ref(82);
const compMaxDim = ref(0);
const compFormat = ref("auto");
const compTargetKb = ref(0);
const compWmKind = ref<"none" | "text" | "image" | "blind">("none");
const compWmText = ref("");
const compWmFontSize = ref(24);
const compWmColor = ref("#ffffff");
const compWmOpacity = ref(30);
const compWmRotation = ref(-30);
const compWmTiled = ref(true);
const compWmDensity = ref(55);
const compWmPosition = ref("bottom-right");
const compWmImageDataUrl = ref("");
const compWmImageName = ref("");
const compWmImageScale = ref(25);
const compWmStrength = ref(50);
const compAdvancedOpen = ref(false);

const compParams = computed<CompressImageParams>(() => {
  const quality =
    compPreset.value === "custom" ? compQuality.value : PRESET_QUALITY[compPreset.value];
  const p: CompressImageParams = {
    quality,
    maxDim: compMaxDim.value,
    format: fmtToApi(compFormat.value),
    // el-input-number 清空时为 null,归零避免后端反序列化失败
    targetKb: compTargetKb.value || 0,
  };
  if (compWmKind.value !== "none") {
    p.watermark = {
      kind: compWmKind.value,
      text: compWmText.value,
      fontSize: compWmFontSize.value || 24,
      color: compWmColor.value,
      opacity: compWmOpacity.value,
      rotation: compWmRotation.value,
      tiled: compWmTiled.value,
      density: compWmDensity.value,
      position: compWmPosition.value,
      imageDataUrl: compWmImageDataUrl.value,
      imageScale: compWmImageScale.value,
      strength: compWmStrength.value,
    };
  }
  return p;
});

function parentDirOf(p: string): string {
  return p.slice(0, p.lastIndexOf("/")) || "/";
}

function pushFileJob(path: string) {
  if (compJobs.value.some((j) => j.path === path)) {
    return;
  }
  const name = path.slice(path.lastIndexOf("/") + 1);
  compJobs.value.push({
    key: `f:${path}`,
    kind: "file",
    path,
    name,
    size: 0,
  });
}

async function compAddFiles() {
  try {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Images", extensions: COMP_EXTS }],
    });
    if (!picked) {
      return;
    }
    for (const p of Array.isArray(picked) ? picked : [picked]) {
      pushFileJob(p);
    }
    compAutoRun();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function compAddDir() {
  try {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string" || !picked) {
      return;
    }
    const files = await listImageFiles(picked);
    let n = 0;
    for (const f of files) {
      const ext = f.name.slice(f.name.lastIndexOf(".") + 1).toLowerCase();
      if (COMP_EXTS.includes(ext)) {
        pushFileJob(`${picked}/${f.name}`);
        n += 1;
      }
    }
    if (!n) {
      ElMessage.warning(t("images.compDirEmpty"));
      return;
    }
    compAutoRun();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function compPaste() {
  try {
    const img = await getClipboardImage();
    if (!img) {
      ElMessage.warning(t("toolbox.tools.imgcompress.noImage"));
      return;
    }
    const canvas = document.createElement("canvas");
    canvas.width = img.width;
    canvas.height = img.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    ctx.putImageData(new ImageData(new Uint8ClampedArray(img.rgba), img.width, img.height), 0, 0);
    const dataUrl = canvas.toDataURL("image/png");
    const key = `c:${Date.now()}`;
    compJobs.value.push({
      key,
      kind: "clipboard",
      dataUrl,
      name: t("images.compClipboardName"),
      size: Math.round(dataUrl.length * 0.75),
    });
    compAutoRun();
  } catch (e) {
    ElMessage.error(String(e));
  }
}

function compRemoveJob(key: string) {
  compJobs.value = compJobs.value.filter((j) => j.key !== key);
}

function compClear() {
  compJobs.value = [];
  compSingle.value = null;
  compRows.value = [];
  compOutDirs.value = [];
}

function compAutoRun() {
  if (compJobs.value.length === 1) {
    void compRun();
  }
}

async function compRun() {
  if (!compJobs.value.length || compRunning.value) {
    return;
  }
  compRunning.value = true;
  compSingle.value = null;
  compRows.value = [];
  compOutDirs.value = [];
  try {
    if (compJobs.value.length === 1) {
      const j = compJobs.value[0];
      const r =
        j.kind === "clipboard"
          ? await compressImageData(j.dataUrl!, compParams.value)
          : await compressImageFile(j.path!, compParams.value, null, true);
      compSingle.value = {
        name: j.name,
        dataUrl: r.dataUrl ?? "",
        size: r.size,
        width: r.width,
        height: r.height,
        format: r.format,
        kept: r.kept,
      };
    } else {
      for (const j of compJobs.value) {
        compRows.value.push({
          key: j.key,
          name: j.name,
          kind: j.kind,
          srcBytes: j.size,
          outBytes: 0,
          percent: 0,
          kept: false,
          saved: false,
          failed: false,
          error: "",
          outPath: "",
          dataUrl: "",
        });
        // 取响应式代理再修改,原始对象赋值不会触发更新
        const row = compRows.value[compRows.value.length - 1];
        try {
          if (j.kind === "file") {
            const out = parentDirOf(j.path!);
            if (!compOutDirs.value.includes(out)) {
              compOutDirs.value.push(out);
            }
            const r = await compressImageFile(j.path!, compParams.value, out, false);
            row.outBytes = r.kept ? j.size : r.size;
            row.kept = r.kept;
            row.saved = true;
            row.outPath = r.outPath ?? "";
          } else {
            const r = await compressImageData(j.dataUrl!, compParams.value);
            row.outBytes = r.kept ? j.size : r.size;
            row.kept = r.kept;
            row.saved = true;
            row.dataUrl = r.dataUrl ?? "";
          }
          row.percent =
            !row.kept && j.size > 0 && row.outBytes > 0
              ? Math.round((1 - row.outBytes / j.size) * 100)
              : 0;
        } catch (e) {
          row.failed = true;
          row.error = String(e);
        }
      }
    }
  } finally {
    compRunning.value = false;
  }
}

async function compSaveSingle() {
  if (!compSingle.value?.dataUrl) {
    return;
  }
  try {
    await saveImage(compSingle.value.dataUrl);
    ElMessage.success(t("toolbox.tools.imgcompress.savedDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

async function compCopySingle() {
  if (!compSingle.value?.dataUrl) {
    return;
  }
  try {
    const img = await decodeImage(compSingle.value.dataUrl);
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

async function compSaveRow(row: CompRow) {
  if (!row.dataUrl) {
    return;
  }
  try {
    await saveImage(row.dataUrl);
    ElMessage.success(t("toolbox.tools.imgcompress.savedDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

const compSummary = computed(() => {
  const ready = compRows.value.filter((r) => !r.failed);
  if (!ready.length) {
    return null;
  }
  const srcBytes = ready.reduce((s, r) => s + r.srcBytes, 0);
  const outBytes = ready.reduce((s, r) => s + r.outBytes, 0);
  const savedPct = srcBytes > 0 && outBytes < srcBytes ? Math.round((1 - outBytes / srcBytes) * 100) : 0;
  return { n: ready.length, src: srcBytes, out: outBytes, savedPct };
});

function compOpenOut() {
  if (compOutDirs.value.length) {
    void imagesRevealPath(`${compOutDirs.value[0]}/compressed`);
  }
}

function onWmFileChangeComp(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file || !file.type.startsWith("image/")) {
    input.value = "";
    return;
  }
  const reader = new FileReader();
  reader.onload = () => {
    compWmImageDataUrl.value = String(reader.result ?? "");
    compWmImageName.value = file.name;
  };
  reader.readAsDataURL(file);
  input.value = "";
}

// 单任务时参数变化自动重跑(防抖)
let compTimer: ReturnType<typeof setTimeout> | undefined;
watch(compParams, () => {
  if (view.value === "compress" && compJobs.value.length === 1 && !compRunning.value) {
    clearTimeout(compTimer);
    compTimer = setTimeout(() => void compRun(), 300);
  }
});

/* ---------- 照片多选 → 压缩所选 ---------- */
const selectMode = ref(false);
const selectedIds = ref<Set<number>>(new Set());

function toggleSelectMode() {
  selectMode.value = !selectMode.value;
  if (!selectMode.value) {
    selectedIds.value = new Set();
  }
}

function toggleSelect(it: ImageItemInfo) {
  const s = new Set(selectedIds.value);
  if (s.has(it.id)) {
    s.delete(it.id);
  } else {
    s.add(it.id);
  }
  selectedIds.value = s;
}

function compressSelected() {
  const jobs = photoItems.value
    .filter((i) => selectedIds.value.has(i.id))
    .map<CompJob>((i) => ({
      key: `f:${i.path}`,
      kind: "file",
      path: i.path,
      name: i.name,
      size: i.size,
    }));
  if (!jobs.length) {
    return;
  }
  compJobs.value = jobs;
  selectMode.value = false;
  selectedIds.value = new Set();
  view.value = "compress";
  void compRun();
}

async function favSelected() {
  try {
    for (const id of selectedIds.value) {
      await imagesSetFav(id, true);
    }
    const ids = selectedIds.value;
    items.value.forEach((i) => {
      if (ids.has(i.id)) {
        i.is_fav = true;
      }
    });
    ElMessage.success(t("images.favDone"));
  } catch (e) {
    ElMessage.error(String(e));
  }
}

/** 删除所选照片:逐张移废纸篓(每张独立成败),成功后本地剔除 */
async function deleteSelectedPhotos() {
  if (!selectedIds.value.size) {
    return;
  }
  try {
    await ElMessageBox.confirm(
      t("images.deleteSelectedConfirm", { n: selectedIds.value.size }),
      t("images.deleteSelected"),
      { type: "warning" },
    );
  } catch {
    return;
  }
  const done = new Set<number>();
  let failed = 0;
  let lastErr = "";
  for (const id of selectedIds.value) {
    try {
      await imagesDeleteItem(id);
      done.add(id);
    } catch (e) {
      failed++;
      lastErr = String(e);
    }
  }
  if (done.size) {
    items.value = items.value.filter((i) => !done.has(i.id));
    if (viewer.item && done.has(viewer.item.id)) {
      closeViewer();
    }
    await Promise.all([loadDirs(), loadTags()]);
  }
  selectMode.value = false;
  selectedIds.value = new Set();
  if (!failed) {
    ElMessage.success(t("images.deleteDone"));
  } else if (done.size) {
    ElMessage.warning(t("images.dupPartialFail", { n: failed }));
  } else {
    ElMessage.error(lastErr);
  }
}

</script>

<template>
  <div class="w-screen h-screen bg-[var(--el-bg-color)] text-dp-primary overflow-hidden select-none">
    <!-- 顶栏(拖拽区) -->
    <div
      class="h-12 shrink-0 flex items-center gap-3 px-4 bg-dp-card-bg border-b border-dp-divider"
      data-tauri-drag-region
    >
      <span class="text-sm font-semibold text-dp-primary" data-tauri-drag-region>
        {{ t("images.title") }}
      </span>
      <el-radio-group v-model="view" size="small" data-tauri-drag-region>
        <el-radio-button value="photos">{{ t("images.viewPhotos") }}</el-radio-button>
        <el-radio-button value="compress">{{ t("images.viewCompress") }}</el-radio-button>
        <el-radio-button value="dups">{{ t("images.viewDups") }}</el-radio-button>
      </el-radio-group>
      <el-input
        v-model="searchInput"
        size="small"
        clearable
        :prefix-icon="Search"
        :placeholder="t('images.searchPh')"
        class="w-56 ml-auto"
      />
      <span
        v-if="view === 'photos' && total > 0"
        class="text-[11px] text-dp-tertiary whitespace-nowrap"
      >
        {{ t("images.totalCount", { n: total }) }}
      </span>
      <el-button
        v-if="view === 'photos'"
        size="small"
        text
        :type="selectMode ? 'primary' : ''"
        @click="toggleSelectMode"
      >
        {{ selectMode ? t("images.compExitSelect") : t("images.compSelect") }}
      </el-button>
      <span
        v-if="scanChip.active"
        class="flex items-center gap-1.5 text-[11px] text-dp-tertiary whitespace-nowrap"
      >
        <el-icon class="is-loading" :size="12"><Loading /></el-icon>
        <span>
          {{
            scanChip.thumbsTotal > 0
              ? t("images.scanChipFull", {
                  s: scanChip.scanned,
                  d: scanChip.thumbsDone,
                  t: scanChip.thumbsTotal,
                })
              : t("images.scanChipIndex", { s: scanChip.scanned })
          }}
        </span>
        <el-icon
          class="cursor-pointer hover:text-dp-muted"
          :size="12"
          :title="t('images.scanCancel')"
          @click="cancelAllScans"
        >
          <Close />
        </el-icon>
      </span>
      <el-button size="small" :icon="Close" circle text @click="closeWindow" />
    </div>

    <div class="flex h-[calc(100%-3rem)]">
      <!-- 侧边栏 -->
      <aside class="w-52 shrink-0 border-r border-dp-divider bg-dp-card-bg flex flex-col overflow-y-auto">
        <div class="p-2.5">
          <div class="flex items-center gap-1.5">
            <span class="text-[11px] font-medium text-dp-muted flex-1">
              {{ t("images.libraries") }}
            </span>
            <el-tooltip :content="t('images.addDir')">
              <el-button size="small" :icon="FolderAdd" circle text @click="pickDir" />
            </el-tooltip>
          </div>
          <div
            v-for="d in dirs"
            :key="d.id"
            class="group flex items-center gap-1 px-1.5 py-1 rounded-md cursor-pointer text-[11px] leading-[1.6]"
            :class="dirFilter === d.id && !favOnly && !tagFilter ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] text-dp-accent' : 'text-dp-muted hover:bg-dp-btn-bg'"
            @click="dirFilter = d.id; favOnly = false; tagFilter = null"
          >
            <el-icon
              v-if="scanning[d.id]"
              class="is-loading shrink-0"
              :size="11"
              :title="t('images.scanning')"
              @click.stop="imagesTaskCancel(d.id)"
            >
              <Loading />
            </el-icon>
            <el-icon v-else class="shrink-0" :size="11"><Picture /></el-icon>
            <span class="truncate flex-1 min-w-0" :title="d.path">{{ dirDisplay(d.path) }}</span>
            <span class="shrink-0 text-[10px] text-dp-tertiary">{{ d.item_count }}</span>
            <span class="hidden group-hover:flex items-center shrink-0">
              <el-icon :size="11" class="hover:text-dp-accent" :title="t('images.rescan')" @click.stop="rescan(d.id)">
                <Refresh />
              </el-icon>
              <el-icon :size="11" class="hover:text-[var(--el-color-danger,#f56c6c)]" :title="t('images.removeDir')" @click.stop="removeDir(d)">
                <Close />
              </el-icon>
            </span>
          </div>
          <button
            class="w-full mt-1 px-1.5 py-1 rounded-md text-left text-[11px] text-dp-tertiary hover:bg-dp-btn-bg hover:text-dp-muted flex items-center gap-1.5"
            @click="addFullDisk"
          >
            <el-icon :size="11"><Plus /></el-icon>
            {{ t("images.fullDisk") }}
          </button>
        </div>

        <div class="p-2.5 border-t border-dp-divider">
          <button
            class="w-full flex items-center gap-1.5 px-1.5 py-1 rounded-md text-[11px] leading-[1.6]"
            :class="favOnly ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] text-dp-accent' : 'text-dp-muted hover:bg-dp-btn-bg'"
            @click="favOnly = !favOnly; dirFilter = null; tagFilter = null"
          >
            <el-icon :size="11">
              <StarFilled v-if="favOnly" />
              <Star v-else />
            </el-icon>
            {{ t("images.favorites") }}
          </button>
        </div>

        <div class="p-2.5 border-t border-dp-divider flex-1">
          <div class="flex items-center gap-1.5 mb-1">
            <span class="text-[11px] font-medium text-dp-muted flex-1">{{ t("images.tags") }}</span>
          </div>
          <div class="text-[10px] text-dp-tertiary leading-[1.5] mb-1.5">
            {{ t("images.tagHint") }}
          </div>
          <div class="flex items-center gap-1 mb-1.5">
            <el-input
              v-model="newTagName"
              size="small"
              :placeholder="t('images.addTag')"
              class="flex-1 min-w-0 [&_.el-input\_\_inner]:text-[11px] [&_.el-input\_\_inner]:h-[22px]"
              @keyup.enter="addTag"
            />
            <el-color-picker v-model="tagColor" size="small" />
            <el-button size="small" :icon="Plus" circle text @click="addTag" />
          </div>
          <div
            v-for="tg in tags"
            :key="tg.id"
            class="group flex items-center gap-1 px-1.5 py-1 rounded-md cursor-pointer text-[11px]"
            :class="tagFilter === tg.id ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] text-dp-accent' : 'text-dp-muted hover:bg-dp-btn-bg'"
            @click="tagFilter = tg.id; favOnly = false; dirFilter = null"
          >
            <span
              class="w-2.5 h-2.5 rounded-full shrink-0 cursor-pointer border border-white/40 hover:scale-125 transition-transform"
              :style="{ background: tg.color || '#409eff' }"
              :title="t('images.tagDotTip')"
              @click.stop="openTagEdit(tg)"
            />
            <span class="truncate flex-1 min-w-0">{{ tg.name }}</span>
            <span class="shrink-0 text-[10px] text-dp-tertiary">{{ tg.item_count }}</span>
            <el-icon
              class="hidden group-hover:block shrink-0 hover:text-[var(--el-color-danger,#f56c6c)]"
              :size="11"
              @click.stop="removeTag(tg)"
            >
              <Close />
            </el-icon>
          </div>
          <div v-if="!tags.length" class="text-[10.5px] text-dp-tertiary px-1.5">
            {{ t("images.noTags") }}
          </div>
        </div>
      </aside>

      <!-- 主区 -->
      <main ref="mainRef" class="flex-1 min-w-0 overflow-y-auto view-scroll" @scroll="onMainScroll">
        <!-- 照片:按月分组网格 -->
        <template v-if="view === 'photos'">
          <template v-if="photoItems.length">
            <section v-for="g in grouped" :key="g.key">
              <div class="sticky top-0 z-10 px-4 py-1.5 bg-dp-panel-bg/90 backdrop-blur text-xs font-medium text-dp-muted border-b border-dp-divider/60">
                {{ g.label }}
                <span class="text-dp-tertiary text-[10.5px] ml-1">{{ g.items.length }}</span>
              </div>
              <div class="p-3 grid gap-1.5 grid-cols-[repeat(auto-fill,minmax(118px,1fr))]">
                <button
                  v-for="it in g.items"
                  :key="it.id"
                  class="relative aspect-square rounded-lg overflow-hidden bg-dp-btn-bg group border"
                  :class="selectMode && selectedIds.has(it.id) ? 'border-dp-accent' : 'border-dp-card-border'"
                  @click="selectMode ? toggleSelect(it) : openViewer(it)"
                >
                  <img
                    :src="imgSrc('thumb', it.id, imgV(it))"
                    loading="lazy"
                    class="w-full h-full object-cover transition-transform group-hover:scale-[1.03]"
                    :class="selectMode && selectedIds.has(it.id) ? 'opacity-75' : ''"
                    alt=""
                  />
                  <span
                    v-if="selectMode && selectedIds.has(it.id)"
                    class="absolute top-1 left-1 w-4 h-4 rounded-full bg-dp-accent text-white flex items-center justify-center"
                  >
                    <el-icon :size="10"><Check /></el-icon>
                  </span>
                  <span
                    v-else
                    class="absolute top-1 left-1 hidden group-hover:flex w-5 h-5 rounded-full bg-black/45 items-center justify-center text-white/90 cursor-pointer"
                    :title="`${t('images.locate')} · ${parentDirOf(it.path)}`"
                    @click.stop="imagesRevealPath(it.path)"
                  >
                    <el-icon :size="11"><FolderOpened /></el-icon>
                  </span>
                  <span
                    v-if="itemTagColors(it).length"
                    class="absolute bottom-1 left-1 flex gap-0.5"
                  >
                    <span
                      v-for="(c, ci) in itemTagColors(it).slice(0, 3)"
                      :key="ci"
                      class="w-1.5 h-1.5 rounded-full border border-white/80"
                      :style="{ background: c }"
                    />
                  </span>
                  <el-icon
                    v-if="it.is_fav"
                    class="absolute top-1 right-1 text-[#f7ba2a] drop-shadow"
                    :size="13"
                  >
                    <StarFilled />
                  </el-icon>
                </button>
              </div>
            </section>
            <div v-if="loading" class="py-6 flex justify-center text-dp-tertiary">
              <el-icon class="is-loading" :size="18"><Loading /></el-icon>
            </div>
            <div
              v-if="selectMode"
              class="sticky bottom-2 z-20 mx-3 flex items-center gap-2 px-3 py-2 rounded-xl bg-dp-card-bg border border-dp-card-border shadow-[0_4px_16px_rgba(31,41,55,0.2)]"
            >
              <span class="text-xs text-dp-muted flex-1">
                {{ t("images.compSelected", { n: selectedIds.size }) }}
              </span>
              <el-button size="small" type="primary" :disabled="!selectedIds.size" @click="compressSelected">
                {{ t("images.compCompressSelected") }}
              </el-button>
              <el-button size="small" :disabled="!selectedIds.size" @click="openTagDialog">
                {{ t("images.tagForSelected") }}
              </el-button>
              <el-button size="small" :disabled="!selectedIds.size" @click="favSelected">
                {{ t("images.compFavSelected") }}
              </el-button>
              <el-button size="small" type="danger" :disabled="!selectedIds.size" @click="deleteSelectedPhotos">
                {{ t("images.deleteSelected") }}
              </el-button>
            </div>
          </template>
          <el-empty v-else-if="!loading" :description="t('images.emptyHint')" :image-size="72">
            <el-button type="primary" size="small" :icon="FolderAdd" @click="pickDir">
              {{ t("images.addDir") }}
            </el-button>
          </el-empty>
        </template>

        <!-- 高级压缩:顶部选项 + 全宽结果 -->
        <template v-else-if="view === 'compress'">
          <div class="p-3 flex flex-col gap-2.5">
            <div class="p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px] flex flex-col gap-2.5">
              <!-- 导入 + 开始压缩 -->
              <div class="flex items-center gap-2">
                <el-button size="small" type="primary" @click="compAddFiles">
                  {{ t("images.compImportFiles") }}
                </el-button>
                <el-button size="small" @click="compAddDir">
                  {{ t("images.compImportDir") }}
                </el-button>
                <el-button size="small" :icon="Picture" @click="compPaste">
                  {{ t("images.compPaste") }}
                </el-button>
                <el-button size="small" :disabled="!compJobs.length" @click="compClear">
                  {{ t("images.compClear") }}
                </el-button>
                <span
                  class="text-[10px] leading-[1.5] text-dp-tertiary truncate min-w-0"
                  :title="t('images.compPasteHint')"
                >
                  {{ t("images.compPasteHint") }}
                </span>
                <el-button
                  size="small"
                  type="primary"
                  :loading="compRunning"
                  :disabled="!compJobs.length"
                  class="ml-auto shrink-0"
                  @click="compRun"
                >
                  {{ compRunning
                    ? t("toolbox.tools.imgcompress.batchRunning")
                    : t("images.compStart", { n: compJobs.length }) }}
                </el-button>
              </div>

              <!-- 队列 -->
              <div v-if="compJobs.length" class="flex flex-wrap gap-1.5 max-h-[52px] overflow-y-auto">
                <span
                  v-for="j in compJobs"
                  :key="j.key"
                  class="group inline-flex items-center gap-1 pl-1.5 pr-1 py-1 rounded-md bg-dp-btn-bg shrink-0"
                  :title="j.path || j.name"
                >
                  <span class="truncate text-[11px] text-dp-muted max-w-[150px]">{{ j.name }}</span>
                  <span class="shrink-0 text-[10px] text-dp-tertiary">{{ kb(j.size) }}</span>
                  <el-icon
                    class="shrink-0 text-dp-tertiary hover:text-[var(--el-color-danger,#f56c6c)] cursor-pointer"
                    :size="11"
                    @click="compRemoveJob(j.key)"
                  >
                    <Close />
                  </el-icon>
                </span>
              </div>

              <!-- 参数(横排) -->
              <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
                <div class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.preset") }}</span>
                  <el-select v-model="compPreset" size="small" class="w-[104px]">
                    <el-option :label="t('toolbox.tools.imgcompress.presetBalanced')" value="balanced" />
                    <el-option :label="t('toolbox.tools.imgcompress.presetHigh')" value="high" />
                    <el-option :label="t('toolbox.tools.imgcompress.presetSmall')" value="small" />
                    <el-option :label="t('toolbox.tools.imgcompress.presetCustom')" value="custom" />
                  </el-select>
                </div>
                <div v-if="compPreset === 'custom'" class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.quality") }}</span>
                  <el-slider v-model="compQuality" :min="1" :max="100" class="w-[140px]" />
                  <span class="text-xs text-dp-accent shrink-0 min-w-[36px] text-right">{{ compQuality }}%</span>
                </div>
                <div class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.maxDim") }}</span>
                  <el-radio-group v-model="compMaxDim" size="small">
                    <el-radio-button :value="0">{{ t("toolbox.tools.imgcompress.noLimit") }}</el-radio-button>
                    <el-radio-button :value="1920">1920</el-radio-button>
                    <el-radio-button :value="1280">1280</el-radio-button>
                    <el-radio-button :value="720">720</el-radio-button>
                  </el-radio-group>
                </div>
                <div class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.format") }}</span>
                  <el-select v-model="compFormat" size="small" class="w-[104px]">
                    <el-option :label="t('toolbox.tools.imgcompress.fmtAuto')" value="auto" />
                    <el-option :label="t('toolbox.tools.imgcompress.fmtOriginal')" value="keep" />
                    <el-option label="JPEG" value="image/jpeg" />
                    <el-option label="PNG" value="image/png" />
                    <el-option label="WebP" value="image/webp" />
                  </el-select>
                </div>
                <div class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.targetSize") }}</span>
                  <el-input-number
                    v-model="compTargetKb"
                    size="small"
                    :min="0"
                    :max="102400"
                    :step="50"
                    :controls="false"
                    class="w-[72px] [&_.el-input\_\_inner]:text-[11.5px]"
                  />
                  <span class="text-[10px] text-dp-tertiary shrink-0">KB</span>
                  <span
                    class="text-[10px] text-dp-tertiary truncate max-w-[180px]"
                    :title="t('images.compTargetHint')"
                  >
                    {{ t("images.compTargetHint") }}
                  </span>
                </div>
                <div
                  class="flex items-center gap-1 cursor-pointer select-none ml-auto"
                  @click="compAdvancedOpen = !compAdvancedOpen"
                >
                  <span class="text-xs text-dp-muted">{{ t("toolbox.tools.imgcompress.advanced") }}</span>
                  <el-icon
                    :size="12"
                    class="text-dp-muted transition-transform duration-200"
                    :class="compAdvancedOpen ? 'rotate-180' : ''"
                  >
                    <ArrowDown />
                  </el-icon>
                </div>
              </div>

              <!-- 高级:水印 -->
              <div v-show="compAdvancedOpen" class="flex flex-col gap-2">
                <div class="text-[11px] font-medium text-dp-muted flex items-center gap-1.5">
                  <span class="w-1 h-3 rounded-full bg-dp-accent/70"></span>
                  {{ t("images.compSecWatermark") }}
                </div>
                <div class="flex items-center gap-2">
                  <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.watermark") }}</span>
                  <el-select v-model="compWmKind" size="small" class="w-[104px]">
                    <el-option :label="t('toolbox.tools.imgcompress.wmOff')" value="none" />
                    <el-option :label="t('toolbox.tools.imgcompress.wmText')" value="text" />
                    <el-option :label="t('toolbox.tools.imgcompress.wmImage')" value="image" />
                    <el-option :label="t('toolbox.tools.imgcompress.wmBlind')" value="blind" />
                  </el-select>
                </div>
                <div
                  v-if="compWmKind !== 'none'"
                  class="p-2 bg-dp-panel-bg border border-dp-divider rounded-lg flex flex-wrap items-center gap-x-4 gap-y-2"
                >
                  <div class="flex items-center gap-2">
                    <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmContent") }}</span>
                    <el-input
                      v-model="compWmText"
                      size="small"
                      class="w-[200px] [&_.el-input\_\_inner]:text-[11.5px]"
                    />
                    <el-color-picker v-model="compWmColor" size="small" />
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmOpacity") }}</span>
                    <el-slider v-model="compWmOpacity" :min="0" :max="100" class="w-[150px]" />
                    <span class="text-[10.5px] text-dp-accent w-[30px] text-right shrink-0">{{ compWmOpacity }}%</span>
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmRotation") }}</span>
                    <el-slider v-model="compWmRotation" :min="-90" :max="90" class="w-[150px]" />
                    <span class="text-[10.5px] text-dp-accent w-[30px] text-right shrink-0">{{ compWmRotation }}°</span>
                  </div>
                  <template v-if="compWmKind !== 'blind'">
                    <div class="flex items-center gap-2">
                      <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmLayout") }}</span>
                      <el-radio-group v-model="compWmTiled" size="small">
                        <el-radio-button :value="true">{{ t("toolbox.tools.imgcompress.wmTile") }}</el-radio-button>
                        <el-radio-button :value="false">{{ t("toolbox.tools.imgcompress.wmSingle") }}</el-radio-button>
                      </el-radio-group>
                      <template v-if="compWmKind === 'text'">
                        <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmFontSize") }}</span>
                        <el-input-number
                          v-model="compWmFontSize"
                          size="small"
                          :min="8"
                          :max="200"
                          :controls="false"
                          class="w-[52px] [&_.el-input\_\_inner]:text-[11.5px]"
                        />
                      </template>
                    </div>
                    <div v-if="compWmTiled" class="flex items-center gap-2">
                      <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmDensity") }}</span>
                      <el-slider v-model="compWmDensity" :min="5" :max="95" class="w-[150px]" />
                    </div>
                    <div v-else class="flex items-center gap-2">
                      <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmPosition") }}</span>
                      <el-select v-model="compWmPosition" size="small" class="w-[92px]">
                        <el-option :label="t('toolbox.tools.imgcompress.posCenter')" value="center" />
                        <el-option :label="t('toolbox.tools.imgcompress.posTopLeft')" value="top-left" />
                        <el-option :label="t('toolbox.tools.imgcompress.posTopRight')" value="top-right" />
                        <el-option :label="t('toolbox.tools.imgcompress.posBottomLeft')" value="bottom-left" />
                        <el-option :label="t('toolbox.tools.imgcompress.posBottomRight')" value="bottom-right" />
                      </el-select>
                      <template v-if="compWmKind === 'image'">
                        <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmScale") }}</span>
                        <el-slider v-model="compWmImageScale" :min="5" :max="100" class="w-[150px]" />
                      </template>
                    </div>
                  </template>
                  <template v-else>
                    <div class="flex items-center gap-2">
                      <span class="text-[11px] text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmStrength") }}</span>
                      <el-slider v-model="compWmStrength" :min="1" :max="100" class="w-[150px]" />
                    </div>
                    <div class="text-[10px] leading-[1.5] text-dp-tertiary">
                      {{ t("toolbox.tools.imgcompress.wmBlindHint") }}
                    </div>
                  </template>
                  <div v-if="compWmKind === 'image'" class="flex items-center gap-2">
                    <el-button size="small" :icon="Picture" @click="compWmFileInputRef?.click()">
                      {{ t("toolbox.tools.imgcompress.wmPickImage") }}
                    </el-button>
                    <el-icon v-if="compWmImageDataUrl" class="text-[var(--el-color-success,#67c23a)]" :size="12">
                      <Check />
                    </el-icon>
                    <span v-if="compWmImageName" class="text-[10.5px] text-dp-tertiary truncate max-w-[180px]">
                      {{ compWmImageName }}
                    </span>
                    <input
                      ref="compWmFileInputRef"
                      type="file"
                      accept="image/*"
                      class="hidden"
                      @change="onWmFileChangeComp"
                    />
                  </div>
                </div>
              </div>

              <div class="text-[10px] leading-[1.5] text-dp-tertiary">
                {{ t("images.compBatchOut") }}
              </div>
            </div>

            <!-- 结果区 -->
            <div class="flex-1 min-w-0 flex flex-col gap-2.5">
              <template v-if="compSingle">
                <div
                  class="rounded-[10px] border border-dp-card-border bg-dp-card-bg bg-[repeating-conic-gradient(var(--dp-divider)_0%_25%,transparent_0%_50%)] bg-[length:16px_16px] bg-center bg-no-repeat p-4 flex items-center justify-center min-h-[240px]"
                >
                  <img
                    :src="compSingle.dataUrl"
                    class="max-w-full max-h-[420px] rounded-md shadow-[0_4px_16px_rgba(31,41,55,0.18)]"
                    alt="compressed"
                  />
                </div>
                <div class="px-3 py-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
                  <div class="flex items-center gap-2 mb-1">
                    <span class="text-xs text-dp-primary font-medium truncate flex-1 min-w-0">{{ compSingle.name }}</span>
                    <span
                      v-if="!compSingle.kept"
                      class="px-2 py-0.5 rounded-full bg-[var(--el-color-success-light-9,#f0f9eb)] text-[var(--el-color-success,#67c23a)] text-[10.5px] font-medium shrink-0"
                    >
                      {{ t("images.compDoneBadge") }}
                    </span>
                  </div>
                  <div class="flex justify-between text-xs leading-[1.8]">
                    <span class="text-dp-tertiary">{{ t("toolbox.tools.imgcompress.compressed") }}</span>
                    <span class="text-dp-accent font-medium">
                      {{ compSingle.width }}×{{ compSingle.height }} · {{ kb(compSingle.size) }} ·
                      {{ compSingle.format }}
                    </span>
                  </div>
                  <div
                    v-if="compSingle.kept"
                    class="mt-1 text-[11px] text-dp-tertiary"
                  >
                    {{ t("toolbox.tools.imgcompress.keptOriginal") }}
                  </div>
                </div>
                <div class="flex gap-2">
                  <el-button size="small" class="flex-1" :icon="CopyDocument" @click="compCopySingle">
                    {{ t("toolbox.tools.imgcompress.copyImage") }}
                  </el-button>
                  <el-button size="small" class="flex-1" type="primary" plain :icon="Download" @click="compSaveSingle">
                    {{ t("toolbox.tools.imgcompress.saveImage") }}
                  </el-button>
                </div>
              </template>

              <template v-else-if="compRows.length">
                <div
                  v-if="compSummary"
                  class="px-3 py-2 bg-dp-card-bg border border-dp-card-border rounded-[10px] flex items-center gap-2 text-xs"
                >
                  <span class="text-dp-muted flex-1 min-w-0 truncate">
                    {{ t("images.compBatchSummary", {
                      n: compSummary.n,
                      s: kb(compSummary.src),
                      d: kb(compSummary.out),
                      p: compSummary.savedPct,
                    }) }}
                  </span>
                  <el-button size="small" text type="primary" @click="compOpenOut">
                    {{ t("images.compOpenOut") }}
                  </el-button>
                </div>
                <div
                  v-for="row in compRows"
                  :key="row.key"
                  class="px-3 py-2 bg-dp-card-bg border border-dp-card-border rounded-[10px] flex items-center gap-2 text-xs min-w-0"
                >
                  <el-icon v-if="row.saved && !row.kept" class="shrink-0 text-[var(--el-color-success,#67c23a)]" :size="13">
                    <Check />
                  </el-icon>
                  <el-icon v-else-if="row.failed" class="shrink-0 text-[var(--el-color-danger,#f56c6c)]" :size="13">
                    <Close />
                  </el-icon>
                  <span v-else class="w-[13px] shrink-0" />
                  <span class="text-dp-primary truncate flex-1 min-w-0" :title="row.name">{{ row.name }}</span>
                  <template v-if="row.failed">
                    <span class="text-[var(--el-color-danger,#f56c6c)] truncate max-w-[40%]" :title="row.error">
                      {{ t("toolbox.tools.imgcompress.batchFail") }}
                    </span>
                  </template>
                  <template v-else-if="row.kept">
                    <span class="text-dp-tertiary shrink-0">{{ kb(row.srcBytes) }}</span>
                    <span class="text-dp-tertiary text-[10px] shrink-0">
                      {{ t("toolbox.tools.imgcompress.batchKept") }}
                    </span>
                  </template>
                  <template v-else>
                    <span class="text-dp-tertiary shrink-0">{{ kb(row.srcBytes) }}</span>
                    <span class="text-dp-tertiary shrink-0">→</span>
                    <span class="text-dp-accent font-medium shrink-0">{{ kb(row.outBytes) }}</span>
                    <span
                      v-if="row.percent > 0"
                      class="text-[var(--el-color-success,#67c23a)] shrink-0"
                    >
                      -{{ row.percent }}%
                    </span>
                  </template>
                  <el-button
                    v-if="row.kind === 'clipboard' && row.dataUrl"
                    size="small"
                    text
                    type="primary"
                    @click="compSaveRow(row)"
                  >
                    {{ t("toolbox.tools.imgcompress.saveImage") }}
                  </el-button>
                </div>
              </template>

              <el-empty
                v-else-if="!compRunning"
                :description="t('images.compEmpty')"
                :image-size="72"
              />
            </div>
          </div>
        </template>

        <!-- 重复检测 -->
        <template v-else>
          <div class="p-4">
            <div class="flex items-center gap-3 mb-3">
              <el-button
                type="primary"
                size="small"
                :loading="dedupRunning"
                :icon="Refresh"
                @click="startDedup"
              >
                {{ dedupRunning
                  ? t("images.dedupRunning", { done: dedupProgress.done, total: dedupProgress.total })
                  : t("images.dedupStart") }}
              </el-button>
              <el-button
                v-if="dupGroups.length"
                type="danger"
                size="small"
                :icon="Warning"
                @click="deleteSelectedDups"
              >
                {{ t("images.deleteSelected") }}
              </el-button>
              <span class="text-[11px] text-dp-tertiary">{{ t("images.dedupHint") }}</span>
            </div>

            <template v-if="dupGroups.length">
              <div
                v-for="g in dupGroups"
                :key="g.hash"
                class="mb-4 p-2.5 bg-dp-card-bg border border-dp-card-border rounded-[10px]"
              >
                <div class="text-[11px] text-dp-muted mb-2 flex items-center gap-2">
                  <span>{{ t("images.dupGroup", { n: g.items.length }) }}</span>
                  <span class="text-dp-tertiary text-[10px] font-mono truncate">{{ g.hash.slice(0, 16) }}…</span>
                </div>
                <div class="flex gap-2 flex-wrap">
                  <button
                    v-for="it in g.items"
                    :key="it.id"
                    class="relative w-[104px] rounded-lg overflow-hidden border-2 bg-dp-btn-bg"
                    :class="dupSelection[g.hash]?.includes(it.id) ? 'border-[var(--el-color-danger,#f56c6c)]' : 'border-transparent'"
                    :title="dupSelection[g.hash]?.includes(it.id) ? t('images.markDelete') : t('images.keep')"
                    @click="toggleDupPick(g.hash, it.id)"
                  >
                    <img
                      :src="imgSrc('thumb', it.id, imgV(it))"
                      loading="lazy"
                      class="w-full aspect-square object-cover"
                      alt=""
                    />
                    <div class="px-1 py-0.5 text-[9.5px] text-dp-tertiary truncate">{{ it.name }}</div>
                    <el-icon
                      v-if="dupSelection[g.hash]?.includes(it.id)"
                      class="absolute top-1 right-1 text-[var(--el-color-danger,#f56c6c)]"
                      :size="14"
                    >
                      <Warning />
                    </el-icon>
                  </button>
                </div>
              </div>
            </template>
            <el-empty
              v-else-if="dupLoaded && !dedupRunning"
              :description="t('images.dedupEmpty')"
              :image-size="72"
            />
          </div>
        </template>
      </main>
    </div>

    <!-- 查看器 -->
    <div
      v-if="viewer.open && viewer.item"
      class="fixed inset-0 z-40 bg-black/85 flex"
      @wheel="onWheel"
    >
      <div
        class="flex-1 min-w-0 relative flex items-center justify-center overflow-hidden"
        @click.self="closeViewer"
      >
        <!-- 关闭 / 左右切换 -->
        <button
          class="absolute top-3 right-3 z-10 w-9 h-9 rounded-full bg-black/50 text-white/90 hover:bg-black/70 flex items-center justify-center transition-colors"
          :title="t('images.viewerClose')"
          @click="closeViewer"
        >
          <el-icon :size="18"><Close /></el-icon>
        </button>
        <button
          v-if="viewerNav.hasPrev"
          class="absolute left-3 top-1/2 -translate-y-1/2 z-10 w-10 h-10 rounded-full bg-black/50 text-white/90 hover:bg-black/70 flex items-center justify-center transition-colors"
          :title="t('images.viewerPrev')"
          @click="viewerStep(-1)"
        >
          <el-icon :size="20"><ArrowLeft /></el-icon>
        </button>
        <button
          v-if="viewerNav.hasNext"
          class="absolute right-3 top-1/2 -translate-y-1/2 z-10 w-10 h-10 rounded-full bg-black/50 text-white/90 hover:bg-black/70 flex items-center justify-center transition-colors"
          :title="t('images.viewerNext')"
          @click="viewerStep(1)"
        >
          <el-icon :size="20"><ArrowRight /></el-icon>
        </button>
        <img
          :src="imgSrc('full', viewer.item.id, viewer.item.mtime)"
          class="max-w-full max-h-full object-contain transition-transform"
          :style="viewerStyle"
          draggable="false"
          @pointerdown.prevent="onPanStart"
          @pointermove="onPanMove"
          @pointerup="onPanEnd"
          @pointercancel="onPanEnd"
          alt=""
        />
        <!-- 底部操作栏 -->
        <div class="absolute bottom-4 left-1/2 -translate-x-1/2 flex items-center gap-1 px-2 py-1.5 rounded-xl bg-black/55 backdrop-blur text-white">
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[14px] leading-none text-white/90 hover:bg-white/15" :title="t('images.fav')" @click="toggleFav">
            <el-icon :size="15" :class="viewer.item.is_fav ? 'text-[#f7ba2a]' : ''">
              <StarFilled v-if="viewer.item.is_fav" />
              <Star v-else />
            </el-icon>
          </button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[14px] leading-none text-white/90 hover:bg-white/15" :title="t('images.tagEdit')" @click="viewer.infoOpen = !viewer.infoOpen">
            <el-icon :size="15"><InfoFilled /></el-icon>
          </button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-white/90 hover:bg-white/15" :title="t('images.copyImage')" @click="copyImage">
            <el-icon :size="15"><CopyDocument /></el-icon>
          </button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[14px] leading-none text-white/90 hover:bg-white/15" :title="t('images.compress')" @click="compressItem">⤓</button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-white/90 hover:bg-white/15" :title="t('images.locate')" @click="imagesRevealPath(viewer.item.path)">
            <el-icon :size="15"><FolderOpened /></el-icon>
          </button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-white/90 hover:bg-white/15" :title="t('images.open')" @click="imagesOpenPath(viewer.item.path)">
            <el-icon :size="15"><TopRight /></el-icon>
          </button>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[var(--el-color-danger,#f56c6c)] hover:bg-white/15" :title="t('images.delete')" @click="removeCurrent">
            <el-icon :size="15"><Close /></el-icon>
          </button>
          <span class="mx-1 w-px h-4 bg-white/25" />
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[14px] leading-none text-white/90 hover:bg-white/15" @click="viewer.scale = Math.min(8, viewer.scale * 1.25)">+</button>
          <span class="text-[11px] w-10 text-center tabular-nums">{{ Math.round(viewer.scale * 100) }}%</span>
          <button class="w-7 h-7 inline-flex items-center justify-center rounded-lg text-[14px] leading-none text-white/90 hover:bg-white/15" @click="viewer.scale = Math.max(1, viewer.scale / 1.25); viewer.tx = 0; viewer.ty = 0">−</button>
        </div>
      </div>

      <!-- 信息 / 标签面板 -->
      <aside
        v-if="viewer.infoOpen"
        class="w-64 shrink-0 bg-dp-card-bg border-l border-dp-divider p-3 overflow-y-auto"
      >
        <div class="flex items-center gap-1.5 mb-2">
          <span class="text-xs font-medium text-dp-primary flex-1">{{ t("images.info") }}</span>
          <el-button size="small" :icon="Close" circle text @click="viewer.infoOpen = false" />
        </div>
        <div class="space-y-1.5 text-[11px] leading-[1.7]">
          <div class="text-dp-primary font-medium break-all">{{ viewer.item.name }}</div>
          <div class="flex justify-between gap-2">
            <span class="text-dp-tertiary">{{ t("images.dimensions") }}</span>
            <span class="text-dp-muted">{{ viewer.item.width }}×{{ viewer.item.height }}</span>
          </div>
          <div class="flex justify-between gap-2">
            <span class="text-dp-tertiary">{{ t("images.docSize") }}</span>
            <span class="text-dp-muted">{{ fmtBytes(viewer.item.size) }}</span>
          </div>
          <div class="flex justify-between gap-2">
            <span class="text-dp-tertiary">{{ t("images.takenAt") }}</span>
            <span class="text-dp-muted">{{ fmtDate(viewer.item.taken_at) }}</span>
          </div>
          <div v-if="viewer.item.camera" class="flex justify-between gap-2">
            <span class="text-dp-tertiary">{{ t("images.camera") }}</span>
            <span class="text-dp-muted truncate">{{ viewer.item.camera }}</span>
          </div>
          <div class="flex justify-between gap-2">
            <span class="text-dp-tertiary shrink-0">{{ t("images.dirOf") }}</span>
            <span
              class="text-dp-muted truncate cursor-pointer hover:text-dp-accent"
              :title="`${t('images.locate')} · ${parentDirOf(viewer.item.path)}`"
              @click="imagesRevealPath(parentDirOf(viewer.item.path))"
            >
              {{ parentDirOf(viewer.item.path) }}
            </span>
          </div>
          <div
            class="text-dp-tertiary break-all cursor-pointer hover:text-dp-muted"
            :title="t('images.locate')"
            @click="imagesRevealPath(viewer.item.path)"
          >
            {{ viewer.item.path }}
          </div>
          <div class="pt-1">
            <el-button size="small" class="w-full" :icon="Search" @click="goToItemDir">
              {{ t("images.viewDirOnly") }}
            </el-button>
          </div>
        </div>
        <div class="mt-3 pt-2 border-t border-dp-divider">
          <div class="text-[11px] font-medium text-dp-muted mb-1.5">{{ t("images.tagEdit") }}</div>
          <div class="flex flex-wrap gap-1">
            <button
              v-for="tg in tags"
              :key="tg.id"
              class="px-1.5 py-0.5 rounded-full text-[10.5px] border transition-colors"
              :class="viewer.tags.some((x) => x.id === tg.id)
                ? 'bg-[var(--el-color-primary-light-9,#ecf5ff)] border-dp-accent text-dp-accent'
                : 'border-dp-divider text-dp-tertiary hover:text-dp-muted'"
              @click="toggleItemTag(tg)"
            >
              <span
                class="inline-block w-2 h-2 rounded-full mr-1 align-middle"
                :style="{ background: tg.color || '#409eff' }"
              />
              <el-icon v-if="viewer.tags.some((x) => x.id === tg.id)" :size="9" class="inline align-middle">
                <Check />
              </el-icon>
              {{ tg.name }}
            </button>
            <span v-if="!tags.length" class="text-[10.5px] text-dp-tertiary">{{ t("images.noTags") }}</span>
          </div>
        </div>
      </aside>
    </div>

    <!-- 编辑标签:改名/换色 -->
    <el-dialog v-model="tagEdit.visible" :title="t('images.tagEditTitle')" width="320px">
      <div class="flex items-center gap-2">
        <span class="text-xs text-dp-muted shrink-0">{{ t("toolbox.tools.imgcompress.wmContent") }}</span>
        <el-input v-model="tagEdit.name" size="small" class="flex-1 min-w-0" @keyup.enter="saveTagEdit" />
        <el-color-picker v-model="tagEdit.color" size="small" />
      </div>
      <template #footer>
        <el-button size="small" @click="tagEdit.visible = false">
          {{ t("images.compExitSelect") }}
        </el-button>
        <el-button size="small" type="primary" @click="saveTagEdit">
          {{ t("toolbox.tools.imgcompress.compressBtn") }}
        </el-button>
      </template>
    </el-dialog>

    <!-- 批量打标签 -->
    <el-dialog
      v-model="tagDialog.visible"
      :title="t('images.tagForSelected')"
      width="360px"
      :close-on-click-modal="true"
    >
      <div class="text-[11px] text-dp-tertiary mb-2.5">
        {{ t("images.tagDialogHint", { n: selectedIds.size }) }}
      </div>
      <div class="flex flex-wrap gap-1.5 mb-3">
        <button
          v-for="tg in tags"
          :key="tg.id"
          class="px-2 py-1 rounded-full text-[11px] border border-dp-card-border hover:border-dp-accent hover:text-dp-accent transition-colors"
          @click="applyTagToSelected(tg)"
        >
          <span class="inline-block w-2 h-2 rounded-full mr-1 align-middle" :style="{ background: tg.color || '#409eff' }" />
          {{ tg.name }}
        </button>
        <span v-if="!tags.length" class="text-[11px] text-dp-tertiary">{{ t("images.noTags") }}</span>
      </div>
      <div class="flex items-center gap-2">
        <el-input
          v-model="tagDialogName"
          size="small"
          :placeholder="t('images.addTag')"
          class="flex-1 min-w-0"
          @keyup.enter="createAndApplyTag"
        />
        <el-button size="small" type="primary" @click="createAndApplyTag">
          {{ t("images.tagCreateApply") }}
        </el-button>
      </div>
    </el-dialog>

    <!-- 无边框窗口的边缘/四角缩放手柄(复用面板同款) -->
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
