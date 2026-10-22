import { invoke } from "@tauri-apps/api/core";

export const quickAction = (action: string) => invoke<void>("quick_action", { action });
export const getPlatform = () => invoke<string>("get_platform");
