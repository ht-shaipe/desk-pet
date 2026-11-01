import type { CleanupCategory } from "@/types";

/** 类别 → 基色（treemap tile 填充与图例共用）。 */
export const DISK_CATEGORY_COLORS: Record<CleanupCategory, string> = {
  code: "#4a8df0",
  agentScratch: "#9a6bff",
  toolchain: "#e0a24a",
  synced: "#43b0c9",
  git: "#e0645c",
  media: "#c06ad8",
  documents: "#5ea86b",
  // 缓存是出现频率最高的类别，用清爽的青绿而非灰蓝，与其他类别拉开
  cache: "#56b3a5",
  other: "#667085",
};

/** #rrggbb → rgba(r,g,b,alpha)。 */
export function withAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  const r = parseInt(value.slice(0, 2), 16);
  const g = parseInt(value.slice(2, 4), 16);
  const b = parseInt(value.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}
