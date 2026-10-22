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
  ai_messages?: ChatMessage[];
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
