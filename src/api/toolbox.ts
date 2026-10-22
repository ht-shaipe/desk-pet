import { invoke } from "@tauri-apps/api/core";
import type { ImagePayload } from "@/types";

export const getClipboardImage = () => invoke<ImagePayload | null>("get_clipboard_image");
export const setClipboardImage = (payload: ImagePayload) =>
  invoke<void>("set_clipboard_image", { payload });
export const saveQrPng = (dataUrl: string) => invoke<string>("save_qr_png", { dataUrl });
