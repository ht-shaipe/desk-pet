import { invoke } from "@tauri-apps/api/core";
import type { ClipEntry } from "@/types";

export const getClipboardHistory = () => invoke<ClipEntry[]>("get_clipboard_history");
export const clearClipboardHistory = () => invoke<void>("clear_clipboard_history");
export const deleteClipboardEntry = (id: string) => invoke<void>("delete_clipboard_entry", { id });
export const deleteClipboardEntries = (ids: string[]) =>
  invoke<void>("delete_clipboard_entries", { ids });
export const copyToClipboard = (text: string) => invoke<void>("copy_to_clipboard", { text });
export const copyImageToClipboard = (filename: string) =>
  invoke<void>("copy_image_to_clipboard", { filename });
export const getClipImageDir = () => invoke<string>("get_clip_image_dir");
