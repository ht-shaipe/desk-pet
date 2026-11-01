import { invoke } from "@tauri-apps/api/core";
import type { CleanupRemovalMode, CleanupStream, DiskUsage, DiskVolume, RemovalPlan } from "@/types";

/** 列出可扫描的真实卷（根分区 + 外接/网络卷）。 */
export const listDiskVolumes = () => invoke<DiskVolume[]>("disk_list_volumes");

/** 打开 macOS「完全磁盘访问权限」设置页。 */
export const openFullDiskAccess = () => invoke<void>("disk_open_full_disk_access");

/** 启动规则化扫描（分类/风险/默认勾选由后端规则表决定），返回 SSE 进度地址。 */
export const scanDiskCleanup = (root: string, logDays: number) =>
  invoke<CleanupStream>("disk_cleanup_scan", { root, logDays });

/** 查询指定目录所在磁盘卷的总容量与可用空间。 */
export const getDiskUsage = (root: string) =>
  invoke<DiskUsage>("disk_cleanup_usage", { root });

/** 计算删除计划：targets / covered / blocked 与同卷可释放字节。 */
export const planDiskCleanup = (scanJobId: string, paths: string[]) =>
  invoke<RemovalPlan>("disk_cleanup_plan", { scanJobId, paths });

/** 启动删除任务；paths 必须显式非空，mode 为 trash（默认）或 permanent。 */
export const deleteDiskCleanup = (
  scanJobId: string,
  paths: string[],
  mode: CleanupRemovalMode
) => invoke<CleanupStream>("disk_cleanup_delete", { scanJobId, paths, mode });

/** 请求停止扫描或删除。 */
export const cancelDiskCleanup = (jobId: string) =>
  invoke<void>("disk_cleanup_cancel", { jobId });
