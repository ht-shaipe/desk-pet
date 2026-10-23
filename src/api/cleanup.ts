import { invoke } from "@tauri-apps/api/core";
import type { CleanupKind, CleanupStream, DiskUsage } from "@/types";

/** 启动目录扫描，返回 SSE 进度地址。 */
export const scanDiskCleanup = (root: string, targets: CleanupKind[], logDays: number) =>
  invoke<CleanupStream>("disk_cleanup_scan", { root, targets, logDays });

/** 查询指定目录所在磁盘卷的总容量与可用空间。 */
export const getDiskUsage = (root: string) =>
  invoke<DiskUsage>("disk_cleanup_usage", { root });

/** 按上次扫描结果启动删除任务，ids 为勾选的候选项，缺省删除全部。 */
export const deleteDiskCleanup = (scanJobId: string, ids?: string[]) =>
  invoke<CleanupStream>("disk_cleanup_delete", { scanJobId, ids });

/** 请求停止扫描或删除。 */
export const cancelDiskCleanup = (jobId: string) =>
  invoke<void>("disk_cleanup_cancel", { jobId });
