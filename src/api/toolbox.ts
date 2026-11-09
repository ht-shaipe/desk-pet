import { invoke } from "@tauri-apps/api/core";
import type {
  CompressImageParams,
  CompressImageResult,
  DocumentConvertResult,
  HttpRequestSpec,
  HttpResponseData,
  ImageFileInfo,
  ImagePayload,
  TextDiffResult,
} from "@/types";

export const getClipboardImage = () => invoke<ImagePayload | null>("get_clipboard_image");
export const setClipboardImage = (payload: ImagePayload) =>
  invoke<void>("set_clipboard_image", { payload });
export const saveQrPng = (dataUrl: string) => invoke<string>("save_qr_png", { dataUrl });
export const saveImage = (dataUrl: string) => invoke<string>("save_image", { dataUrl });

/** 目录批量压缩 */
export const listImageFiles = (dir: string) =>
  invoke<ImageFileInfo[]>("list_image_files", { dir });
export const revealPath = (path: string) => invoke<void>("reveal_path", { path });

/** 图片压缩（Rust 原生引擎） */
export const compressImageData = (dataUrl: string, params: CompressImageParams) =>
  invoke<CompressImageResult>("compress_image_data", { dataUrl, params });
export const compressImageFile = (
  path: string,
  params: CompressImageParams,
  outDir: string | null,
  includeData: boolean,
) => invoke<CompressImageResult>("compress_image_file", { path, params, outDir, includeData });

/** 接口请求：由 Rust 代理发送，规避跨域 */
export const httpRequest = (spec: HttpRequestSpec) =>
  invoke<HttpResponseData>("http_request", { spec });

/** 打开(或聚焦)接口请求独立窗口，返回是否为新创建 */
export const httpOpen = () => invoke<boolean>("http_open");

/** 文档转换为 Markdown（docx/pdf/xlsx/epub…）；扫描版 PDF 以 needs_ocr 状态返回 */
export const documentConvert = (path: string) =>
  invoke<DocumentConvertResult>("document_convert", { path });

/** 保存文本内容到本地文件 */
export const saveDocumentMarkdown = (path: string, content: string) =>
  invoke<void>("save_document_markdown", { path, content });

/** 文本对比：行级 + 词级 diff + tree-sitter 语法高亮；language 为 null/"plain" 时纯文本 */
export const textDiff = (left: string, right: string, language: string | null) =>
  invoke<TextDiffResult>("text_diff", { left, right, language });

/** 打开(或聚焦)文本对比独立窗口，返回是否为新创建 */
export const textDiffOpen = () => invoke<boolean>("textdiff_open");
