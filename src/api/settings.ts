import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "@/types";

export const getSettings = () => invoke<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) => invoke<void>("save_settings", { settings });
export const setPetAvatar = (src: string) => invoke<string>("set_pet_avatar", { src });
export const resetPetAvatar = () => invoke<void>("reset_pet_avatar");
