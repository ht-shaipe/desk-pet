<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { Delete, EditPen, Folder, Star, StarFilled } from "@element-plus/icons-vue";
import type { HttpHistoryItem, HttpSavedItem } from "@/types";

const { t } = useI18n();

const props = defineProps<{
  saved: HttpSavedItem[];
  history: HttpHistoryItem[];
  /** 当前已载入的条目 id（高亮） */
  activeSavedId: string | null;
  activeHistoryId: string | null;
}>();

const emit = defineEmits<{
  "load-saved": [item: HttpSavedItem];
  "edit-saved": [item: HttpSavedItem];
  "delete-saved": [item: HttpSavedItem];
  "rename-group": [group: string];
  "delete-group": [group: string];
  "toggle-pin": [item: HttpSavedItem];
  "load-history": [item: HttpHistoryItem];
  "clear-history": [];
}>();

const tab = ref<"saved" | "history">("saved");
const keyword = ref("");

const matchKeyword = (name: string, url: string) => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) {
    return true;
  }
  return name.toLowerCase().includes(kw) || url.toLowerCase().includes(kw);
};

/** 置顶收藏（搜索同样生效） */
const pinnedItems = computed(() =>
  props.saved.filter((item) => item.pinned && matchKeyword(item.name, savedUrl(item))),
);

/** 非置顶收藏按集合分组（空组名归入默认分组） */
const groups = computed(() => {
  const map = new Map<string, HttpSavedItem[]>();
  for (const item of props.saved) {
    if (item.pinned || !matchKeyword(item.name, savedUrl(item))) {
      continue;
    }
    const key = item.group_name ?? "";
    const list = map.get(key) ?? [];
    list.push(item);
    map.set(key, list);
  }
  return [...map.entries()];
});

const savedUrl = (item: HttpSavedItem): string => {
  try {
    const spec = JSON.parse(item.spec || "{}") as { url?: string };
    return spec.url ?? "";
  } catch {
    return "";
  }
};

/** 收藏条目的请求方法（spec JSON 解析失败时按 GET 显示） */
const methodOf = (item: HttpSavedItem): string => {
  try {
    const spec = JSON.parse(item.spec || "{}") as { method?: string };
    return (spec.method ?? "GET").toUpperCase();
  } catch {
    return "GET";
  }
};

const historyItems = computed(() =>
  props.history.filter((item) => matchKeyword(item.url, item.url)),
);

function groupName(group: string): string {
  return group || t("toolbox.tools.http.groupDefault");
}

/** 同日显示 HH:mm，否则 MM-DD HH:mm */
function fmtTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  const hh = `${date.getHours()}`.padStart(2, "0");
  const mm = `${date.getMinutes()}`.padStart(2, "0");
  if (sameDay) {
    return `${hh}:${mm}`;
  }
  const mo = `${date.getMonth() + 1}`.padStart(2, "0");
  const dd = `${date.getDate()}`.padStart(2, "0");
  return `${mo}-${dd} ${hh}:${mm}`;
}

function statusClass(status: number): string {
  if (status >= 200 && status < 400) {
    return "text-[var(--el-color-success,#67c23a)]";
  }
  if (status === 0) {
    return "text-[var(--el-color-danger,#f56c6c)]";
  }
  return "text-[var(--el-color-warning,#e6a23c)]";
}

function methodClass(method: string): string {
  switch (method) {
    case "GET":
      return "text-[var(--el-color-success,#67c23a)]";
    case "POST":
      return "text-[var(--el-color-primary,#409eff)]";
    case "PUT":
      return "text-[var(--el-color-warning,#e6a23c)]";
    case "DELETE":
      return "text-[var(--el-color-danger,#f56c6c)]";
    default:
      return "text-dp-muted";
  }
}

const itemClass = (active: boolean): string =>
  active
    ? "bg-[color-mix(in_srgb,var(--el-color-primary,#409eff)_12%,transparent)]"
    : "hover:bg-[color-mix(in_srgb,var(--el-color-primary,#409eff)_7%,transparent)]";
</script>

<template>
  <aside class="w-[192px] shrink-0 flex flex-col min-h-0 bg-dp-card-bg border border-dp-card-border rounded-[10px]">
    <!-- tab + 搜索 -->
    <div class="shrink-0 px-2.5 pt-2.5 pb-1.5 border-b border-dp-divider">
      <el-radio-group v-model="tab" size="small" class="w-full [&_.el-radio-button\_\_inner]:w-full">
        <el-radio-button value="saved" class="flex-1">
          {{ t("toolbox.tools.http.collections") }}({{ saved.length }})
        </el-radio-button>
        <el-radio-button value="history" class="flex-1">
          {{ t("toolbox.tools.http.history") }}({{ history.length }})
        </el-radio-button>
      </el-radio-group>
      <el-input
        v-model="keyword"
        size="small"
        clearable
        :placeholder="t('toolbox.tools.http.searchPh')"
        class="mt-2 [&_.el-input\_\_inner]:text-[11.5px]"
      />
    </div>

    <!-- 收藏列表 -->
    <el-scrollbar v-if="tab === 'saved'" class="flex-1 min-h-0 dp-scroll">
      <div class="flex flex-col py-1">
        <!-- 置顶 -->
        <div v-if="pinnedItems.length" class="mb-1">
          <div class="flex items-center gap-1 px-2.5 h-[26px] text-[11px] text-dp-muted select-none">
            <el-icon :size="12" class="text-[var(--el-color-warning,#e6a23c)]"><StarFilled /></el-icon>
            <span class="flex-1 min-w-0 truncate font-medium">{{ t("toolbox.tools.http.pinned") }}</span>
            <span class="text-dp-tertiary">{{ pinnedItems.length }}</span>
          </div>
          <div
            v-for="item in pinnedItems"
            :key="item.id"
            class="group flex items-center gap-1.5 pl-3.5 pr-1.5 h-[28px] cursor-pointer"
            :class="itemClass(item.id === activeSavedId)"
            @click="emit('load-saved', item)"
          >
            <span class="text-[9.5px] font-semibold shrink-0 w-[40px]" :class="methodClass(methodOf(item))">
              {{ methodOf(item) }}
            </span>
            <span class="flex-1 min-w-0 truncate text-[11.5px] text-dp-secondary">
              {{ item.name || savedUrl(item) }}
            </span>
            <el-button
              size="small"
              text
              :icon="StarFilled"
              class="!p-0 !h-[18px] !w-[18px] shrink-0"
              :title="t('toolbox.tools.http.unpinReq')"
              @click.stop="emit('toggle-pin', item)"
            />
            <el-button
              size="small"
              text
              :icon="Delete"
              class="!p-0 !h-[18px] !w-[18px] shrink-0 hidden group-hover:block"
              :title="t('common.delete')"
              @click.stop="emit('delete-saved', item)"
            />
          </div>
        </div>

        <div v-for="[group, items] in groups" :key="group" class="mb-1">
          <div
            class="group flex items-center gap-1 px-2.5 h-[26px] text-[11px] text-dp-muted select-none"
          >
            <span class="flex-1 min-w-0 truncate font-medium">{{ groupName(group) }}</span>
            <span class="text-dp-tertiary">{{ items.length }}</span>
            <el-button
              size="small"
              text
              :icon="Folder"
              class="!p-0 !h-[18px] !w-[18px] hidden group-hover:block"
              :title="t('toolbox.tools.http.renameGroup')"
              @click="emit('rename-group', group)"
            />
            <el-button
              size="small"
              text
              :icon="Delete"
              class="!p-0 !h-[18px] !w-[18px] hidden group-hover:block"
              :title="t('toolbox.tools.http.deleteGroup')"
              @click="emit('delete-group', group)"
            />
          </div>
          <div
            v-for="item in items"
            :key="item.id"
            class="group flex items-center gap-1.5 pl-3.5 pr-1.5 h-[28px] cursor-pointer"
            :class="itemClass(item.id === activeSavedId)"
            @click="emit('load-saved', item)"
          >
            <span class="text-[9.5px] font-semibold shrink-0 w-[40px]" :class="methodClass(methodOf(item))">
              {{ methodOf(item) }}
            </span>
            <span class="flex-1 min-w-0 truncate text-[11.5px] text-dp-secondary">
              {{ item.name || savedUrl(item) }}
            </span>
            <el-button
              size="small"
              text
              :icon="Star"
              class="!p-0 !h-[18px] !w-[18px] shrink-0 hidden group-hover:block"
              :title="t('toolbox.tools.http.pinReq')"
              @click.stop="emit('toggle-pin', item)"
            />
            <el-button
              size="small"
              text
              :icon="EditPen"
              class="!p-0 !h-[18px] !w-[18px] shrink-0 hidden group-hover:block"
              :title="t('toolbox.tools.http.editSaved')"
              @click.stop="emit('edit-saved', item)"
            />
            <el-button
              size="small"
              text
              :icon="Delete"
              class="!p-0 !h-[18px] !w-[18px] shrink-0 hidden group-hover:block"
              :title="t('common.delete')"
              @click.stop="emit('delete-saved', item)"
            />
          </div>
        </div>
        <div v-if="!groups.length" class="px-3 py-6 text-center text-[11px] leading-[1.8] text-dp-tertiary">
          {{ t("toolbox.tools.http.emptySaved") }}
        </div>
      </div>
    </el-scrollbar>

    <!-- 历史列表 -->
    <template v-else>
      <el-scrollbar class="flex-1 min-h-0 dp-scroll">
        <div class="flex flex-col py-1">
          <div
            v-for="item in historyItems"
            :key="item.id"
            class="group flex items-center gap-1.5 pl-2.5 pr-1.5 h-[30px] cursor-pointer"
            :class="itemClass(item.id === activeHistoryId)"
            @click="emit('load-history', item)"
          >
            <span class="text-[9.5px] font-semibold shrink-0 w-[40px]" :class="methodClass(item.method)">
              {{ item.method }}
            </span>
            <span class="flex-1 min-w-0 truncate text-[11px] text-dp-secondary">
              {{ item.url }}
            </span>
            <span class="shrink-0 text-[10px] font-mono" :class="statusClass(item.status)">
              {{ item.status || "ERR" }}
            </span>
            <span class="shrink-0 text-[10px] text-dp-tertiary w-[38px] text-right">
              {{ fmtTime(item.create_time) }}
            </span>
          </div>
          <div v-if="!historyItems.length" class="px-3 py-6 text-center text-[11px] leading-[1.8] text-dp-tertiary">
            {{ t("toolbox.tools.http.emptyHistory") }}
          </div>
        </div>
      </el-scrollbar>
      <div v-if="historyItems.length" class="shrink-0 px-2.5 py-1.5 border-t border-dp-divider">
        <el-button size="small" text :icon="Delete" class="w-full" @click="emit('clear-history')">
          {{ t("toolbox.tools.http.clearHistory") }}
        </el-button>
      </div>
    </template>
  </aside>
</template>
