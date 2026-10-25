/**
 * 收藏云同步（复用 stash 扩展与 hub 服务端的既有契约）：
 * - 推送：POST /api/cms/favorite/batch 全量 upsert
 * - 拉取：POST /api/cms/favorite/pull 游标分页增量（updatedAfter/serverNow）+ deleted 列表
 * - 仓库数据刷新：GitHub/Gitee 公开 API 更新 stars/forks/languages
 *
 * 鉴权 Token 取账户设置 auth_token（裸 Authorization 头，不带 Bearer），与 stash 扩展一致。
 */
import { fetch as httpFetch } from "@tauri-apps/plugin-http";
import { useConfigStore } from "@/stores/config";
import { useFavoritesStore } from "@/stores/favorites";
import { isoNow, repoHostOf, serverToLocal, toServerProject } from "@/lib/stash";
import type { Favorite } from "@/types";

const API_BASE = "https://hub.htui.cc/api/cms/favorite";
const PULL_PAGE_SIZE = 500;

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
    throw new Error(`HTTP ${resp.status}`);
  }
  return items.length;
}

/** 增量拉取结果统计。 */
export interface PullResult {
  merged: number;
  deleted: number;
}

/** 游标分页增量拉取：本地 upsert 新数据、删除云端已删列表，并推进基线。 */
export async function pullFavorites(): Promise<PullResult> {
  const config = useConfigStore();
  const store = useFavoritesStore();
  const token = config.settings.auth_token ?? "";
  if (!token) {
    throw new Error("NO_TOKEN");
  }

  const baseline = config.settings.favorite_last_sync ?? "";
  let cursor = "";
  let serverNow = "";
  const items: Favorite[] = [];
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
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}`);
    }
    const data = (await resp.json()) as {
      items?: Record<string, unknown>[];
      deleted?: string[];
      nextCursor?: string;
      serverNow?: string;
    };
    for (const raw of data.items ?? []) {
      const mapped = serverToLocal(raw);
      if (mapped) {
        items.push(mapped);
      }
    }
    deleted.push(...(data.deleted ?? []));
    serverNow = data.serverNow ?? "";
    cursor = data.nextCursor ?? "";
    rounds += 1;
  } while (cursor && rounds < 200);

  const merged = await store.merge(items);
  let removed = 0;
  // 云端删除列表可能含 stash://note/... 伪地址（笔记），不过滤 http 前缀
  const deletable = deleted.filter((u) => typeof u === "string" && u.length > 0);
  if (deletable.length > 0) {
    removed = await store.removeMany(deletable);
  }
  if (serverNow) {
    await config.setFavoriteLastSync(serverNow);
  }
  return { merged, deleted: removed };
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
  } catch {
    // 静默：离线/网络异常时不打扰用户，手动「拉取数据」按钮兜底
  } finally {
    autoPullRunning = false;
  }
}
