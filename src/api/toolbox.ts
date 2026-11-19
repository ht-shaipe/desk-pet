import { invoke } from "@tauri-apps/api/core";
import type {
  CompressImageParams,
  CompressImageResult,
  DocumentConvertResult,
  HttpCookieEntry,
  HttpEnvItem,
  HttpHistoryItem,
  HttpRequestSpec,
  HttpResponseData,
  HttpRespRecordItem,
  HttpSavedItem,
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

/** 接口请求选项 */
export interface HttpRequestOptions {
  /** 取消信号。触发后前端立即结束等待，Rust 侧请求在后台完成但结果被丢弃。 */
  signal?: AbortSignal;
}

/** 接口请求：由 Rust 代理发送，规避跨域；成败均自动记入历史 */
export const httpRequest = (
  spec: HttpRequestSpec,
  options?: HttpRequestOptions,
) => {
  const invokePromise = invoke<HttpResponseData>("http_request", { spec });
  const signal = options?.signal;
  if (!signal) {
    return invokePromise;
  }
  // Rust 侧请求无法被前端中断，这里通过竞速让前端在用户取消时提前结束等待；
  // Rust 侧请求会在后台继续完成，但结果会被丢弃。
  return new Promise<HttpResponseData>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Request aborted", "AbortError"));
      return;
    }
    const onAbort = () => {
      reject(new DOMException("Request aborted", "AbortError"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    invokePromise.then(
      (resp) => {
        signal.removeEventListener("abort", onAbort);
        resolve(resp);
      },
      (err) => {
        signal.removeEventListener("abort", onAbort);
        reject(err);
      },
    );
  });
};

/** 全部接口收藏（默认分组在前，组内按 sort 升序） */
export const listHttpSaved = () => invoke<HttpSavedItem[]>("list_http_saved");
/** 新增/更新接口收藏（id 留空由 Rust 生成） */
export const upsertHttpSaved = (item: HttpSavedItem) =>
  invoke<void>("upsert_http_saved", { item });
/** 删除接口收藏 */
export const deleteHttpSaved = (id: string) =>
  invoke<number>("delete_http_saved", { id });
/** 重命名集合（整组迁移） */
export const renameHttpGroup = (old: string, newGroup: string) =>
  invoke<number>("rename_http_group", { old, new: newGroup });
/** 删除集合（连同其中收藏） */
export const deleteHttpGroup = (group: string) =>
  invoke<number>("delete_http_group", { group });

/** 最近请求历史（新 → 旧，100 条） */
export const listHttpHistory = () => invoke<HttpHistoryItem[]>("list_http_history");
/** 清空请求历史 */
export const clearHttpHistory = () => invoke<void>("clear_http_history");

/** 全部环境变量集（启用环境在前） */
export const listHttpEnvs = () => invoke<HttpEnvItem[]>("list_http_envs");
/** 新增/更新环境（id 留空由 Rust 生成） */
export const upsertHttpEnv = (item: HttpEnvItem) => invoke<void>("upsert_http_env", { item });
/** 删除环境 */
export const deleteHttpEnv = (id: string) => invoke<number>("delete_http_env", { id });
/** 切换启用环境（null = 不启用） */
export const setActiveHttpEnv = (id: string | null) =>
  invoke<void>("set_active_http_env", { id });

/** 某请求键下的响应历史（新 → 旧，20 条） */
export const listHttpResponses = (specKey: string) =>
  invoke<HttpRespRecordItem[]>("list_http_responses", { specKey });
/** 清空响应历史（specKey 为空表示全部） */
export const clearHttpResponses = (specKey: string | null) =>
  invoke<number>("clear_http_responses", { specKey });

/** 共享 Cookie jar 全部条目 */
export const httpCookieList = () => invoke<HttpCookieEntry[]>("http_cookie_list");
/** 清空共享 Cookie jar */
export const httpCookieClear = () => invoke<void>("http_cookie_clear");

/** 启动 SSE 流式请求（事件经 http-sse 推送），返回 streamId */
export const httpSseStart = (spec: HttpRequestSpec) =>
  invoke<string>("http_sse_start", { spec });
/** 停止 SSE 流式请求 */
export const httpSseStop = (id: string) => invoke<void>("http_sse_stop", { id });

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
