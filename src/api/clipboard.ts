import { invoke } from "@tauri-apps/api/core";
import type { ClipEntry } from "@/types";

export const getClipboardHistory = () => invoke<ClipEntry[]>("get_clipboard_history");
export const clearClipboardHistory = () => invoke<void>("clear_clipboard_history");
export const deleteClipboardEntry = (id: string) => invoke<void>("delete_clipboard_entry", { id });
export const copyToClipboard = (text: string) => invoke<void>("copy_to_clipboard", { text });
