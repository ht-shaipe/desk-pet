import { invoke } from "@tauri-apps/api/core";
import type {
  ImageDetail,
  ImageDirInfo,
  ImageDupGroup,
  ImagePage,
  ImageQueryDto,
  ImageTagInfo,
} from "@/types";

export const imagesOpen = (view?: string) =>
  invoke<boolean>("images_open", { view: view ?? null });
export const imagesAddDir = (path: string, fullDisk: boolean) =>
  invoke<ImageDirInfo>("images_add_dir", { path, fullDisk });
export const imagesListDirs = () => invoke<ImageDirInfo[]>("images_list_dirs");
export const imagesRemoveDir = (id: number) => invoke<void>("images_remove_dir", { id });
export const imagesScanStart = (dirId: number) => invoke<void>("images_scan_start", { dirId });
export const imagesTaskCancel = (taskKey: number) =>
  invoke<void>("images_task_cancel", { taskKey });
export const imagesQuery = (query: ImageQueryDto) => invoke<ImagePage>("images_query", { query });
export const imagesDetail = (id: number) => invoke<ImageDetail | null>("images_detail", { id });
export const imagesSetFav = (id: number, fav: boolean) =>
  invoke<void>("images_set_fav", { id, fav });
export const imagesDeleteItem = (id: number) => invoke<void>("images_delete_item", { id });
export const imagesOpenPath = (path: string) => invoke<void>("images_open_path", { path });
export const imagesRevealPath = (path: string) => invoke<void>("reveal_path", { path });

export const imagesListTags = () => invoke<ImageTagInfo[]>("images_list_tags");
export const imagesItemTagPairs = () =>
  invoke<{ itemId: number; tagId: number }[]>("images_item_tag_pairs");
export const imagesAddTag = (name: string, color: string) =>
  invoke<ImageTagInfo>("images_add_tag", { name, color });
export const imagesRenameTag = (id: number, name: string, color: string) =>
  invoke<void>("images_rename_tag", { id, name, color });
export const imagesDeleteTag = (id: number) => invoke<void>("images_delete_tag", { id });
export const imagesSetItemTag = (itemId: number, tagId: number, on: boolean) =>
  invoke<void>("images_set_item_tag", { itemId, tagId, on });

export const imagesDedupStart = () => invoke<void>("images_dedup_start");
export const imagesDupGroups = () => invoke<ImageDupGroup[]>("images_dup_groups");
export const imagesDeletePaths = (paths: string[], ids: number[]) =>
  invoke<void>("images_delete_paths", { paths, ids });

const IS_WIN = navigator.userAgent.includes("Windows");

/** img:// 自定义协议地址(Windows 经 http://img.localhost 代理,其余平台直接 scheme)。
 *  v 参数取文件 mtime,内容变化即破缓存。 */
export function imgSrc(kind: "thumb" | "full", id: number, v: number | string): string {
  const base = IS_WIN ? `http://img.localhost/${kind}/${id}` : `img://localhost/${kind}/${id}`;
  return `${base}?v=${v}`;
}
