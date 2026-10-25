import { invoke } from "@tauri-apps/api/core";
import type { Favorite } from "@/types";

export const getFavorites = () => invoke<Favorite[]>("get_favorites");

/** 批量 upsert（导入 / 云拉取 / 手动添加共用），返回实际入库条数。 */
export const upsertFavorites = (items: Favorite[]) =>
  invoke<number>("upsert_favorites", { items });

export const deleteFavorites = (urls: string[]) =>
  invoke<number>("delete_favorites", { urls });

export const clearFavorites = () => invoke<void>("clear_favorites");

/** 读取收藏导入 JSON（用户通过对话框选择的路径）。 */
export const readFavoriteImportFile = (path: string) =>
  invoke<string>("read_favorite_import_file", { path });

/** 写出收藏导出 JSON。 */
export const writeFavoriteExportFile = (path: string, content: string) =>
  invoke<void>("write_favorite_export_file", { path, content });
