import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, ClipEntry, LauncherConfig, LauncherItem, Note } from "@/types";

export const getSettings = () => invoke<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) => invoke<void>("save_settings", { settings });
export const getLauncher = () => invoke<LauncherConfig>("get_launcher");
export const saveLauncher = (config: LauncherConfig) => invoke<void>("save_launcher", { config });
export const getNotes = () => invoke<Note[]>("get_notes");
export const saveNotes = (notes: Note[]) => invoke<void>("save_notes", { notes });
export const launchItem = (item: LauncherItem) => invoke<void>("launch_item", { item });
export const getClipboardHistory = () => invoke<ClipEntry[]>("get_clipboard_history");
export const clearClipboardHistory = () => invoke<void>("clear_clipboard_history");
export const deleteClipboardEntry = (id: string) => invoke<void>("delete_clipboard_entry", { id });
export const copyToClipboard = (text: string) => invoke<void>("copy_to_clipboard", { text });
export const quickAction = (action: string) => invoke<void>("quick_action", { action });
export const getPlatform = () => invoke<string>("get_platform");
