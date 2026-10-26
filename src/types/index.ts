export type ThemeMode = "light" | "dark" | "auto";

export interface AiModel {
  id: string;
  name: string;
  /** 接口基础地址，如 https://api.openai.com/v1 */
  base_url: string;
  api_key: string;
  /** 模型名，如 gpt-4o-mini */
  model: string;
}

export interface ChatMessage {
  /** user | assistant */
  role: string;
  content: string;
}

/** 聊天消息（SQLite 持久化记录）。 */
export interface ChatEntry {
  id: number;
  /** user | assistant */
  role: string;
  content: string;
  created_at: number;
}

export interface AppSettings {
  pet_x: number | null;
  pet_y: number | null;
  always_on_top: boolean;
  locale: string | null;
  theme?: ThemeMode | null;
  sprite_size?: number | null;
  accent_color?: string | null;
  auth_token?: string | null;
  auth_user?: string | null;
  pet_avatar?: string | null;
  app_name?: string | null;
  /** 是否在 Dock 中显示（仅 macOS），缺省 true */
  show_dock?: boolean | null;
  ai_models?: AiModel[];
  ai_active?: string | null;
  /** 收藏云同步开关（token 复用 auth_token） */
  favorite_sync?: boolean | null;
  /** 收藏增量拉取基线（服务端时间） */
  favorite_last_sync?: string | null;
  /** 番茄钟自定义时长（分钟），缺省用默认值 */
  pomo_focus_min?: number | null;
  pomo_short_min?: number | null;
  pomo_long_min?: number | null;
}

export type FavType = "repo" | "article" | "video" | "product" | "social" | "image" | "bookmark" | "note";

/** 收藏条目：与 stash 扩展 / hub 云端字段一致。 */
export interface Favorite {
  url: string;
  name: string;
  description: string;
  fav_type: string;
  /** 附加载荷 JSON 字符串（note 正文、图片地址等） */
  payload: string;
  languages: string[];
  stars: number;
  forks: number;
  tags: string[];
  category: number;
  category_name: string;
  /** ISO 时间字符串 */
  create_time: string;
  update_time: string;
}

export type LaunchType = "app" | "url" | "command";

export interface LauncherItem {
  id: string;
  name: string;
  icon: string;
  item_type: LaunchType;
  value: string;
  color: string;
}

export interface LauncherConfig {
  items: LauncherItem[];
}

/** 系统已安装应用（供启动项选择）。 */
export interface AppEntry {
  name: string;
  value: string;
}

export interface ClipEntry {
  id: string;
  text: string;
  kind: string;
  updated_at: number;
  /** 相同内容累计复制次数 */
  copy_count: number;
}

export interface ImagePayload {
  width: number;
  height: number;
  rgba: number[];
}

/** 目录批量压缩：目录内图片文件信息 */
export interface ImageFileInfo {
  name: string;
  /** 文件大小（字节） */
  size: number;
}

/* ---------- 接口请求 ---------- */

/** 请求头 / 查询参数键值对 */
export interface HttpHeader {
  name: string;
  value: string;
}

/** 交给 Rust 代理发送的请求描述 */
export interface HttpRequestSpec {
  url: string;
  method: string;
  params: HttpHeader[];
  headers: HttpHeader[];
  body?: string;
  bodyType?: "json" | "text";
}

/** Rust 代理返回的请求结果（含实际请求头与完整响应头） */
export interface HttpResponseData {
  status: number;
  statusText: string;
  durationMs: number;
  /** 响应体字节数（截断前） */
  size: number;
  requestHeaders: HttpHeader[];
  responseHeaders: HttpHeader[];
  body: string;
  truncated: boolean;
  /** 非文本类型响应，前端不展示内容 */
  binary: boolean;
  /** 跟随重定向后的最终地址 */
  finalUrl: string;
}

/* ---------- 磁盘清理 ---------- */

export type CleanupKind = "logs" | "target" | "node_modules";

export interface CleanupCandidate {
  id: string;
  kind: CleanupKind;
  path: string;
  bytes: number;
}

/** 后端创建扫描/删除任务后返回的 SSE 连接信息。 */
export interface CleanupStream {
  jobId: string;
  url: string;
}

/** 扫描目录所在磁盘卷的空间情况（字节）。 */
export interface DiskUsage {
  total: number;
  available: number;
}

export type CleanupEvent =
  | { type: "started"; mode: "scan" | "delete"; root: string }
  | {
      type: "progress";
      dirs: number;
      files: number;
      bytes: number;
      errors: number;
      current: string;
    }
  | { type: "found"; items: CleanupCandidate[] }
  | { type: "scanDone"; count: number; bytes: number; errors: number; truncated: boolean }
  | { type: "deleting"; id: string; kind: CleanupKind; path: string; bytes: number }
  | { type: "deleted"; id: string; ok: boolean; bytes: number; error: string | null }
  | { type: "deleteDone"; removed: number; failed: number; bytes: number }
  | { type: "cancelled"; mode: "scan" | "delete" };
