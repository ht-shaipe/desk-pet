//! AI 编程用量统计命令（工具箱「AI 用量」工具 + 宠物徽标）：
//! 增量扫描本地 Claude Code / Codex CLI 日志，按时间范围聚合 token 与估算成本。

use deskpet_store::AiUsageGroupRow;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::ai_usage;
use crate::ai_usage_pricing;
use crate::state::AppState;

/// 前端聚合行：token 计数 + 估算成本（未知价格为 None，前端显示 "—"）。
#[derive(Serialize, Clone, Debug)]
pub struct AiUsageRow {
    pub key: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub cost_usd: Option<f64>,
}

/// 时间范围聚合结果（时间边界由前端按本地时区计算）。
#[derive(Serialize, Clone, Debug)]
pub struct AiUsageRangeResult {
    /// 库里是否有任何事件（区分「未检测到日志」与「该时段无用量」）
    pub has_any: bool,
    pub totals: AiUsageRow,
    pub by_tool: Vec<AiUsageRow>,
    pub by_model: Vec<AiUsageRow>,
    pub by_project: Vec<AiUsageRow>,
}

fn row(group: AiUsageGroupRow, cost: Option<f64>) -> AiUsageRow {
    AiUsageRow {
        key: group.key,
        input: group.input,
        output: group.output,
        cache_read: group.cache_read,
        cache_write: group.cache_write,
        cost_usd: cost,
    }
}

/// 汇总成本：任一组成模型缺价时返回 None（不冒充完整成本）。
fn sum_cost(parts: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut total = 0.0;
    for c in parts {
        total += c?;
    }
    Some((total * 1e6).round() / 1e6)
}

/// 增量扫描本地 AI 编程工具日志（Claude Code / Codex CLI），返回本次新增事件数。
/// 首扫历史大日志可能耗时，丢阻塞线程池避免卡 UI。
#[tauri::command]
pub async fn ai_usage_refresh(app: AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || ai_usage::scan_all(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// 查询时间范围 [from_ms, to_ms) 的用量聚合（含按模型估算成本）。
#[tauri::command]
pub fn ai_usage_query(app: AppHandle, from_ms: i64, to_ms: i64) -> Result<AiUsageRangeResult, String> {
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    let raw = store
        .ai_usage_query_range(from_ms, to_ms)
        .map_err(|e| e.to_string())?;
    let has_any = store.ai_usage_count_events().map_err(|e| e.to_string())? > 0;
    // 模型行：真实存储成本 + 价格表估算混合
    let by_model = raw
        .by_model
        .iter()
        .map(|m| {
            let cost = ai_usage_pricing::blended_cost(
                &m.key,
                m.stored_cost,
                m.est_input,
                m.est_output,
                m.est_cache_read,
                m.est_cache_write,
            );
            row(m.clone(), cost)
        })
        .collect::<Vec<_>>();
    // 工具行 / 总量：经工具×模型二维行求和，任一模型缺价则整体为 None
    let tool_cost = |tool: Option<&str>| -> Option<f64> {
        sum_cost(raw.by_tool_model.iter().filter_map(|r| {
            if tool.map_or(true, |t| r.tool == t) {
                Some(ai_usage_pricing::blended_cost(
                    &r.model,
                    r.stored_cost,
                    r.est_input,
                    r.est_output,
                    r.est_cache_read,
                    r.est_cache_write,
                ))
            } else {
                None
            }
        }))
    };
    let by_tool = raw
        .by_tool
        .iter()
        .map(|t| row(t.clone(), tool_cost(Some(&t.key))))
        .collect::<Vec<_>>();
    let totals = row(raw.totals.clone(), tool_cost(None));
    // 项目维度只展示 token，不算成本（项目内模型构成复杂，缺价会让整行变 "—"，意义不大）
    let by_project = raw
        .by_project
        .iter()
        .map(|p| row(p.clone(), None))
        .collect::<Vec<_>>();
    Ok(AiUsageRangeResult { has_any, totals, by_tool, by_model, by_project })
}

/// 启动后台增量扫描线程：每 5 分钟扫一次新日志，
/// 有新事件时广播 `ai_usage://updated`（徽标与工具页据此刷新）。
pub fn start_poll(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(300));
        match ai_usage::scan_all(&app) {
            Ok(n) if n > 0 => {
                let _ = app.emit("ai_usage://updated", n);
            }
            Ok(_) => {}
            Err(e) => eprintln!("[ai_usage] 后台扫描失败: {e}"),
        }
    });
}
