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

export interface Note {
  id: string;
  content: string;
  color: string;
  updated_at: number;
}

export interface ClipEntry {
  id: string;
  text: string;
  updated_at: number;
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
