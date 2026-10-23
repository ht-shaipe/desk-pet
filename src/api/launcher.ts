import { invoke } from "@tauri-apps/api/core";
import type { AppEntry, LauncherConfig, LauncherItem } from "@/types";

export const getLauncher = () => invoke<LauncherConfig>("get_launcher");
export const saveLauncher = (config: LauncherConfig) => invoke<void>("save_launcher", { config });
export const launchItem = (item: LauncherItem) => invoke<void>("launch_item", { item });
/** 提取系统应用图标，返回 PNG 缓存文件路径（无法提取时为 null）。 */
export const getAppIcon = (item: LauncherItem) =>
  invoke<string | null>("get_app_icon", { item });
/** 按启动值提取应用图标（应用选择下拉列表用），返回 PNG 缓存文件路径。 */
export const getAppIconPath = (value: string) =>
  invoke<string | null>("get_app_icon_by_value", { value });
/** 枚举系统已安装应用。 */
export const listApps = () => invoke<AppEntry[]>("list_apps");
