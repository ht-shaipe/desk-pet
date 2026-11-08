/**
 * 系统监控共享格式化（工具箱「系统监控」工具与悬浮监控条共用）。
 * 阈值变色参考 LiteMonitor 三色报警；格式化参考其数值智能格式化。
 */

/** 阈值：≥85 红、≥60 黄，其余正常。 */
export const WARN_PERCENT = 60;
export const DANGER_PERCENT = 85;

export type MonitorLevel = "ok" | "warn" | "danger";

/** 百分比 → 三色档位。 */
export function levelOf(percent: number): MonitorLevel {
  if (percent >= DANGER_PERCENT) return "danger";
  if (percent >= WARN_PERCENT) return "warn";
  return "ok";
}

/**
 * 数值智能格式化：单位自适应（1024 进制），≥100 取整、≥10 一位小数、其余两位。
 * 值非法或 ≤0 时显示 `0 {第二档单位}`（与常用显示口径一致）。
 */
export function fmtUnits(value: number, units: string[]): string {
  if (!Number.isFinite(value) || value <= 0) {
    return `0 ${units[1] ?? units[0]}`;
  }
  let v = value;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** 网络速率（字节/秒）。 */
export const fmtRate = (bytes: number) => fmtUnits(bytes, ["B/s", "KB/s", "MB/s", "GB/s"]);

/** 容量（字节）。 */
export const fmtBytes = (bytes: number) => fmtUnits(bytes, ["B", "KB", "MB", "GB", "TB"]);
