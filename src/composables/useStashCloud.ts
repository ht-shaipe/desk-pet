/**
 * 收藏云同步（复用 stash 扩展与 hub 服务端的既有契约）：
 * - 推送：POST /api/cms/favorite/batch 全量 upsert
 * - 拉取：POST /api/cms/favorite/pull 游标分页全量拉取（不用 updatedAfter 增量，防各端时钟偏差漏数据）+ deleted 列表
 * - 删除：POST /api/cms/favorite/batch_delete 批量软删除（墓碑），随拉取同步到各端
 * - 仓库数据刷新：GitHub/Gitee 公开 API 更新 stars/forks/languages
 *
 * 鉴权 Token 取账户设置 auth_token（裸 Authorization 头，不带 Bearer），与 stash 扩展一致。
 */
import { fetch as httpFetch } from "@tauri-apps/plugin-http";
import { debugLog } from "@/api/system";
import { useConfigStore } from "@/stores/config";
import { useFavoritesStore } from "@/stores/favorites";
import { isoNow, repoHostOf, serverToLocal, toServerProject } from "@/lib/stash";
import { unwrapHub } from "@/lib/hub";
import type { Favorite } from "@/types";

const API_BASE = "https://hub.htui.cc/api/cms/favorite";
const PULL_PAGE_SIZE = 500;

/** printf 风格参数 -> 单行文本：占位替换 + 多余参数追加（落盘日志用）。 */
function formatLogArgs(args: unknown[]): string {
  let i = 1;
  const fmt = String(args[0] ?? "").replace(/%[sdijO]/g, () => String(args[i++] ?? ""));
  const rest = args
    .slice(i)
    .map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
    .join(" ");
  return `${fmt}${rest ? ` ${rest}` : ""}`.replace(/\s+/g, " ").trim().slice(0, 2000);
}

/** 调试日志：DevTools Console 过滤 [stash-cloud]；同时落盘 app data 的 debug.log（不阻塞主流程）。 */
function dlog(...args: unknown[]): void {
  console.info("[stash-cloud]", ...args);
  void debugLog("stash-cloud", formatLogArgs(args)).catch(() => {});
}

/** 日志里掩码显示 token，只留前 6 位与长度。 */
function maskToken(token: string): string {
  return token ? `${token.slice(0, 6)}…(len ${token.length})` : "(空)";
}

/** 非 2xx 响应：带上截断的响应体抛出，便于定位服务端返回的真实原因。 */
async function httpError(resp: Response): Promise<Error> {
  const body = (await resp.text().catch(() => "")).slice(0, 300);
  dlog("响应异常: status=%d body=%s", resp.status, body || "(空)");
  return new Error(`HTTP ${resp.status}${body ? ` - ${body}` : ""}`);
}

function authHeaders(token: string): Record<string, string> {
  return { "Content-Type": "application/json", Authorization: token };
}

/** 推送全部收藏到云端，返回成功条数。 */
export async function pushFavorites(): Promise<number> {
  const config = useConfigStore();
  const store = useFavoritesStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    throw new Error("NO_TOKEN");
  }
  const items = store.entries.map(toServerProject);
  const resp = await httpFetch(`${API_BASE}/batch`, {
    method: "POST",
    headers: authHeaders(token),
    body: JSON.stringify({ items }),
  });
  if (!resp.ok) {
    throw await httpError(resp);
  }
  unwrapHub<unknown>(await resp.text());
  dlog("推送成功: %d 条", items.length);
  return items.length;
}

/** 增量拉取结果统计。 */
export interface PullResult {
  merged: number;
  deleted: number;
}

/** 游标分页全量拉取：本地 upsert 数据、应用云端删除列表；LWW 合并保证旧数据不覆盖本地新编辑。 */
export async function pullFavorites(): Promise<PullResult> {
  try {
    return await doPullFavorites();
  } catch (e) {
    dlog("拉取失败: %s", String(e));
    throw e;
  }
}

async function doPullFavorites(): Promise<PullResult> {
  const config = useConfigStore();
  const store = useFavoritesStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    dlog("拉取中止: 未配置 auth_token");
    throw new Error("NO_TOKEN");
  }

  // 注意：不走 updatedAfter 增量。云端行的 update_time 是各推送端自己写的时间戳，
  // 时钟/时区偏差或延迟推送会让「最新数据」的时间早于本地基线，增量条件永远漏掉它。
  // 收藏数据量小，每次全量分页拉取 + 本地 LWW 合并最稳。
  const lastSync = config.settings.favorite_last_sync ?? "";
  dlog("开始拉取: token=%s, favorite_sync=%s, 上次同步=%s", maskToken(token), config.settings.favorite_sync, lastSync || "(无)");
  let cursor = "";
  let serverNow = "";
  const items: Favorite[] = [];
  const deleted: string[] = [];

  // 游标分页拉全量；轮次上限防异常服务端死循环
  let rounds = 0;
  do {
    const body: Record<string, unknown> = { limit: PULL_PAGE_SIZE };
    if (cursor) {
      body.cursor = cursor;
    }
    dlog("pull 请求第%d轮: body=%s", rounds + 1, JSON.stringify(body));
    const resp = await httpFetch(`${API_BASE}/pull`, {
      method: "POST",
      headers: authHeaders(token),
      body: JSON.stringify(body),
    });
    const raw = await resp.text();
    dlog("pull 第%d轮原始响应: HTTP %d, body=%s", rounds + 1, resp.status, raw.slice(0, 800));
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}${raw ? ` - ${raw.slice(0, 300)}` : ""}`);
    }
    const data = unwrapHub<{
      items?: Record<string, unknown>[];
      deleted?: string[];
      nextCursor?: string;
      serverNow?: string;
    }>(raw);
    dlog(
      "pull 第%d轮响应: items=%d deleted=%d nextCursor=%s serverNow=%s",
      rounds + 1,
      (data.items ?? []).length,
      (data.deleted ?? []).length,
      data.nextCursor ?? "(无)",
      data.serverNow ?? "(无)",
    );
    let rejected = 0;
    let rejectedSample: unknown = null;
    for (const raw of data.items ?? []) {
      const mapped = serverToLocal(raw);
      if (mapped) {
        items.push(mapped);
      } else {
        rejected += 1;
        rejectedSample ??= raw;
      }
    }
    if (rejected) {
      dlog("pull 第%d轮: %d 条被 serverToLocal 丢弃, 样例:", rounds + 1, rejected, rejectedSample);
    }
    deleted.push(...(data.deleted ?? []));
    serverNow = data.serverNow ?? "";
    cursor = data.nextCursor ?? "";
    rounds += 1;
  } while (cursor && rounds < 200);
  dlog("拉取完成: 合计新增=%d 条, 云端删除=%d 条, 轮次=%d", items.length, deleted.length, rounds);

  const merged = await store.merge(items);
  let removed = 0;
  // 云端删除列表可能含 stash://note/... 伪地址（笔记），不过滤 http 前缀
  const deletable = deleted.filter((u) => typeof u === "string" && u.length > 0);
  if (deletable.length > 0) {
    removed = await store.removeMany(deletable);
  }
  if (serverNow) {
    await config.setFavoriteLastSync(serverNow);
    dlog("同步时间已记录: %s（仅展示用，不作为增量游标）", serverNow);
  }
  dlog("本地合并结果: upsert=%d 条, 删除=%d 条", merged, removed);
  return { merged, deleted: removed };
}

/** 把本地删除同步到云端：批量软删除（服务端打 delete_time 墓碑，其它设备经 pull 的 deleted 列表感知）。 */
export async function deleteFavoritesRemote(urls: string[]): Promise<number> {
  if (urls.length === 0) {
    return 0;
  }
  const config = useConfigStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    dlog("云端删除跳过: 未登录, urls=%d", urls.length);
    return 0;
  }
  const resp = await httpFetch(`${API_BASE}/batch_delete`, {
    method: "POST",
    headers: authHeaders(token),
    body: JSON.stringify({ urls }),
  });
  const raw = await resp.text();
  dlog("云端删除响应: HTTP %d, urls=%d, body=%s", resp.status, urls.length, raw.slice(0, 300));
  if (!resp.ok) {
    throw new Error(`HTTP ${resp.status}${raw ? ` - ${raw.slice(0, 300)}` : ""}`);
  }
  const result = unwrapHub<{ removed?: number }>(raw);
  const removed = result.removed ?? 0;
  dlog("云端删除完成: 服务端删除 %d / %d 条", removed, urls.length);
  return removed;
}

/** 仓库数据刷新结果。 */
export interface RepoStatsResult {
  updated: number;
  failed: number;
}

/**
 * 刷新 GitHub/Gitee 仓库的 stars/forks/languages 并入库。
 * 与 stash 扩展行为一致：刷新成功且开启同步时，把更新条目推送云端。
 * GitHub 匿名 API 有限流，逐条串行 + 250ms 间隔。
 */
export async function refreshRepoStats(): Promise<RepoStatsResult> {
  const config = useConfigStore();
  const store = useFavoritesStore();
  // 与分组逻辑同用 repoHostOf（剥 www），避免 www.github.com 条目不被刷新
  const repos = store.entries.filter((e) => {
    const h = repoHostOf(e.url);
    return h === "github.com" || h === "gitee.com";
  });
  if (repos.length === 0) {
    return { updated: 0, failed: 0 };
  }

  const updates: Favorite[] = [];
  let failed = 0;
  for (const repo of repos) {
    const parts = repo.url.replace(/^https?:\/\//, "").split("/").filter(Boolean);
    if (parts.length < 3) {
      continue;
    }
    const owner = parts[1];
    const name = parts[2].replace(/\.git$/, "");
    const api =
      repoHostOf(repo.url) === "gitee.com"
        ? `https://gitee.com/api/v5/repos/${owner}/${name}`
        : `https://api.github.com/repos/${owner}/${name}`;
    try {
      const resp = await httpFetch(api, { method: "GET", headers: { Accept: "application/json" } });
      if (!resp.ok) {
        throw new Error(`HTTP ${resp.status}`);
      }
      const data = (await resp.json()) as {
        stargazers_count?: number;
        forks_count?: number;
        language?: string;
      };
      updates.push({
        ...repo,
        stars: data.stargazers_count ?? repo.stars,
        forks: data.forks_count ?? repo.forks,
        languages: data.language ? [data.language] : repo.languages,
        // 碰到了新数据就得盖新时间戳，否则推送/增量基线会拿旧 updateTime
        update_time: isoNow(),
      });
      await new Promise((r) => setTimeout(r, 250));
    } catch {
      failed += 1;
    }
  }

  const updated = await store.merge(updates);

  if (updates.length > 0 && config.settings.favorite_sync && config.settings.auth_token) {
    try {
      const items = updates.map(toServerProject);
      await httpFetch(`${API_BASE}/batch`, {
        method: "POST",
        headers: authHeaders(config.settings.auth_token),
        body: JSON.stringify({ items }),
      });
    } catch {
      // 云推送失败不影响本地刷新结果
    }
  }
  return { updated, failed };
}

const AUTO_PULL_INTERVAL = 5 * 60_000;
let lastAutoPullAt = 0;
let autoPullRunning = false;

/**
 * 启动/打开收藏页时的静默自动拉取：
 * - 仅在「云同步开启 + 已登录」时执行
 * - 5 分钟内至多一次（启动与开页共用节流），失败静默不弹提示
 * - 增量基线由 pullFavorites 内部维护（favorite_last_sync）
 */
export async function autoPullFavorites(): Promise<void> {
  const config = useConfigStore();
  if (!config.settings.favorite_sync || !config.settings.auth_token) {
    return;
  }
  const now = Date.now();
  if (now - lastAutoPullAt < AUTO_PULL_INTERVAL || autoPullRunning) {
    return;
  }
  autoPullRunning = true;
  lastAutoPullAt = now;
  try {
    await pullFavorites();
  } catch (e) {
    // 静默：离线/网络异常时不打扰用户（手动「拉取数据」按钮兜底），仅记录日志便于排查
    dlog("自动拉取失败: %s", String(e));
  } finally {
    autoPullRunning = false;
  }
}
