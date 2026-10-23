import { invoke } from "@tauri-apps/api/core";
import type { ImageFileInfo, ImagePayload } from "@/types";

export const getClipboardImage = () => invoke<ImagePayload | null>("get_clipboard_image");
export const setClipboardImage = (payload: ImagePayload) =>
  invoke<void>("set_clipboard_image", { payload });
export const saveQrPng = (dataUrl: string) => invoke<string>("save_qr_png", { dataUrl });
export const saveImage = (dataUrl: string) => invoke<string>("save_image", { dataUrl });

/** 目录批量压缩 */
export const listImageFiles = (dir: string) =>
  invoke<ImageFileInfo[]>("list_image_files", { dir });
export const readImageDataUrl = (path: string) =>
  invoke<string>("read_image_data_url", { path });
export const saveCompressedImage = (dir: string, srcName: string, dataUrl: string) =>
  invoke<string>("save_compressed_image", { dir, srcName, dataUrl });
export const revealPath = (path: string) => invoke<void>("reveal_path", { path });
