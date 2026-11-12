//! AI 编程用量统计：增量解析 Claude Code / Codex CLI 本地 JSONL 日志。
//! 参考 cclank/tokei（知度，MIT）的解析思路用 Rust 重实现：
//! - Claude Code：`~/.claude/projects/<proj>/<session>.jsonl`，assistant 行
//!   `message.usage` 记录单次 API 用量，按 message.id 去重、跳过 API 错误行
//! - Codex CLI：`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`，token_count 事件；
//!   新版 `info.last_token_usage` 为单轮用量直取，旧版直接字段为会话累计值，
//!   与文件游标 last_* 做差
//! 扫描为增量式：文件 size/mtime 未变即跳过，新行解析后与游标同事务落库。
//! 目录支持环境变量覆盖（DESKPET_AI_USAGE_CLAUDE_DIR / _CODEX_DIR）便于测试。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use deskpet_store::{AiUsageEventInput, AiUsageFileState};
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};

use crate::state::AppState;

/// 工具标识（ai_usage_event.tool 字段值）
pub const TOOL_CLAUDE: &str = "claude_code";
pub const TOOL_CODEX: &str = "codex_cli";
pub const TOOL_OPENCODE: &str = "opencode";

/* ---------------- ISO8601 时间解析（不引入 chrono） ---------------- */

/// ISO8601 UTC 时间字符串 -> Unix 毫秒。
/// 支持 `2026-11-12T08:30:00Z` / 带任意位小数秒 / `±HH:MM` 偏移（按 UTC 归一）。
pub(crate) fn parse_iso8601_utc_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<i64> {
        let mut v: i64 = 0;
        for &c in b.get(r)? {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + (c - b'0') as i64;
        }
        Some(v)
    };
    if b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (h, mi, sec) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    // 小数秒：截断到毫秒，多余位数忽略
    let mut rest = &b[19..];
    let mut frac_ms: i64 = 0;
    if rest.first() == Some(&b'.') {
        let mut i = 1;
        let mut count = 0u32;
        let mut val: i64 = 0;
        while i < rest.len() && rest[i].is_ascii_digit() && count < 3 {
            val = val * 10 + (rest[i] - b'0') as i64;
            i += 1;
            count += 1;
        }
        if count == 0 {
            return None;
        }
        // 越过剩余小数位（亚毫秒精度忽略）
        while i < rest.len() && rest[i].is_ascii_digit() {
            i += 1;
        }
        frac_ms = val * 10i64.pow(3 - count);
        rest = &rest[i..];
    }
    let offset_secs = match rest.first() {
        Some(&b'Z') | Some(&b'z') => {
            rest = &rest[1..];
            0
        }
        Some(&b'+') | Some(&b'-') => {
            if rest.len() < 6 || rest[3] != b':' {
                return None;
            }
            let oh: i64 = rest[1..3].iter().try_fold(0i64, |a, &c| {
                c.is_ascii_digit()
                    .then(|| a * 10 + (c - b'0') as i64)
                    .ok_or(())
            }).ok()?;
            let om: i64 = rest[4..6].iter().try_fold(0i64, |a, &c| {
                c.is_ascii_digit()
                    .then(|| a * 10 + (c - b'0') as i64)
                    .ok_or(())
            }).ok()?;
            let total = oh * 3600 + om * 60;
            let sign = if rest[0] == b'+' { 1 } else { -1 };
            rest = &rest[6..];
            sign * total
        }
        _ => return None,
    };
    // 尾部只允许空白
    if rest.iter().any(|c| !c.is_ascii_whitespace()) {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    let secs = days * 86400 + h * 3600 + mi * 60 + sec - offset_secs;
    Some(secs * 1000 + frac_ms)
}

/// 年月日 -> 自 1970-01-01 的天数（Howard Hinnant days_from_civil，civil_from_days 的逆）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/* ---------------- Claude Code 解析 ---------------- */

/// 解析 Claude Code JSONL 单行（assistant 行）。非用量行 / 缺字段行返回 None。
pub(crate) fn parse_claude_line(line: &str, project: &str) -> Option<AiUsageEventInput> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    // API 错误重试行带相同 message.id，计入会虚增用量
    if v.get("isApiErrorMessage").and_then(|b| b.as_bool()) == Some(true) {
        return None;
    }
    let message = v.get("message")?;
    let usage = message.get("usage")?;
    let id = message.get("id").and_then(|i| i.as_str())?;
    let ts = parse_iso8601_utc_ms(v.get("timestamp").and_then(|t| t.as_str())?)?;
    let get = |k: &str| usage.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
    let (input, output) = (get("input_tokens"), get("output_tokens"));
    let (cache_read, cache_write) = (
        get("cache_read_input_tokens"),
        get("cache_creation_input_tokens"),
    );
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    Some(AiUsageEventInput {
        tool: TOOL_CLAUDE.into(),
        ts,
        model: message
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .into(),
        project: project.into(),
        input_tokens: input,
        output_tokens: output,
        cache_read,
        cache_write,
        cost_usd: None,
        dedup_key: Some(format!("cc:{id}")),
    })
}

/* ---------------- Codex CLI 解析 ---------------- */

/// Codex 单文件解析上下文（session_meta / turn_context 跨行生效）。
pub(crate) struct CodexCtx {
    pub project: String,
    pub model: String,
    /// 会话累计基线（旧版 token_count 直接字段做差用）：(input, output, cache_read, cache_write)
    pub last: (i64, i64, i64, i64),
}

impl CodexCtx {
    pub fn new(last: (i64, i64, i64, i64)) -> Self {
        Self { project: String::new(), model: String::new(), last }
    }
}

fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

/// 解析 Codex rollout JSONL 单行，产出用量事件。
/// session_meta / turn_context 更新上下文后返回 None；token_count 行按两种版本形态取数。
pub(crate) fn parse_codex_line(line: &str, ctx: &mut CodexCtx) -> Option<AiUsageEventInput> {
    let v: Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload")?;
    let line_type = v.get("type").and_then(|t| t.as_str())?;
    match line_type {
        "session_meta" => {
            if let Some(cwd) = payload.get("cwd").and_then(|c| c.as_str()) {
                ctx.project = basename(cwd);
            }
            None
        }
        "turn_context" => {
            if let Some(m) = payload.get("model").and_then(|m| m.as_str()) {
                ctx.model = m.to_string();
            }
            None
        }
        "event_msg" => {
            if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                return None;
            }
            let ts = parse_iso8601_utc_ms(v.get("timestamp").and_then(|t| t.as_str())?)?;
            // 新版：info.last_token_usage 为本轮实际用量（output_tokens 已含推理，不重复累计）
            if let Some(last_usage) = payload.get("info").and_then(|i| i.get("last_token_usage")) {
                let get = |k: &str| last_usage.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
                let (input, output, cache_read) = (
                    get("input_tokens"),
                    get("output_tokens"),
                    get("cached_input_tokens"),
                );
                if input == 0 && output == 0 && cache_read == 0 {
                    return None;
                }
                return Some(AiUsageEventInput {
                    tool: TOOL_CODEX.into(),
                    ts,
                    model: ctx.model.clone(),
                    project: ctx.project.clone(),
                    input_tokens: input,
                    output_tokens: output,
                    cache_read,
                    cache_write: 0,
                    cost_usd: None,
                    dedup_key: None,
                });
            }
            // 旧版：直接字段为会话累计值，与基线做差；负差值视为基线失效，仅重置基线
            let get = |k: &str| payload.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
            let (input, output, cache_read) =
                (get("input_tokens"), get("output_tokens"), get("cached_input_tokens"));
            let (li, lo, lcr, _) = ctx.last;
            ctx.last = (input, output, cache_read, 0);
            let (di, do_, dcr) = (input - li, output - lo, cache_read - lcr);
            if di <= 0 && do_ <= 0 && dcr <= 0 {
                return None;
            }
            Some(AiUsageEventInput {
                tool: TOOL_CODEX.into(),
                ts,
                model: ctx.model.clone(),
                project: ctx.project.clone(),
                input_tokens: di.max(0),
                output_tokens: do_.max(0),
                cache_read: dcr.max(0),
                cache_write: 0,
                cost_usd: None,
                dedup_key: None,
            })
        }
        _ => None,
    }
}

/* ---------------- OpenCode 解析 ---------------- */

/// 解析 opencode.db message.data（assistant 行）为用量事件。
/// data 含 tokens{input,output,reasoning,cache{read,write}}、cost（真实成本）、
/// modelID、path.cwd、time.created；time 缺失时回退行级 time_created。
pub(crate) fn parse_opencode_message(
    id: &str,
    time_created: i64,
    data: &str,
) -> Option<AiUsageEventInput> {
    let v: Value = serde_json::from_str(data).ok()?;
    if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return None;
    }
    let tokens = v.get("tokens")?;
    let get = |k: &str| tokens.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
    // output 已含 reasoning（与 Claude/Codex 口径一致，reasoning 仅作信息字段不重复累计）
    let (input, output) = (get("input"), get("output"));
    let (cache_read, cache_write) = (
        tokens
            .get("cache")
            .and_then(|c| c.get("read"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0),
        tokens
            .get("cache")
            .and_then(|c| c.get("write"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0),
    );
    let cost = v.get("cost").and_then(|c| c.as_f64());
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && cost.unwrap_or(0.0) == 0.0
    {
        return None;
    }
    let ts = v
        .pointer("/time/created")
        .and_then(|t| t.as_i64())
        .unwrap_or(time_created);
    Some(AiUsageEventInput {
        tool: TOOL_OPENCODE.into(),
        ts,
        model: v
            .get("modelID")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .into(),
        project: v
            .pointer("/path/cwd")
            .and_then(|p| p.as_str())
            .map(basename)
            .unwrap_or_default(),
        input_tokens: input,
        output_tokens: output,
        cache_read,
        cache_write,
        cost_usd: cost,
        dedup_key: Some(format!("oc:{id}")),
    })
}

/* ---------------- 增量扫描 ---------------- */

/// 日志根目录：优先环境变量（测试/适配用），缺省 $HOME 下标准路径；目录不存在返回 None。
fn env_dir(var: &str, default: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(var) {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let p = home.join(default);
    p.is_dir().then_some(p)
}

/// Claude Code 会话日志根目录。
fn claude_root() -> Option<PathBuf> {
    env_dir("DESKPET_AI_USAGE_CLAUDE_DIR", ".claude/projects")
}

/// Codex CLI 会话日志根目录。
fn codex_root() -> Option<PathBuf> {
    env_dir("DESKPET_AI_USAGE_CODEX_DIR", ".codex/sessions")
}

/// OpenCode 本地数据库路径（XDG 数据目录，macOS 同样是 ~/.local/share）。
fn opencode_db_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DESKPET_AI_USAGE_OPENCODE_DB") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let p = home.join(".local/share/opencode/opencode.db");
    p.is_file().then_some(p)
}

fn mtime_ms(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 增量扫描各工具的本地日志（无数据时为空操作），返回新增事件数。
/// 单文件解析在锁外进行、落库时短锁，避免首扫大日志阻塞其他命令。
pub fn scan_all<R: Runtime>(app: &AppHandle<R>) -> Result<u64, String> {
    let mut total = 0u64;
    for (tool, root) in [(TOOL_CLAUDE, claude_root()), (TOOL_CODEX, codex_root())] {
        let Some(root) = root else { continue };
        total += scan_tool(app, tool, &root)?;
    }
    if let Some(db) = opencode_db_path() {
        total += scan_opencode_at(app, &db)?;
    }
    Ok(total)
}

/// 扫描 OpenCode 本地库（增量：游标存于 ai_usage_file_state.last_input，语义为已读到的最大 time_created）。
fn scan_opencode_at<R: Runtime>(app: &AppHandle<R>, db_path: &Path) -> Result<u64, String> {
    let path_str = db_path.to_string_lossy().to_string();
    // 游标：path + tool 定位，last_input 复用为 since_ms
    let since: i64 = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .ai_usage_list_file_states()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|s| s.path == path_str && s.tool == TOOL_OPENCODE)
            .map(|s| s.last_input.max(0))
            .unwrap_or(0)
    };
    // 打不开（未使用/锁冲突）按无新数据处理
    let Ok(rows) = deskpet_store::read_opencode_messages(db_path, since) else {
        return Ok(0);
    };
    let mut events: Vec<AiUsageEventInput> = Vec::new();
    let mut max_ts = since;
    for r in &rows {
        max_ts = max_ts.max(r.time_created);
        if let Some(e) = parse_opencode_message(&r.id, r.time_created, &r.data) {
            events.push(e);
        }
    }
    // 无新消息也要推进游标吗：max_ts == since 且 events 空时直接返回 0
    if events.is_empty() && max_ts == since {
        return Ok(0);
    }
    let new_state = AiUsageFileState {
        path: path_str,
        tool: TOOL_OPENCODE.into(),
        size: 0,
        mtime: 0,
        parsed_lines: 0,
        last_input: max_ts,
        last_output: 0,
        last_cache_read: 0,
        last_cache_write: 0,
    };
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    store
        .ai_usage_apply_file_scan(&events, &new_state)
        .map_err(|e| e.to_string())
}

fn scan_tool<R: Runtime>(app: &AppHandle<R>, tool: &str, root: &Path) -> Result<u64, String> {
    let prev: HashMap<String, AiUsageFileState> = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .ai_usage_list_file_states()
            .map_err(|e| e.to_string())?
    }
    .into_iter()
    .filter(|s| s.tool == tool)
    .map(|s| (s.path.clone(), s))
    .collect();

    let mut inserted = 0u64;
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(md) = std::fs::metadata(path) else { continue };
        let size = md.len() as i64;
        let mtime = mtime_ms(&md);
        let path_str = path.to_string_lossy().to_string();
        let Some(prev_state) = prev.get(&path_str) else {
            inserted += scan_one_file(app, tool, path, &path_str, size, mtime, None)?;
            continue;
        };
        if prev_state.size == size && prev_state.mtime == mtime {
            continue;
        }
        // size 变小视为轮转/截断：从头重解析（已入库事件靠 dedup_key 幂等）
        let resume = (size >= prev_state.size).then_some(prev_state);
        inserted += scan_one_file(app, tool, path, &path_str, size, mtime, resume)?;
    }
    Ok(inserted)
}

/// 解析单个日志文件的新增行并连同新游标原子落库。
/// `prev` 为已有游标（增量续扫），None 表示从头解析。
fn scan_one_file<R: Runtime>(
    app: &AppHandle<R>,
    tool: &str,
    path: &Path,
    path_str: &str,
    size: i64,
    mtime: i64,
    prev: Option<&AiUsageFileState>,
) -> Result<u64, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(0);
    };
    let skip = prev.map(|p| p.parsed_lines.max(0) as usize).unwrap_or(0);
    let mut new_state = AiUsageFileState {
        path: path_str.to_string(),
        tool: tool.to_string(),
        size,
        mtime,
        parsed_lines: skip as i64,
        last_input: prev.map(|p| p.last_input).unwrap_or(0),
        last_output: prev.map(|p| p.last_output).unwrap_or(0),
        last_cache_read: prev.map(|p| p.last_cache_read).unwrap_or(0),
        last_cache_write: prev.map(|p| p.last_cache_write).unwrap_or(0),
    };
    let mut events: Vec<AiUsageEventInput> = Vec::new();
    match tool {
        TOOL_CLAUDE => {
            let project = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            for (i, line) in content.lines().enumerate() {
                if i < skip {
                    continue;
                }
                new_state.parsed_lines = (i + 1) as i64;
                if let Some(e) = parse_claude_line(line, &project) {
                    events.push(e);
                }
            }
        }
        TOOL_CODEX => {
            let mut ctx = CodexCtx::new((
                new_state.last_input,
                new_state.last_output,
                new_state.last_cache_read,
                new_state.last_cache_write,
            ));
            for (i, line) in content.lines().enumerate() {
                if i < skip {
                    continue;
                }
                new_state.parsed_lines = (i + 1) as i64;
                if let Some(mut e) = parse_codex_line(line, &mut ctx) {
                    // 无天然去重键：按文件路径 + 原始行内容哈希，重扫幂等
                    if e.dedup_key.is_none() {
                        let mut hasher = blake3::Hasher::new();
                        hasher.update(path_str.as_bytes());
                        hasher.update(&[0]);
                        hasher.update(line.as_bytes());
                        e.dedup_key = Some(format!("cx:{}", &hasher.finalize().to_hex()[..16]));
                    }
                    events.push(e);
                }
            }
            new_state.last_input = ctx.last.0;
            new_state.last_output = ctx.last.1;
            new_state.last_cache_read = ctx.last.2;
            new_state.last_cache_write = ctx.last.3;
        }
        _ => return Ok(0),
    }
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    store
        .ai_usage_apply_file_scan(&events, &new_state)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /* -------- ISO8601 解析 -------- */

    #[test]
    fn iso_parse_epoch_and_offsets() {
        assert_eq!(parse_iso8601_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso8601_utc_ms("2026-11-12T00:00:00Z"), Some(1_794_441_600_000));
        // 毫秒小数
        assert_eq!(
            parse_iso8601_utc_ms("2026-11-12T08:30:00.123Z"),
            Some(1_794_472_200_123)
        );
        // 微秒截断到毫秒
        assert_eq!(
            parse_iso8601_utc_ms("2026-11-12T08:30:00.123456Z"),
            parse_iso8601_utc_ms("2026-11-12T08:30:00.123Z")
        );
        // 无小数
        assert_eq!(
            parse_iso8601_utc_ms("2026-11-12T08:30:00Z"),
            Some(1_794_472_200_000)
        );
        // +08:00 偏移归一为 UTC
        assert_eq!(
            parse_iso8601_utc_ms("2026-11-12T16:30:00+08:00"),
            parse_iso8601_utc_ms("2026-11-12T08:30:00Z")
        );
        // 负偏移
        assert_eq!(
            parse_iso8601_utc_ms("2026-11-12T03:30:00-05:00"),
            parse_iso8601_utc_ms("2026-11-12T08:30:00Z")
        );
        // 非法输入
        assert_eq!(parse_iso8601_utc_ms(""), None);
        assert_eq!(parse_iso8601_utc_ms("2026-11-12"), None);
        assert_eq!(parse_iso8601_utc_ms("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso8601_utc_ms("not-a-time"), None);
    }

    /* -------- Claude Code 行解析 -------- */

    #[test]
    fn claude_line_parses_usage_and_dedups() {
        let line = r#"{"type":"assistant","timestamp":"2026-11-12T08:30:00.123Z","requestId":"req_1","isSidechain":false,"isApiErrorMessage":false,"message":{"id":"msg_01","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":200,"output_tokens":80}}}"#;
        let e = parse_claude_line(line, "Users-x-proj").unwrap();
        assert_eq!(e.tool, TOOL_CLAUDE);
        assert_eq!(e.ts, 1_794_472_200_123);
        assert_eq!(e.model, "claude-sonnet-4-5-20250929");
        assert_eq!(e.project, "Users-x-proj");
        assert_eq!(e.input_tokens, 100);
        assert_eq!(e.output_tokens, 80);
        assert_eq!(e.cache_read, 200);
        assert_eq!(e.cache_write, 50);
        assert_eq!(e.dedup_key.as_deref(), Some("cc:msg_01"));

        // API 错误行跳过
        let err_line = r#"{"type":"assistant","isApiErrorMessage":true,"timestamp":"2026-11-12T08:30:00Z","message":{"id":"msg_02","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        assert!(parse_claude_line(err_line, "p").is_none());
        // user 行跳过
        let user_line = r#"{"type":"user","timestamp":"2026-11-12T08:30:00Z","message":{"usage":{"input_tokens":1}}}"#;
        assert!(parse_claude_line(user_line, "p").is_none());
        // 缺 usage / 缺 id 跳过
        assert!(parse_claude_line(r#"{"type":"assistant"}"#, "p").is_none());
        assert!(parse_claude_line(
            r#"{"type":"assistant","message":{"usage":{"input_tokens":1}}}"#,
            "p"
        )
        .is_none());
        // 全零 usage 跳过
        assert!(parse_claude_line(
            r#"{"type":"assistant","timestamp":"2026-11-12T08:30:00Z","message":{"id":"m","usage":{"input_tokens":0}}}"#,
            "p"
        )
        .is_none());
    }

    /* -------- Codex 行解析 -------- */

    const CODEX_META: &str = r#"{"timestamp":"2026-11-12T08:00:00.000Z","type":"session_meta","payload":{"id":"s1","cwd":"/Users/x/desk-pet","originator":"codex_cli_rs"}}"#;
    const CODEX_TURN: &str = r#"{"timestamp":"2026-11-12T08:00:01.000Z","type":"turn_context","payload":{"cwd":"/Users/x/desk-pet","model":"gpt-5-codex"}}"#;

    #[test]
    fn codex_new_format_per_turn_usage() {
        let mut ctx = CodexCtx::new((0, 0, 0, 0));
        assert!(parse_codex_line(CODEX_META, &mut ctx).is_none());
        assert_eq!(ctx.project, "desk-pet");
        assert!(parse_codex_line(CODEX_TURN, &mut ctx).is_none());
        assert_eq!(ctx.model, "gpt-5-codex");

        let line = r#"{"timestamp":"2026-11-12T08:30:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":900,"cached_input_tokens":500,"output_tokens":120,"reasoning_output_tokens":40},"last_token_usage":{"input_tokens":400,"cached_input_tokens":300,"output_tokens":60,"reasoning_output_tokens":20}}}}"#;
        let e = parse_codex_line(line, &mut ctx).unwrap();
        assert_eq!(e.tool, TOOL_CODEX);
        assert_eq!(e.model, "gpt-5-codex");
        assert_eq!(e.project, "desk-pet");
        // output_tokens 已含推理，取 60 而非 60+20
        assert_eq!((e.input_tokens, e.output_tokens, e.cache_read), (400, 60, 300));
        assert_eq!(e.dedup_key, None);
    }

    #[test]
    fn codex_old_format_cumulative_delta() {
        let mut ctx = CodexCtx::new((0, 0, 0, 0));
        parse_codex_line(CODEX_META, &mut ctx);
        parse_codex_line(CODEX_TURN, &mut ctx);
        // 累计 100/20/10 -> 差值即本体
        let l1 = r#"{"timestamp":"2026-11-12T08:10:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":100,"cached_input_tokens":10,"output_tokens":20}}"#;
        let e1 = parse_codex_line(l1, &mut ctx).unwrap();
        assert_eq!((e1.input_tokens, e1.output_tokens, e1.cache_read), (100, 20, 10));
        // 累计增长到 150/30/10 -> 差值 50/10/0
        let l2 = r#"{"timestamp":"2026-11-12T08:20:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":150,"cached_input_tokens":10,"output_tokens":30}}"#;
        let e2 = parse_codex_line(l2, &mut ctx).unwrap();
        assert_eq!((e2.input_tokens, e2.output_tokens, e2.cache_read), (50, 10, 0));
        // 累计不变 -> 无事件
        let l3 = r#"{"timestamp":"2026-11-12T08:30:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":150,"cached_input_tokens":10,"output_tokens":30}}"#;
        assert!(parse_codex_line(l3, &mut ctx).is_none());
        // 跨扫描续算：游标基线传入后，累计值继续做差
        let mut ctx2 = CodexCtx::new(ctx.last);
        let l4 = r#"{"timestamp":"2026-11-12T08:40:00.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":170,"cached_input_tokens":15,"output_tokens":45}}"#;
        let e4 = parse_codex_line(l4, &mut ctx2).unwrap();
        assert_eq!((e4.input_tokens, e4.output_tokens, e4.cache_read), (20, 15, 5));
        // 其他 event_msg 子类型忽略
        let other = r#"{"timestamp":"2026-11-12T08:41:00.000Z","type":"event_msg","payload":{"type":"agent_message","message":"hi"}}"#;
        assert!(parse_codex_line(other, &mut ctx2).is_none());
    }

    /* -------- 端到端：fixture 文件 -> scan_tool -> store 查询 -------- */

    /// 用 tauri mock app + 临时目录 DB 跑完整扫描链路（不碰真实日志与正式库）。
    #[test]
    fn scan_tool_end_to_end_with_mock_app() {
        let logs = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();

        // Claude fixture：<root>/Users-x-proj/session.jsonl
        let claude_proj = logs.path().join("Users-x-proj");
        std::fs::create_dir_all(&claude_proj).unwrap();
        std::fs::write(
            claude_proj.join("session.jsonl"),
            concat!(
                r#"{"type":"user","timestamp":"2026-11-12T08:00:00.000Z","message":{"role":"user"}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-11-12T08:30:00.123Z","message":{"id":"msg_01","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":200,"output_tokens":80}}}"#,
                "\n",
                // API 错误行：同 id 重复，应被跳过
                r#"{"type":"assistant","timestamp":"2026-11-12T08:31:00.000Z","isApiErrorMessage":true,"message":{"id":"msg_01","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":200,"output_tokens":80}}}"#,
                "\n",
            ),
        )
        .unwrap();

        // Codex fixture：<root>/2026/11/12/rollout-2026-11-12.jsonl（新格式单轮直取）
        let codex_dir = logs.path().join("2026/11/12");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("rollout-2026-11-12.jsonl"),
            concat!(
                r#"{"timestamp":"2026-11-12T08:00:00.000Z","type":"session_meta","payload":{"id":"s1","cwd":"/Users/x/desk-pet","originator":"codex_cli_rs"}}"#,
                "\n",
                r#"{"timestamp":"2026-11-12T08:00:01.000Z","type":"turn_context","payload":{"cwd":"/Users/x/desk-pet","model":"gpt-5-codex"}}"#,
                "\n",
                r#"{"timestamp":"2026-11-12T08:30:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":900,"cached_input_tokens":500,"output_tokens":120},"last_token_usage":{"input_tokens":400,"cached_input_tokens":300,"output_tokens":60}}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let app = tauri::test::mock_app();
        let store = deskpet_store::Store::open(db_dir.path()).unwrap();
        app.manage(crate::state::AppState::new(store));

        // 首扫：各 1 条事件
        assert_eq!(scan_tool(app.handle(), TOOL_CLAUDE, logs.path()).unwrap(), 1);
        assert_eq!(scan_tool(app.handle(), TOOL_CODEX, logs.path()).unwrap(), 1);
        // 幂等：文件未变，重扫 0 新增
        assert_eq!(scan_tool(app.handle(), TOOL_CLAUDE, logs.path()).unwrap(), 0);
        assert_eq!(scan_tool(app.handle(), TOOL_CODEX, logs.path()).unwrap(), 0);

        // 查询聚合：总量 = Claude(100+80+200+50) + Codex(400+60+300)
        let state = app.state::<crate::state::AppState>();
        let s = state.store.lock().unwrap();
        let raw = s
            .ai_usage_query_range(1_794_400_000_000, 1_794_500_000_000)
            .unwrap();
        assert_eq!(raw.totals.input, 500);
        assert_eq!(raw.totals.output, 140);
        assert_eq!(raw.totals.cache_read, 500);
        assert_eq!(raw.totals.cache_write, 50);
        assert_eq!(raw.by_tool.len(), 2);
        assert_eq!(raw.by_model.len(), 2);
        // 项目：Claude 用目录名，Codex 用 cwd basename
        let mut projects: Vec<String> = raw.by_project.iter().map(|p| p.key.clone()).collect();
        projects.sort();
        assert_eq!(projects, vec!["Users-x-proj".to_string(), "desk-pet".to_string()]);
    }

    /* -------- OpenCode：message 解析 + 扫描端到端 -------- */

    const OC_ASSISTANT: &str = r#"{"parentID":"msg_1","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/Users/shaipe/workspace/tool/dsh-desktop","root":"/Users/shaipe/workspace/tool/dsh-desktop"},"cost":0.0123,"tokens":{"total":97683,"input":276,"output":127,"reasoning":0,"cache":{"write":0,"read":97280}},"modelID":"mimo-v2.5-free","providerID":"opencode","time":{"created":1789484001448,"completed":1789484012659},"finish":"stop"}"#;

    #[test]
    fn opencode_message_parses_tokens_and_real_cost() {
        let e = parse_opencode_message("msg_x", 1_789_484_000_000, OC_ASSISTANT).unwrap();
        assert_eq!(e.tool, TOOL_OPENCODE);
        assert_eq!(e.ts, 1_789_484_001_448);
        assert_eq!(e.model, "mimo-v2.5-free");
        assert_eq!(e.project, "dsh-desktop");
        assert_eq!((e.input_tokens, e.output_tokens), (276, 127));
        assert_eq!((e.cache_read, e.cache_write), (97280, 0));
        // 真实成本随事件存储
        assert_eq!(e.cost_usd, Some(0.0123));
        assert_eq!(e.dedup_key.as_deref(), Some("oc:msg_x"));
        // user 行 / 空 usage 行跳过
        assert!(parse_opencode_message("u", 0, r#"{"role":"user"}"#).is_none());
        assert!(
            parse_opencode_message(
                "z",
                0,
                r#"{"role":"assistant","cost":0,"tokens":{"input":0,"output":0}}"#
            )
            .is_none()
        );
        // data 缺 time.created 时回退行级 time_created
        let no_time = r#"{"role":"assistant","cost":0.5,"tokens":{"input":10,"output":5},"modelID":"gpt-5"}"#;
        let e = parse_opencode_message("m", 123_456, no_time).unwrap();
        assert_eq!(e.ts, 123_456);
        assert_eq!(e.cost_usd, Some(0.5));
    }

    /// opencode.db fixture -> scan_opencode_at -> store 查询（mock app + 临时库）。
    #[test]
    fn scan_opencode_end_to_end_with_mock_app() {
        let tmp = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = tmp.path().join("opencode.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, \
             time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL)",
        )
        .unwrap();
        let ins = "INSERT INTO message (id, session_id, time_created, time_updated, data) \
                   VALUES (?1, 's', ?2, ?2, ?3)";
        conn.execute(ins, rusqlite::params!("m_user", 100, r#"{"role":"user"}"#))
            .unwrap();
        conn.execute(ins, rusqlite::params!("m1", 200, OC_ASSISTANT)).unwrap();
        drop(conn);

        let app = tauri::test::mock_app();
        let store = deskpet_store::Store::open(db_dir.path()).unwrap();
        app.manage(crate::state::AppState::new(store));

        // 首扫：1 条 assistant 事件
        assert_eq!(scan_opencode_at(app.handle(), &db).unwrap(), 1);
        // 幂等：重扫 0 新增（dedup 命中，游标推进后无新消息）
        assert_eq!(scan_opencode_at(app.handle(), &db).unwrap(), 0);

        // 追加新消息后再扫：1 新增
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute(
            ins,
            rusqlite::params!(
                "m2",
                300,
                r#"{"role":"assistant","cost":0.02,"tokens":{"input":50,"output":10,"cache":{"read":0,"write":0}},"modelID":"gpt-5","path":{"cwd":"/Users/shaipe/workspace/tool/dsh-desktop"},"time":{"created":1789485000000}}"#
            ),
        )
        .unwrap();
        drop(conn);
        assert_eq!(scan_opencode_at(app.handle(), &db).unwrap(), 1);
        assert_eq!(scan_opencode_at(app.handle(), &db).unwrap(), 0);

        // 查询：两条事件聚合，stored 成本为真实成本之和
        let state = app.state::<crate::state::AppState>();
        let s = state.store.lock().unwrap();
        let raw = s
            .ai_usage_query_range(0, 1_789_486_000_000)
            .unwrap();
        assert_eq!(raw.totals.input, 326);
        assert_eq!(raw.totals.output, 137);
        assert_eq!(raw.totals.cache_read, 97280);
        assert!((raw.totals.stored_cost - 0.0323).abs() < 1e-9);
        assert_eq!(raw.totals.est_input, 0);
        assert_eq!(raw.by_tool.len(), 1);
        assert_eq!(raw.by_tool[0].key, "opencode");
        assert_eq!(raw.by_model.len(), 2);
        assert_eq!(raw.by_project.len(), 1);
        assert_eq!(raw.by_project[0].key, "dsh-desktop");
    }
}
