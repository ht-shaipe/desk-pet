import { invoke } from "@tauri-apps/api/core";
import type { DocDirInfo, DocPage, DocQueryDto } from "@/types";

export const docsOpen = () => invoke<boolean>("docs_open");
export const docsAddDir = (path: string) => invoke<DocDirInfo>("docs_add_dir", { path });
export const docsListDirs = () => invoke<DocDirInfo[]>("docs_list_dirs");
export const docsRemoveDir = (id: number) => invoke<number>("docs_remove_dir", { id });
export const docsScanStart = (dirId: number) => invoke<void>("docs_scan_start", { dirId });
export const docsTaskCancel = (dirId: number) => invoke<void>("docs_task_cancel", { dirId });
export const docsQuery = (query: DocQueryDto) => invoke<DocPage>("docs_query", { query });
export const docsDeleteItem = (id: number) => invoke<void>("docs_delete_item", { id });
