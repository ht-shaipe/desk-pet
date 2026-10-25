import { defineStore } from "pinia";
import { ref } from "vue";
import {
  clearFavorites,
  deleteFavorites,
  getFavorites,
  upsertFavorites,
} from "@/api/favorite";
import { isoNow, normalizeImported } from "@/lib/stash";
import type { Favorite } from "@/types";

export const useFavoritesStore = defineStore("favorites", () => {
  const entries = ref<Favorite[]>([]);
  const loaded = ref(false);

  async function refresh() {
    entries.value = await getFavorites();
  }

  async function init() {
    if (!loaded.value) {
      await refresh();
      loaded.value = true;
    }
  }

  /** 新增/编辑收藏（更新时间自动刷新）。 */
  async function upsert(item: Favorite) {
    await upsertFavorites([{ ...item, update_time: isoNow() }]);
    await refresh();
  }

  /** 批量合并（导入 / 云端拉取），返回入库条数。 */
  async function merge(items: Favorite[]): Promise<number> {
    if (items.length === 0) {
      return 0;
    }
    const count = await upsertFavorites(items);
    await refresh();
    return count;
  }

  /** 导入 stash 扩展导出的 JSON 文本，返回入库条数。 */
  async function importJson(text: string): Promise<number> {
    return merge(normalizeImported(JSON.parse(text)));
  }

  /** 剪贴板一键收藏：同址已存在时不覆盖（避免冲掉已刷新的仓库数据），返回是否新增。 */
  async function addFromUrl(item: Favorite): Promise<boolean> {
    if (entries.value.some((e) => e.url === item.url)) {
      return false;
    }
    await upsert(item);
    return true;
  }

  async function removeMany(urls: string[]): Promise<number> {
    if (urls.length === 0) {
      return 0;
    }
    const removed = await deleteFavorites(urls);
    await refresh();
    return removed;
  }

  async function remove(url: string) {
    await removeMany([url]);
  }

  async function clearAll() {
    await clearFavorites();
    await refresh();
  }

  return {
    entries,
    init,
    refresh,
    upsert,
    merge,
    importJson,
    addFromUrl,
    remove,
    removeMany,
    clearAll,
  };
});
