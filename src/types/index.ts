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
  /** 稳定设备指纹（gRPC 通道握手用，安装后生成，前端只读透传） */
  device_id?: string | null;
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
  /** 剪贴板云同步开关（token 复用 auth_token） */
  clipboard_sync?: boolean | null;
  /** 剪贴板增量拉取基线（服务端时间） */
  clipboard_last_sync?: string | null;
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
  /** 图片内容签名（文本条目为空串） */
  sig?: string;
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

/** 内容类别（与 deskpet-disk classify::Category 对齐）。 */
export type CleanupCategory =
  | "code"
  | "agentScratch"
  | "toolchain"
  | "synced"
  | "git"
  | "media"
  | "documents"
  | "cache"
  | "other";

/** 可回收原因：为什么可以删。 */
export type ReclaimReason =
  | "regenerable"
  | "syncHistory"
  | "packageStore"
  | "buildOutput"
  | "reinstallable"
  | "sandboxLayers"
  | "snapshots"
  | "trash"
  | "temporary";

/** 风险分级：safe 可默认勾选；caution 需手动选；risky 不进推荐。 */
export type CleanupRisk = "safe" | "caution" | "risky";

/** 清理候选：展示路径同时作为稳定 key（重扫后失效需重扫）。 */
export interface DiskCandidate {
  path: string;
  name: string;
  kind: "dir" | "file";
  /** 分配大小口径（删除后真正能释放的字节），硬链接只计一次 */
  bytes: number;
  files: number;
  /** 最近修改时间（Unix 秒，0 = 未知） */
  modified: number;
  ruleId: string;
  category: CleanupCategory;
  reclaim: ReclaimReason | null;
  risk: CleanupRisk;
  /** 目录体积是否完整（扫描预算内完成） */
  complete: boolean;
  /** 扫描完成后是否默认勾选 */
  defaultSelected: boolean;
}

/** 后端创建扫描/删除任务后返回的 SSE 连接信息。 */
export interface CleanupStream {
  jobId: string;
  url: string;
}

/** 可扫描卷（根分区 + 外接/网络卷）。 */
export interface DiskVolume {
  path: string;
  total: number;
  available: number;
}

/** 扫描目录所在磁盘卷的空间情况（字节）。 */
export interface DiskUsage {
  total: number;
  available: number;
}

/** 删除计划中的目标/被吸收项。 */
export interface PlanTarget {
  path: string;
  bytes: number;
  isDir: boolean;
  /** 永久删除危险度（仅分析窗口计划携带） */
  gitRepos?: number | null;
  nonReclaimableBytes?: number | null;
}

/** 被安全规则拒绝的项及原因。 */
export interface PlanBlocked {
  path: string;
  error: string;
}

/** 删除计划：复核清单的数据来源。 */
export interface RemovalPlan {
  targets: PlanTarget[];
  covered: PlanTarget[];
  blocked: PlanBlocked[];
  reclaim: number;
}

export type CleanupRemovalMode = "trash" | "permanent";

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
  | { type: "found"; items: DiskCandidate[] }
  | {
      type: "preview";
      /** 顶层目录聚合（只读，扫描中不可操作）；name 为空表示溢出桶 */
      entries: { name: string; bytes: number; files: number }[];
      elapsedMs: number;
    }
  | {
      type: "scanDone";
      count: number;
      bytes: number;
      files: number;
      errors: number;
      truncated: boolean;
      complete: boolean;
      durationMs: number;
    }
  | { type: "deleting"; path: string; bytes: number; mode: CleanupRemovalMode }
  | { type: "deleted"; path: string; ok: boolean; bytes: number; error: string | null }
  | {
      type: "deleteDone";
      removed: number;
      failed: number;
      bytes: number;
      /** statvfs 前后实测差值，可能小于估算（APFS 克隆/快照/其他进程写入） */
      freedBytes: number;
      freeBefore: number;
      freeAfter: number;
      failures: { path: string; error: string }[];
    }
  | { type: "cancelled"; mode: "scan" | "delete" }
  | { type: "failed"; mode: "scan" | "delete"; error: string };

/* ---------- 磁盘分析（treemap） ---------- */

export type DiskMetric = "bytes" | "files";

export interface LayoutTile {
  path: string;
  name: string;
  kind: "dir" | "files" | "others";
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  /** 目录 tile 的标题条；小 tile 为 null */
  header: [number, number, number, number] | null;
  bytes: number;
  files: number;
  category: CleanupCategory;
  reclaim: ReclaimReason | null;
  risk: CleanupRisk;
  /** Others tile 合并的数量 */
  count: number | null;
  hasChildren: boolean;
}

export interface DiskLayout {
  root: string;
  tiles: LayoutTile[];
  totalBytes: number;
  totalFiles: number;
  truncated: boolean;
}

/** 分析侧栏"值得看看"条目。 */
export interface AnalyzerInsight {
  path: string;
  name: string;
  bytes: number;
  files: number;
  category: CleanupCategory;
  reclaim: ReclaimReason | null;
  risk: CleanupRisk;
}

/** 标记项在当前树中的状态（重扫后大小刷新、消失项检出）。 */
export interface DiskMarkInfo {
  path: string;
  bytes: number;
  files: number;
  exists: boolean;
}
