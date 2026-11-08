/**
 * 剪贴板云同步（镜像收藏同步的 hub 服务端契约）：
 * - 推送：POST /api/cms/clipboard/batch 全量 upsert（仅文本条目，图片依赖本地文件不参与）
 * - 拉取：POST /api/cms/clipboard/pull 游标分页增量（updatedAfter/serverNow）+ deleted 列表
 *
 * 鉴权 Token 取账户设置 auth_token（裸 Authorization 头，不带 Bearer），与收藏同步一致。
 */
import { fetch as httpFetch } from "@tauri-apps/plugin-http";
import { useConfigStore } from "@/stores/config";
import { useClipboardStore } from "@/stores/clipboard";
import { mergeClipboardEntries } from "@/api/clipboard";
import { normalizeServerTime, toServerTime } from "@/lib/stash";
import { unwrapHub } from "@/lib/hub";
import type { ClipEntry } from "@/types";

const API_BASE = "https://hub.htui.cc/api/cms/clipboard";
const PULL_PAGE_SIZE = 500;

function authHeaders(token: string): Record<string, string> {
  return { "Content-Type": "application/json", Authorization: token };
}

/** 本地条目 -> 云端结构（时间统一 "YYYY-MM-DD HH:MM:SS"）。 */
function toServerClip(e: ClipEntry): Record<string, unknown> {
  return {
    id: e.id,
    text: e.text,
    kind: e.kind,
    copyCount: e.copy_count,
    sig: e.sig ?? "",
    updateTime: toServerTime(new Date(e.updated_at).toISOString()),
  };
}

/** 云端条目 -> 本地 ClipEntry（时间解析失败回退当前时间）。 */
function serverToLocalClip(p: Record<string, unknown>): ClipEntry | null {
  const id = typeof p.id === "string" ? p.id.trim() : "";
  const text = typeof p.text === "string" ? p.text : "";
  if (!id || !text.trim()) {
    return null;
  }
  const iso = normalizeServerTime(p.updateTime as string);
  const copyCount = typeof p.copyCount === "number" && p.copyCount > 0 ? Math.floor(p.copyCount) : 1;
  return {
    id,
    text,
    kind: typeof p.kind === "string" && p.kind ? p.kind : "text",
    updated_at: iso ? new Date(iso).getTime() : Date.now(),
    copy_count: copyCount,
    sig: typeof p.sig === "string" ? p.sig : "",
  };
}

/** 推送全部文本条目到云端，返回成功条数。 */
export async function pushClipboard(): Promise<number> {
  const config = useConfigStore();
  const store = useClipboardStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    throw new Error("NO_TOKEN");
  }
  const items = store.entries.filter((e) => e.kind !== "image").map(toServerClip);
  const resp = await httpFetch(`${API_BASE}/batch`, {
    method: "POST",
    headers: authHeaders(token),
    body: JSON.stringify({ items }),
  });
  if (!resp.ok) {
    throw new Error(`HTTP ${resp.status}`);
  }
  unwrapHub<unknown>(await resp.text());
  return items.length;
}

/** 增量拉取结果统计。 */
export interface ClipPullResult {
  merged: number;
  deleted: number;
}

/** 游标分页增量拉取：本地合并新数据、删除云端已删列表，并推进基线。 */
async function doPullClipboard(): Promise<ClipPullResult> {
  const config = useConfigStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    throw new Error("NO_TOKEN");
  }

  const baseline = config.settings.clipboard_last_sync ?? "";
  let cursor = "";
  let serverNow = "";
  const items: ClipEntry[] = [];
  const deleted: string[] = [];

  // 游标分页拉全量；轮次上限防异常服务端死循环
  let rounds = 0;
  do {
    const body: Record<string, unknown> = { limit: PULL_PAGE_SIZE };
    if (baseline) {
      body.updatedAfter = baseline;
    }
    if (cursor) {
      body.cursor = cursor;
    }
    const resp = await httpFetch(`${API_BASE}/pull`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(body),
    });
    const raw = await resp.text();
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}${raw ? ` - ${raw.slice(0, 300)}` : ""}`);
    }
    const data = unwrapHub<{
      items?: Record<string, unknown>[];
      deleted?: string[];
      nextCursor?: string;
      serverNow?: string;
    }>(raw);
    for (const raw of data.items ?? []) {
      const mapped = serverToLocalClip(raw);
      if (mapped) {
        items.push(mapped);
      }
    }
    deleted.push(...(data.deleted ?? []));
    serverNow = data.serverNow ?? "";
    cursor = data.nextCursor ?? "";
    rounds += 1;
  } while (cursor && rounds < 200);

  // 仅同步文本条目：图片依赖本地 PNG 文件，云端条目不落地
  const merged = await mergeClipboardEntries(
    items.filter((e) => e.kind !== "image"),
    deleted.filter((d) => typeof d === "string" && d.length > 0)
  );
  if (serverNow) {
    await config.setClipboardLastSync(serverNow);
  }
  return { merged, deleted: deleted.length };
}

// 共享在途拉取：手动「从云端拉取」与静默自动拉取并发时复用同一次分页请求，
// 避免双份游标分页互相推进基线、重复合并。
let pullInflight: Promise<ClipPullResult> | null = null;

/** 增量拉取（在途去重）：已在拉取时直接返回同一个 Promise。 */
export function pullClipboard(): Promise<ClipPullResult> {
  if (!pullInflight) {
    pullInflight = doPullClipboard().finally(() => {
      pullInflight = null;
    });
  }
  return pullInflight;
}

const AUTO_PULL_INTERVAL = 5 * 60_000;
let lastAutoPullAt = 0;
let autoPullRunning = false;

/**
 * 启动/打开剪贴板页时的静默自动拉取：
 * - 仅在「云同步开启 + 已登录」时执行
 * - 5 分钟内至多一次（启动与开页共用节流），失败静默不弹提示
 * - 增量基线由 pullClipboard 内部维护（clipboard_last_sync）
 */
export async function autoPullClipboard(): Promise<void> {
  const config = useConfigStore();
  if (!config.settings.clipboard_sync || !config.settings.auth_token) {
    return;
  }
  const now = Date.now();
  if (now - lastAutoPullAt < AUTO_PULL_INTERVAL || autoPullRunning) {
    return;
  }
  autoPullRunning = true;
  lastAutoPullAt = now;
  try {
    await pullClipboard();
  } catch {
    // 静默：离线/网络异常时不打扰用户，手动「从云端拉取」按钮兜底
  } finally {
    autoPullRunning = false;
  }
}
