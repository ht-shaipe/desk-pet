import { invoke } from "@tauri-apps/api/core";
import type { MonitorSample } from "@/types";

/** 菜单栏/悬浮条共用的芯片分区开关 */
export interface MonitorChipSections {
  cpu: boolean;
  mem: boolean;
  net: boolean;
}

/** 采样一次系统运行状态（CPU / 内存 / 网络速率 / 磁盘卷） */
export const monitorSample = () => invoke<MonitorSample>("monitor_sample");

/** 切换悬浮监控条，返回切换后是否处于显示状态 */
export const monitorbarToggle = () => invoke<boolean>("monitorbar_toggle");

/** 更新菜单栏监控（macOS：采样绘制为芯片图显示在状态项上；sample 为 null 即清除） */
export const monitorMenubarSet = (
  sample: MonitorSample | null,
  showIcon: boolean,
  sections?: MonitorChipSections,
) => invoke<void>("monitor_menubar_set", { sample, showIcon, sections });
