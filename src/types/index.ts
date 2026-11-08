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

/** OpenAI function-calling 的工具定义（发给模型的 tools 数组项）。 */
export interface ToolDefinition {
  type: "function";
  function: {
    name: string;
    description: string;
    /** JSON Schema */
    parameters: Record<string, unknown>;
  };
}

/** 模型返回的一次工具调用。 */
export interface ToolCall {
  id: string;
  type: "function";
  function: {
    name: string;
    /** JSON 字符串形式的入参 */
    arguments: string;
  };
}

export interface ChatMessage {
  /** user | assistant | tool | system */
  role: string;
  content: string;
  /** assistant 消息携带的工具调用列表 */
  tool_calls?: ToolCall[];
  /** tool 消息对应的调用 id */
  tool_call_id?: string;
}

/** 聊天消息（SQLite 持久化记录）。 */
export interface ChatEntry {
  id: number;
  /** user | assistant | tool */
  role: string;
  content: string;
  /** assistant 消息携带的工具调用列表（JSON 字符串） */
  tool_calls?: string | null;
  /** tool 消息对应的调用 id */
  tool_call_id?: string | null;
  created_at: number;
}

/** MCP 服务器配置（stdio 子进程或 Streamable HTTP）。 */
export interface McpServerConfig {
  id: string;
  name: string;
  /** stdio | http，缺省 stdio */
  transport?: "stdio" | "http" | null;
  /** stdio：启动命令，如 npx / node */
  command?: string | null;
  /** stdio：命令参数 */
  args?: string[];
  /** stdio：附加环境变量 */
  env?: Record<string, string>;
  /** http：端点 URL */
  url?: string | null;
  /** 启用后随应用启动自动连接，缺省 true */
  enabled?: boolean | null;
}

/** MCP 工具描述（Rust mcp_cmds 返回）。 */
export interface McpToolDto {
  name: string;
  title?: string | null;
  description?: string | null;
  /** 工具入参 JSON Schema */
  input_schema: unknown;
}

/** MCP 工具调用结果（Rust 聚合文本内容）。 */
export interface McpToolResult {
  ok: boolean;
  text: string;
  structured?: unknown | null;
}

/** 文档转换结果（document_convert 命令返回）。 */
export interface DocumentConvertResult {
  /** 转换出的 Markdown；needs_ocr 时为空串 */
  markdown: string;
  /** true：扫描版 PDF（无文本层），需要 OCR 才能提取内容 */
  needs_ocr: boolean;
  /** needs_ocr 时：无文本层的页码（1 起） */
  ocr_pages: number[];
  /** needs_ocr 时：PDF 总页数 */
  page_count: number;
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
  /** 收起面板时在精灵下方显示时钟，缺省 true */
  pet_clock?: boolean | null;
  /** MCP 服务器配置列表 */
  mcp_servers?: McpServerConfig[] | null;
  /** 悬浮监控条是否显示（主窗口启动时据此自动恢复） */
  monitor_bar_open?: boolean | null;
  /** 悬浮监控条鼠标穿透 */
  monitor_bar_click_through?: boolean | null;
  /** 悬浮监控条分区开关：CPU / 内存 / 网速，缺省均显示 */
  monitor_bar_cpu?: boolean | null;
  monitor_bar_mem?: boolean | null;
  monitor_bar_net?: boolean | null;
  /** 菜单栏监控文字（macOS 托盘标题）是否显示 */
  monitor_menubar_open?: boolean | null;
  /** 菜单栏监控文字显示期间是否保留桌宠托盘图标，缺省保留 */
  monitor_menubar_show_icon?: boolean | null;
  /** 监控刷新间隔（毫秒，1000/2000/5000）：工具页、悬浮条、菜单栏共用 */
  monitor_refresh_ms?: number | null;
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

/* ---------- 图片压缩（Rust 原生引擎，参考 PicLite） ---------- */

/** 水印参数 */
export interface CompressWatermarkOptions {
  /** text = 文字水印，image = 图片水印，blind = 盲水印（简化实现） */
  kind: "text" | "image" | "blind";
  /** 水印文字 / 盲水印标识内容 */
  text: string;
  fontSize: number;
  /** #RRGGBB */
  color: string;
  /** 0-100 */
  opacity: number;
  /** 顺时针旋转角度（度） */
  rotation: number;
  /** true = 铺满，false = 单枚 */
  tiled: boolean;
  /** 铺满密度 5-95，越大越密 */
  density: number;
  /** 单枚位置：center / top-left / top-right / bottom-left / bottom-right */
  position: string;
  /** 图片水印的 dataURL */
  imageDataUrl: string;
  /** 图片水印宽度相对宿主图宽的百分比 */
  imageScale: number;
  /** 盲水印强度 1-100 */
  strength: number;
}

/** 图片压缩参数 */
export interface CompressImageParams {
  /** 编码质量 1-100（PNG ≥90 无损，<90 调色板有损） */
  quality: number;
  /** 长边上限，0 = 不限 */
  maxDim: number;
  /** auto = 智能择优（多格式竞速取最小），keep = 保持原格式 */
  format: "auto" | "keep" | "jpeg" | "png" | "webp";
  /** 目标体积（KB），0 = 关闭 */
  targetKb: number;
  watermark?: CompressWatermarkOptions;
}

/** 图片压缩结果 */
export interface CompressImageResult {
  /** 压缩结果 dataURL（单图预览时返回） */
  dataUrl: string | null;
  /** 批量写出时的落盘路径 */
  outPath: string | null;
  originalSize: number;
  size: number;
  width: number;
  height: number;
  /** 实际输出格式名：jpeg / png / webp / gif */
  format: string;
  ext: string;
  /** 未达节省阈值，原样保留 */
  kept: boolean;
  animated: boolean;
}

/* ---------- 资料库（独立窗口，索引存 SQLite） ---------- */

/** 图库目录（监控的扫描根目录；全盘扫描为特殊条目）。字段 snake_case 与 Rust 契约一致 */
export interface ImageDirInfo {
  id: number;
  path: string;
  full_disk: boolean;
  added_at: number;
  last_scan_at: number;
  item_count: number;
}

/** 图库条目：照片或文档（doc=1） */
export interface ImageItemInfo {
  id: number;
  dir_id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  mtime: number;
  width: number;
  height: number;
  /** EXIF 拍摄时间（Unix 秒），无 EXIF 时为 mtime */
  taken_at: number;
  camera: string;
  is_fav: boolean;
  doc: boolean;
  /** 缩略图相对路径（image_thumbs/ 下），空 = 未生成 */
  thumb: string;
  hash: string;
}

export interface ImageTagInfo {
  id: number;
  name: string;
  color: string;
  item_count: number;
}

export interface ImageQueryDto {
  dirId?: number;
  favOnly?: boolean;
  docOnly?: boolean;
  tagId?: number;
  search?: string;
  limit?: number;
  offset?: number;
}

export interface ImagePage {
  items: ImageItemInfo[];
  hasMore: boolean;
  total: number;
}

export interface ImageDetail {
  item: ImageItemInfo;
  tags: ImageTagInfo[];
}

export interface ImageDupGroup {
  hash: string;
  items: ImageItemInfo[];
}

/** 扫描进度事件载荷（images://scan-progress） */
export interface ImageScanProgress {
  dirId: number;
  scanned: number;
  added: number;
  updated: number;
  removed: number;
  /** 缩略图并行生成进度 */
  thumbsDone: number;
  thumbsTotal: number;
  done: boolean;
}

/** 批量删除结果 */
export interface DupDeleteResult {
  deleted: number;
  /** 失败的 "路径: 原因" 列表(失败项保留在图库中) */
  failed: string[];
}

/* ---------- 文档管理（独立窗口，索引存 SQLite） ---------- */

export interface DocDirInfo {
  id: number;
  path: string;
  added_at: number;
  last_scan_at: number;
  item_count: number;
}

export interface DocItemInfo {
  id: number;
  dir_id: number;
  path: string;
  name: string;
  ext: string;
  size: number;
  mtime: number;
  added_at: number;
}

export interface DocQueryDto {
  dirId?: number;
  search?: string;
  /** mtime(默认)/ name / size / ext */
  sortBy?: string;
  sortOrder?: "asc" | "desc";
  limit?: number;
  offset?: number;
}

export interface DocPage {
  items: DocItemInfo[];
  hasMore: boolean;
  total: number;
}

/** 扫描进度事件载荷（docs://scan-progress） */
export interface DocsScanProgress {
  dirId: number;
  scanned: number;
  added: number;
  updated: number;
  removed: number;
  done: boolean;
}

/** 去重进度事件载荷（images://dedup-progress） */
export interface ImageDedupProgress {
  total: number;
  done: number;
  doneFlag: boolean;
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

/* ---------- 系统监控 ---------- */

/** CPU 采样：总体使用率与每核使用率（0-100）。 */
export interface CpuSample {
  usage: number;
  cores: number[];
}

/** 内存采样（字节）。 */
export interface MemorySample {
  used: number;
  total: number;
}

/** 网络采样：上下行速率（字节/秒）与开机以来累计流量（字节），已排除回环接口。 */
export interface NetworkSample {
  downRate: number;
  upRate: number;
  totalDown: number;
  totalUp: number;
}

/** 磁盘卷空间采样（字节）。 */
export interface VolumeSample {
  name: string;
  mount: string;
  total: number;
  available: number;
}

/** 一次系统采样快照。 */
export interface MonitorSample {
  cpu: CpuSample;
  memory: MemorySample;
  network: NetworkSample;
  volumes: VolumeSample[];
}
