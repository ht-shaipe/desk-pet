import { invoke } from "@tauri-apps/api/core";

export const quickAction = (action: string) => invoke<void>("quick_action", { action });
export const getPlatform = () => invoke<string>("get_platform");
/** 设置是否在 Dock 中显示（仅 macOS）。 */
export const setShowDock = (visible: boolean) =>
  invoke<void>("set_show_dock", { visible });
