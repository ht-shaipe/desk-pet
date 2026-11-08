import { invoke } from "@tauri-apps/api/core";

export const quickAction = (action: string) => invoke<void>("quick_action", { action });
export const getPlatform = () => invoke<string>("get_platform");
/** 设置是否在 Dock 中显示（仅 macOS）。 */
export const setShowDock = (visible: boolean) =>
  invoke<void>("set_show_dock", { visible });
/** 写一行调试日志到 app data 的 debug.log（返回日志文件路径）。 */
export const debugLog = (scope: string, message: string) =>
  invoke<string>("debug_log", { scope, message });
