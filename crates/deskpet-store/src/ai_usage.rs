//! AI 编程用量统计（Claude Code / Codex CLI 本地日志聚合）：
//! 事件表按 dedup_key 幂等入库，文件游标表支撑增量解析，查询按 token 维度聚合。
//! 成本估算不在此层——价格表易变，由 src-tauri 命令层按模型单价补算。

use rusqlite::params;

use crate::store::Store;
use crate::Result;

/// 单条 API 用量事件（扫描器产出、写入入参）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AiUsageEventInput {
    /// "claude_code" | "codex_cli"
    pub tool: String,
    /// Unix 毫秒（UTC）
    pub ts: i64,
    pub model: String,
    pub project: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// 随事件存储的真实成本（OpenCode 自带），None 表示未知、由价格表估算
    pub cost_usd: Option<f64>,
    /// 幂等去重键（Claude 为 "cc:<message.id>"，Codex 增量事件无天然键用 None）
    pub dedup_key: Option<String>,
}

/// 单个日志文件的增量解析游标（Codex 累计值做差需要跨次扫描保存 last_*）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct AiUsageFileState {
    pub path: String,
    pub tool: String,
    pub size: i64,
    pub mtime: i64,
    pub parsed_lines: i64,
    pub last_input: i64,
    pub last_output: i64,
    pub last_cache_read: i64,
    pub last_cache_write: i64,
}

/// 聚合行：key = 工具 / 模型 / 项目名。
/// 成本口径分两层：`stored_cost` 为随事件存储的真实成本之和（OpenCode），
/// `est_*` 为无存储成本事件的 token 子集（命令层按价格表估算）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AiUsageGroupRow {
    pub key: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub stored_cost: f64,
    pub est_input: i64,
    pub est_output: i64,
    pub est_cache_read: i64,
    pub est_cache_write: i64,
}

/// 工具 + 模型二维聚合行（成本按模型单价计算，需要知道每个工具下的模型构成）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AiUsageToolModelRow {
    pub tool: String,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub stored_cost: f64,
    pub est_input: i64,
    pub est_output: i64,
    pub est_cache_read: i64,
    pub est_cache_write: i64,
}

/// 时间范围聚合结果（原始 token，成本在命令层补算）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct AiUsageRangeRaw {
    pub totals: AiUsageGroupRow,
    pub by_tool: Vec<AiUsageGroupRow>,
    pub by_model: Vec<AiUsageGroupRow>,
    pub by_project: Vec<AiUsageGroupRow>,
    pub by_tool_model: Vec<AiUsageToolModelRow>,
}

/// 聚合 SELECT 列：全量 token、真实成本、估算子集 token（cost_usd IS NULL 的事件）。
const AGG_COLS: &str = "COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
     COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_write),0), \
     COALESCE(SUM(cost_usd),0), \
     COALESCE(SUM(CASE WHEN cost_usd IS NULL THEN input_tokens ELSE 0 END),0), \
     COALESCE(SUM(CASE WHEN cost_usd IS NULL THEN output_tokens ELSE 0 END),0), \
     COALESCE(SUM(CASE WHEN cost_usd IS NULL THEN cache_read ELSE 0 END),0), \
     COALESCE(SUM(CASE WHEN cost_usd IS NULL THEN cache_write ELSE 0 END),0)";

/// 行 -> 聚合行（列序同 AGG_COLS，起始索引 base）。
fn group_row<T>(row: &rusqlite::Row, base: usize) -> rusqlite::Result<T>
where
    T: FromAgg,
{
    T::from_row(row, base)
}

/// 聚合行构造 trait（key 在前与 tool/model 在前两种形态）。
trait FromAgg: Sized {
    fn from_row(row: &rusqlite::Row, base: usize) -> rusqlite::Result<Self>;
}

impl FromAgg for AiUsageGroupRow {
    fn from_row(row: &rusqlite::Row, base: usize) -> rusqlite::Result<Self> {
        Ok(AiUsageGroupRow {
            key: row.get(base)?,
            input: row.get(base + 1)?,
            output: row.get(base + 2)?,
            cache_read: row.get(base + 3)?,
            cache_write: row.get(base + 4)?,
            stored_cost: row.get(base + 5)?,
            est_input: row.get(base + 6)?,
            est_output: row.get(base + 7)?,
            est_cache_read: row.get(base + 8)?,
            est_cache_write: row.get(base + 9)?,
        })
    }
}

impl FromAgg for AiUsageToolModelRow {
    fn from_row(row: &rusqlite::Row, base: usize) -> rusqlite::Result<Self> {
        Ok(AiUsageToolModelRow {
            tool: row.get(base)?,
            model: row.get(base + 1)?,
            input: row.get(base + 2)?,
            output: row.get(base + 3)?,
            cache_read: row.get(base + 4)?,
            cache_write: row.get(base + 5)?,
            stored_cost: row.get(base + 6)?,
            est_input: row.get(base + 7)?,
            est_output: row.get(base + 8)?,
            est_cache_read: row.get(base + 9)?,
            est_cache_write: row.get(base + 10)?,
        })
    }
}

/// 按 key 分组聚合（ORDER BY 总 token 降序）。
fn group_by(
    conn: &rusqlite::Connection,
    from_ms: i64,
    to_ms: i64,
    key_expr: &str,
) -> Result<Vec<AiUsageGroupRow>> {
    let sql = format!(
        "SELECT {key_expr}, {AGG_COLS} FROM ai_usage_event \
         WHERE ts >= ?1 AND ts < ?2 GROUP BY {key_expr} \
         ORDER BY (SUM(input_tokens) + SUM(output_tokens) + SUM(cache_read) + SUM(cache_write)) DESC",
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from_ms, to_ms], |row| group_row(row, 0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

impl Store {
    /// 全部日志文件的解析游标。
    pub fn ai_usage_list_file_states(&self) -> Result<Vec<AiUsageFileState>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, tool, size, mtime, parsed_lines, last_input, last_output, \
             last_cache_read, last_cache_write FROM ai_usage_file_state",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AiUsageFileState {
                path: row.get(0)?,
                tool: row.get(1)?,
                size: row.get(2)?,
                mtime: row.get(3)?,
                parsed_lines: row.get(4)?,
                last_input: row.get(5)?,
                last_output: row.get(6)?,
                last_cache_read: row.get(7)?,
                last_cache_write: row.get(8)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 单文件扫描结果原子落库：事件 INSERT OR IGNORE（dedup_key 命中即静默跳过）
    /// + 游标 upsert，同一事务避免崩溃后重复计数。返回真实新增事件数。
    pub fn ai_usage_apply_file_scan(
        &self,
        events: &[AiUsageEventInput],
        state: &AiUsageFileState,
    ) -> Result<u64> {
        let tx = self.conn.unchecked_transaction()?;
        let mut inserted = 0u64;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO ai_usage_event \
                 (tool, ts, model, project, input_tokens, output_tokens, cache_read, cache_write, cost_usd, dedup_key) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for e in events {
                inserted += stmt.execute(params![
                    e.tool,
                    e.ts,
                    e.model,
                    e.project,
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_read,
                    e.cache_write,
                    e.cost_usd,
                    e.dedup_key,
                ])? as u64;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO ai_usage_file_state \
             (path, tool, size, mtime, parsed_lines, last_input, last_output, last_cache_read, last_cache_write) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                state.path,
                state.tool,
                state.size,
                state.mtime,
                state.parsed_lines,
                state.last_input,
                state.last_output,
                state.last_cache_read,
                state.last_cache_write,
            ],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    /// 事件总数（区分「未检测到日志」与「该时段无用量」的空态）。
    pub fn ai_usage_count_events(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM ai_usage_event", [], |r| r.get(0))?;
        Ok(n)
    }

    /// 时间范围聚合（ts ∈ [from_ms, to_ms)）。时间边界由前端按本地时区计算后传入。
    pub fn ai_usage_query_range(&self, from_ms: i64, to_ms: i64) -> Result<AiUsageRangeRaw> {
        let totals = {
            let sql =
                format!("SELECT {AGG_COLS} FROM ai_usage_event WHERE ts >= ?1 AND ts < ?2");
            let mut stmt = self.conn.prepare(&sql)?;
            stmt.query_row(params![from_ms, to_ms], |row| {
                Ok(AiUsageGroupRow {
                    key: String::new(),
                    input: row.get(0)?,
                    output: row.get(1)?,
                    cache_read: row.get(2)?,
                    cache_write: row.get(3)?,
                    stored_cost: row.get(4)?,
                    est_input: row.get(5)?,
                    est_output: row.get(6)?,
                    est_cache_read: row.get(7)?,
                    est_cache_write: row.get(8)?,
                })
            })?
        };
        let by_tool = group_by(&self.conn, from_ms, to_ms, "tool")?;
        let by_model = group_by(&self.conn, from_ms, to_ms, "model")?;
        let by_project = group_by(&self.conn, from_ms, to_ms, "project")?;
        // 工具 × 模型二维行：命令层据此计算每个工具的成本
        let by_tool_model = {
            let sql = format!(
                "SELECT tool, model, {AGG_COLS} FROM ai_usage_event \
                 WHERE ts >= ?1 AND ts < ?2 GROUP BY tool, model"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params![from_ms, to_ms], |row| group_row(row, 0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        Ok(AiUsageRangeRaw {
            totals,
            by_tool,
            by_model,
            by_project,
            by_tool_model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(dedup: Option<&str>, ts: i64, model: &str, input: i64, output: i64) -> AiUsageEventInput {
        AiUsageEventInput {
            tool: "claude_code".into(),
            ts,
            model: model.into(),
            project: "proj".into(),
            input_tokens: input,
            output_tokens: output,
            cache_read: 10,
            cache_write: 20,
            cost_usd: None,
            dedup_key: dedup_map(dedup),
        }
    }

    fn dedup_map(dedup: Option<&str>) -> Option<String> {
        dedup.map(str::to_string)
    }

    fn file_state(path: &str, parsed_lines: i64) -> AiUsageFileState {
        AiUsageFileState {
            path: path.into(),
            tool: "claude_code".into(),
            size: 100,
            mtime: 200,
            parsed_lines,
            ..Default::default()
        }
    }

    #[test]
    fn file_scan_dedups_and_counts_inserted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let e1 = ev(Some("cc:m1"), 1000, "claude-sonnet-4-5", 100, 50);
        // 同 dedup_key 的重复事件（同 message.id 多行）应被忽略
        let e1_dup = ev(Some("cc:m1"), 1000, "claude-sonnet-4-5", 100, 50);
        let e2 = ev(Some("cc:m2"), 2000, "claude-opus-4-5", 10, 5);
        let state = file_state("/tmp/a.jsonl", 2);
        let n = store
            .ai_usage_apply_file_scan(&[e1.clone(), e1_dup, e2.clone()], &state)
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(store.ai_usage_count_events().unwrap(), 2);

        // 游标已持久化
        let states = store.ai_usage_list_file_states().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].parsed_lines, 2);
        assert_eq!(states[0].path, "/tmp/a.jsonl");

        // 重复提交同批事件：0 新增（dedup_key 命中），游标更新
        let state2 = file_state("/tmp/a.jsonl", 3);
        let n = store
            .ai_usage_apply_file_scan(&[e1, e2], &state2)
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(store.ai_usage_count_events().unwrap(), 2);
        assert_eq!(store.ai_usage_list_file_states().unwrap()[0].parsed_lines, 3);
    }

    #[test]
    fn null_dedup_keys_all_inserted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        // Codex 增量事件 dedup_key 为 None：SQLite UNIQUE 允许多个 NULL，全部插入
        let e1 = ev(None, 1000, "gpt-5", 7, 3);
        let e2 = ev(None, 2000, "gpt-5", 8, 4);
        let n = store
            .ai_usage_apply_file_scan(&[e1, e2], &file_state("/tmp/b.jsonl", 2))
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(store.ai_usage_count_events().unwrap(), 2);
    }

    #[test]
    fn query_range_groups_and_totals() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut codex = ev(Some("cc:x"), 500, "gpt-5", 1000, 2000);
        codex.tool = "codex_cli".into();
        codex.project = "desk-pet".into();
        // 模拟 OpenCode 式真实成本事件：token 全部计入 stored 口径、est 子集为零
        codex.input_tokens = 300;
        codex.output_tokens = 400;
        codex.cache_read = 0;
        codex.cache_write = 0;
        codex.cost_usd = Some(0.25);
        let claude = ev(Some("cc:y"), 1500, "claude-sonnet-4-5", 100, 50);
        let old = ev(Some("cc:z"), 100, "claude-sonnet-4-5", 999, 999);
        let state = file_state("/tmp/c.jsonl", 3);
        store
            .ai_usage_apply_file_scan(&[codex, claude, old], &state)
            .unwrap();

        // 范围 [200, 2000)：不含 ts=100 的 old
        let raw = store.ai_usage_query_range(200, 2000).unwrap();
        assert_eq!(raw.totals.input, 400);
        assert_eq!(raw.totals.output, 450);
        assert_eq!(raw.totals.cache_read, 10);
        assert_eq!(raw.totals.cache_write, 20);
        // stored 成本只含 OpenCode 式事件；est 子集只含无成本事件（claude）的 token
        assert_eq!(raw.totals.stored_cost, 0.25);
        assert_eq!(raw.totals.est_input, 100);
        assert_eq!(raw.totals.est_output, 50);
        assert_eq!(raw.totals.est_cache_read, 10);
        assert_eq!(raw.totals.est_cache_write, 20);
        // by_tool：700 总 token 的 codex 在前
        assert_eq!(raw.by_tool.len(), 2);
        assert_eq!(raw.by_tool[0].key, "codex_cli");
        assert_eq!(raw.by_tool[0].stored_cost, 0.25);
        assert_eq!(raw.by_tool[0].est_input, 0);
        assert_eq!(raw.by_tool[1].key, "claude_code");
        assert_eq!(raw.by_tool[1].stored_cost, 0.0);
        assert_eq!(raw.by_model.len(), 2);
        assert_eq!(raw.by_model[0].key, "gpt-5");
        assert_eq!(raw.by_project.len(), 2);
        // 工具 × 模型二维行
        assert_eq!(raw.by_tool_model.len(), 2);
        assert!(raw
            .by_tool_model
            .iter()
            .any(|r| r.tool == "codex_cli" && r.model == "gpt-5" && r.stored_cost == 0.25));

        // 空范围
        let raw = store.ai_usage_query_range(100_000, 200_000).unwrap();
        assert_eq!(raw.totals.input, 0);
        assert_eq!(raw.totals.stored_cost, 0.0);
        assert!(raw.by_tool.is_empty());
        assert_eq!(store.ai_usage_count_events().unwrap(), 3);
    }
}
