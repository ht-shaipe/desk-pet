import { invoke } from "@tauri-apps/api/core";
import type { AiUsageRangeResult } from "@/types";

/** 增量扫描本地 Claude Code / Codex CLI 日志，返回本次新增事件数 */
export const aiUsageRefresh = () => invoke<number>("ai_usage_refresh");

/** 查询时间范围 [fromMs, toMs) 的用量聚合（含按模型估算成本） */
export const aiUsageQuery = (fromMs: number, toMs: number) =>
  invoke<AiUsageRangeResult>("ai_usage_query", { fromMs, toMs });
