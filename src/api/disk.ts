import { invoke } from "@tauri-apps/api/core";
import type {
  AnalyzerInsight,
  CleanupRemovalMode,
  CleanupStream,
  DiskLayout,
  DiskMarkInfo,
  DiskMetric,
  RemovalPlan,
} from "@/types";

/** 打开（或聚焦）磁盘分析窗口。 */
export const openDiskAnalyzer = () => invoke<boolean>("disk_analyzer_open");

/** 启动全树扫描（scanId 即返回的 jobId）。 */
export const scanDiskAnalyze = (root: string) =>
  invoke<CleanupStream>("disk_analyze_scan", { root });

/** 计算当前视图的 treemap tile。 */
export const layoutDiskAnalyze = (params: {
  scanId: string;
  path: string;
  width: number;
  height: number;
  maxDepth: number;
  metric: DiskMetric;
  filter?: string;
}) =>
  invoke<DiskLayout>("disk_analyze_layout", {
    scanId: params.scanId,
    path: params.path,
    width: params.width,
    height: params.height,
    maxDepth: params.maxDepth,
    metric: params.metric,
    filter: params.filter,
  });

/** 解析标记项在当前树中的大小（重扫后调用）。 */
export const marksDiskAnalyze = (scanId: string, paths: string[]) =>
  invoke<DiskMarkInfo[]>("disk_analyze_marks", { scanId, paths });

/** 计算标记的删除计划（复核清单）。 */
export const planDiskAnalyze = (scanId: string, paths: string[]) =>
  invoke<RemovalPlan>("disk_analyze_plan", { scanId, paths });

/** 删除标记项（trash/permanent），返回 SSE 进度地址。 */
export const deleteDiskAnalyze = (
  scanId: string,
  paths: string[],
  mode: CleanupRemovalMode
) => invoke<CleanupStream>("disk_analyze_delete", { scanId, paths, mode });

/** 分析侧栏"值得看看"：树上顶层可回收大目录。 */
export const insightsDiskAnalyze = (scanId: string) =>
  invoke<AnalyzerInsight[]>("disk_analyze_insights", { scanId });

/** 释放树缓存（窗口关闭时调用）。 */
export const closeDiskAnalyze = (scanId: string) =>
  invoke<void>("disk_analyze_close", { scanId });
