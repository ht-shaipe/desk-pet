//! OpenCode（sst/opencode）本地用量读取：只读打开 `~/.local/share/opencode/opencode.db`，
//! 拉取 assistant 消息（其 data JSON 内含 tokens/cost/model/cwd/时间戳）。
//! 只做 SQL 读取，JSON 解析在 src-tauri 层完成；打开失败/表不存在时返回空（工具未使用）。

use std::path::Path;

use rusqlite::OpenFlags;

use crate::Result;

/// opencode.db 中一条 assistant 消息（原始 data JSON 交由上层解析）。
#[derive(Debug, Clone)]
pub struct OpenCodeMessageRow {
    pub id: String,
    /// 消息表列 time_created（Unix 毫秒）
    pub time_created: i64,
    /// message.data 原始 JSON 字符串
    pub data: String,
}

/// 只读打开 opencode.db，返回 time_created >= since_ms 的 assistant 消息（按时间升序）。
/// 打开失败（未安装/权限/WAL 锁定等）返回空 Vec——调用方以「该工具无数据」处理。
pub fn read_opencode_messages(db_path: &Path, since_ms: i64) -> Result<Vec<OpenCodeMessageRow>> {
    if !db_path.is_file() {
        return Ok(Vec::new());
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // 兼容旧/新版本：无 message 表视为未使用
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='message'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, time_created, data FROM message \
         WHERE time_created >= ?1 AND json_extract(data, '$.role') = 'assistant' \
         ORDER BY time_created, id",
    )?;
    let rows = stmt.query_map([since_ms], |row| {
        Ok(OpenCodeMessageRow {
            id: row.get(0)?,
            time_created: row.get(1)?,
            data: row.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn reads_assistant_messages_since() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, \
             time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL)",
        )
        .unwrap();
        let ins = "INSERT INTO message (id, session_id, time_created, time_updated, data) \
                   VALUES (?1, 's', ?2, ?2, ?3)";
        conn.execute(ins, params!("m1", 100, r#"{"role":"user"}"#)).unwrap();
        conn.execute(
            ins,
            params!("m2", 200, r#"{"role":"assistant","tokens":{"input":10,"output":5}}"#),
        )
        .unwrap();
        conn.execute(
            ins,
            params!("m3", 300, r#"{"role":"assistant","tokens":{"input":7,"output":3}}"#),
        )
        .unwrap();
        drop(conn);

        // since=0：只返回 assistant 的 m2/m3
        let rows = read_opencode_messages(&db, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "m2");
        assert_eq!(rows[1].id, "m3");
        assert_eq!(rows[1].time_created, 300);

        // since=250：只剩 m3
        let rows = read_opencode_messages(&db, 250).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "m3");

        // 文件不存在：空
        assert!(read_opencode_messages(&dir.path().join("nope.db"), 0)
            .unwrap()
            .is_empty());

        // 无 message 表：空
        let empty = dir.path().join("empty.db");
        let conn = rusqlite::Connection::open(&empty).unwrap();
        conn.execute_batch("CREATE TABLE other (x)").unwrap();
        drop(conn);
        assert!(read_opencode_messages(&empty, 0).unwrap().is_empty());
    }
}
