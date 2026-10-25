import type { Favorite, FavType } from "@/types";

/** 与 stash 扩展一致的 19 个预设分类。 */
export const FAV_CATEGORIES = [
  "AI/Agent",
  "前端框架",
  "后端框架",
  "UI 组件库",
  "工具链/构建",
  "DevOps / CI/CD",
  "数据库",
  "网络/HTTP",
  "安全",
  "测试",
  "文档/知识管理",
  "运维/监控",
  "移动端",
  "桌面端",
  "游戏",
  "音视频",
  "区块链",
  "编程语言",
  "其他",
] as const;

/** 收藏类型元信息（颜色沿用 stash 配色）。 */
export const FAV_TYPES: Array<{ value: FavType; color: string }> = [
  { value: "repo", color: "#2563eb" },
  { value: "article", color: "#0ea5e9" },
  { value: "video", color: "#ef4444" },
  { value: "product", color: "#f59e0b" },
  { value: "social", color: "#ec4899" },
  { value: "image", color: "#10b981" },
  { value: "bookmark", color: "#3b82f6" },
  { value: "note", color: "#64748b" },
];

export function favTypeColor(type: string): string {
  return FAV_TYPES.find((t) => t.value === type)?.color ?? "#3b82f6";
}

/** 常用编程语言颜色（与 stash 扩展一致）。 */
export const LANGUAGE_COLORS: Record<string, string> = {
  JavaScript: "#f1e05a", TypeScript: "#3178c6", Python: "#3572A5",
  Java: "#b07219", Go: "#00ADD8", Rust: "#dea584",
  "C++": "#f34b7d", C: "#555555", "C#": "#178600",
  Ruby: "#701516", PHP: "#4F5D95", Swift: "#F05138",
  Kotlin: "#A97BFF", Dart: "#00B4AB", Shell: "#89e051",
  Vue: "#41b883", HTML: "#e34c26", CSS: "#563d7c",
  Scala: "#c22d40", R: "#198CE7", Lua: "#000080",
  Zig: "#ec915c", Haskell: "#5e5086", Elixir: "#6e4a7e",
  "Objective-C": "#438eff", Perl: "#0298c3", Clojure: "#db5855",
  CoffeeScript: "#244776", Groovy: "#e69f56", Julia: "#a270ba",
};

export function languageColor(lang: string): string {
  return LANGUAGE_COLORS[lang] ?? "#8b8b8b";
}

/** 代码托管平台：host 关键字 -> 平台名（用于分组与仓库识别）。 */
const REPO_HOSTS: Array<[string, string]> = [
  ["github.com", "GitHub"],
  ["gitlab.com", "GitLab"],
  ["gitee.com", "Gitee"],
  ["bitbucket.org", "Bitbucket"],
  ["codeberg.org", "Codeberg"],
  ["sourceforge.net", "SourceForge"],
];

/** 从 URL 提取平台平台关键字，非托管平台返回 null。 */
export function repoHostOf(url: string): string | null {
  try {
    const host = new URL(url).hostname.replace(/^www\./, "");
    for (const [key] of REPO_HOSTS) {
      if (host.includes(key)) {
        return key;
      }
    }
    return null;
  } catch {
    return null;
  }
}

/** 是否为笔记（stash:// 伪地址或 note 类型）。 */
export function isNoteItem(item: { url: string; fav_type: string }): boolean {
  return item.fav_type === "note" || item.url.startsWith("stash://");
}

/**
 * 分组键：
 * - 笔记单独成组
 * - repo 按托管平台分组
 * - 其余按收藏类型分组
 * - 兜底按主机名
 */
export function groupKeyOf(item: { url: string; fav_type: string }): string {
  if (isNoteItem(item)) {
    return "note";
  }
  if (item.fav_type === "repo") {
    return repoHostOf(item.url) ?? cleanHost(item.url) ?? "default";
  }
  if (item.fav_type) {
    return "type:" + item.fav_type;
  }
  return cleanHost(item.url) ?? "default";
}

function cleanHost(url: string): string | null {
  try {
    return new URL(url).hostname.replace(/^www\./, "") || null;
  } catch {
    return null;
  }
}

export function platformLabelOf(key: string): string {
  for (const [host, label] of REPO_HOSTS) {
    if (key.includes(host)) {
      return label;
    }
  }
  return key;
}

// ---------- 时间格式（stash 云端为 "YYYY-MM-DD HH:MM:SS"，内部统一 ISO） ----------

/** 云端/导入的本地字符串时间 -> ISO；无法解析返回空串。 */
export function normalizeServerTime(t: string | null | undefined): string {
  if (!t) {
    return "";
  }
  const d = new Date(String(t).replace(" ", "T"));
  return Number.isNaN(d.getTime()) ? "" : d.toISOString();
}

/** ISO -> 云端 "YYYY-MM-DD HH:MM:SS"。 */
export function toServerTime(t: string | null | undefined): string {
  if (!t) {
    return "";
  }
  const d = new Date(t);
  if (Number.isNaN(d.getTime())) {
    return "";
  }
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function isoNow(): string {
  return new Date().toISOString();
}

// ---------- 导入归一化 / 导出序列化 ----------

/** 任意来源（字符串 / 对象）的 payload 归一为可入库 JSON 串；损坏 / 非对象回落 "{}"。 */
function payloadJson(value: unknown): string {
  return JSON.stringify(payloadObject(value));
}

/** payload 的对象形态：字符串则解析，损坏 / 非对象回落 {}。 */
function payloadObject(value: unknown): Record<string, unknown> {
  const raw =
    typeof value === "string"
      ? (() => {
          try {
            const v = JSON.parse(value);
            return v && typeof v === "object" && !Array.isArray(v) ? v : null;
          } catch {
            return null;
          }
        })()
      : value;
  return raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
}

/** 从 payload JSON 串安全读取字段（损坏时回退新对象）。 */
export function parsePayload(payload: string): Record<string, string> {
  try {
    const v = JSON.parse(payload || "{}");
    if (v && typeof v === "object" && !Array.isArray(v)) {
      const out: Record<string, string> = {};
      for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
        if (typeof val === "string") {
          out[k] = val;
        } else if (val != null) {
          out[k] = String(val);
        }
      }
      return out;
    }
  } catch {
    // fallthrough
  }
  return {};
}

/** 推送到云端的项目结构（另字段名/时间格式与后端契约一致）。 */
export function toServerProject(f: Favorite): Record<string, unknown> {
  return {
    url: f.url,
    name: f.name ?? "",
    description: f.description ?? "",
    favType: f.fav_type || "repo",
    payload: payloadObject(f.payload),
    languages: f.languages ?? [],
    stars: f.stars ?? 0,
    forks: f.forks ?? 0,
    tags: f.tags ?? [],
    category: typeof f.category === "number" ? f.category : 0,
    categoryName: f.category_name || null,
    createTime: toServerTime(f.create_time),
    updateTime: toServerTime(f.update_time),
  };
}

/** 云端 pull 的项目 -> 内部 Favorite（含时间格式归一）。 */
export function serverToLocal(p: Record<string, unknown>): Favorite | null {
  const url = typeof p.url === "string" ? p.url.trim() : "";
  if (!url) {
    return null;
  }
  const category = typeof p.category === "number" ? p.category : 0;
  const categoryName = typeof p.categoryName === "string" ? p.categoryName : "其他";
  const created = normalizeServerTime(p.createTime as string) || isoNow();
  return {
    url,
    name: (p.name as string) || "",
    description: (p.description as string) || "",
    fav_type: (p.favType as string) || "repo",
    payload: payloadJson(p.payload),
    languages: (p.languages as string[]) ?? [],
    stars: typeof p.stars === "number" ? p.stars : 0,
    forks: typeof p.forks === "number" ? p.forks : 0,
    tags: (p.tags as string[]) ?? [],
    category,
    category_name: categoryName,
    create_time: created,
    update_time: normalizeServerTime(p.updateTime as string) || isoNow(),
  };
}

/**
 * 归一化 stash 扩展导出的 JSON 数组为 Favorite[]。
 * 兼容旧字段（type/createdAt/updatedAt、字符串 category、语言单字段 language），
 * 不认识的条目跳过。
 */
export function normalizeImported(raw: unknown): Favorite[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const out: Favorite[] = [];
  for (const p of raw) {
    if (!p || typeof p !== "object") {
      continue;
    }
    const src = p as Record<string, unknown>;
    const url = typeof src.url === "string" ? src.url.trim() : "";
    if (!url) {
      continue;
    }
    // category：新导出为 number（分类号），旧导出可能是分类名字符串
    let category = 0;
    let categoryName = "其他";
    if (typeof src.categoryName === "string" && src.categoryName) {
      categoryName = src.categoryName;
    } else if (typeof src.category === "string" && src.category) {
      categoryName = src.category;
    } else if (typeof src.category === "number") {
      category = src.category;
      const idx = FAV_CATEGORIES[category];
      categoryName = idx ?? "其他";
    }
    const languages =
      Array.isArray(src.languages) && src.languages.every((l) => typeof l === "string")
        ? (src.languages as string[])
        : typeof src.language === "string" && src.language
          ? [src.language]
          : [];
    const payloadString = payloadJson(src.payload);
    out.push({
      url,
      name: typeof src.name === "string" ? src.name : "",
      description: typeof src.description === "string" ? src.description : "",
      fav_type: typeof src.favType === "string" ? src.favType : typeof src.type === "string" ? src.type : "repo",
      payload: payloadString,
      languages,
      stars: typeof src.stars === "number" ? src.stars : 0,
      forks: typeof src.forks === "number" ? src.forks : 0,
      tags: Array.isArray(src.tags) ? (src.tags as string[]).filter((t) => typeof t === "string") : [],
      category,
      category_name: categoryName,
      create_time:
        normalizeServerTime(src.createTime as string) ||
        normalizeServerTime(src.createdAt as string) ||
        normalizeServerTime(src.create_time as string) ||
        isoNow(),
      update_time:
        normalizeServerTime(src.updateTime as string) ||
        normalizeServerTime(src.updatedAt as string) ||
        "",
    });
  }
  return out;
}

/**
 * 从仓库/网页 URL 快速建档（剪贴板联动 / 手动添加的默认值共用）。
 * 托管平台且路径形如 owner/repo 的识别为 repo，其余为 bookmark。
 */
export function buildFavoriteFromUrl(url: string, overrides: Partial<Favorite> = {}): Favorite {
  let name = "";
  let isRepo = false;
  try {
    const u = new URL(url);
    const parts = u.pathname.replace(/\/$/, "").split("/").filter(Boolean);
    const host = repoHostOf(url);
    if (host === "sourceforge.net" && parts.length >= 2 && parts[0] === "projects") {
      name = `sourceforge/${parts[1]}`;
      isRepo = true;
    } else if (host && parts.length >= 2) {
      name = `${parts[0]}/${parts[1]}`;
      isRepo = true;
    } else {
      name = parts.length > 0 ? parts[parts.length - 1] : u.hostname;
    }
  } catch {
    name = url;
  }
  const now = isoNow();
  const catIndex = FAV_CATEGORIES.indexOf("其他");
  return {
    url,
    name: overrides.name ?? name,
    description: overrides.description ?? "",
    fav_type: overrides.fav_type ?? (isRepo ? "repo" : "bookmark"),
    payload: overrides.payload ? JSON.stringify(overrides.payload) : "{}",
    languages: overrides.languages ?? [],
    stars: overrides.stars ?? 0,
    forks: overrides.forks ?? 0,
    tags: overrides.tags ?? [],
    category: overrides.category ?? Math.max(0, catIndex),
    category_name: overrides.category_name ?? "其他",
    create_time: overrides.create_time ?? now,
    update_time: overrides.update_time ?? now,
  };
}

/** 导出格式与 stash 扩展一致（stash 可直接导入该 JSON）。 */
export function exportFavorites(items: Favorite[]): string {
  return JSON.stringify(
    items.map((f) => ({
      url: f.url,
      name: f.name,
      description: f.description,
      favType: f.fav_type || "repo",
      payload: payloadObject(f.payload),
      languages: f.languages ?? [],
      stars: f.stars ?? 0,
      forks: f.forks ?? 0,
      tags: f.tags ?? [],
      category: f.category ?? 0,
      categoryName: f.category_name || null,
      createTime: f.create_time || "",
      updateTime: f.update_time || "",
    })),
    null,
    2
  );
}

