import { invoke } from "@tauri-apps/api/core";
import type { LauncherConfig, LauncherItem } from "@/types";

export const getLauncher = () => invoke<LauncherConfig>("get_launcher");
export const saveLauncher = (config: LauncherConfig) => invoke<void>("save_launcher", { config });
export const launchItem = (item: LauncherItem) => invoke<void>("launch_item", { item });
