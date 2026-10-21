export interface AppSettings {
  pet_x: number | null;
  pet_y: number | null;
  always_on_top: boolean;
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
