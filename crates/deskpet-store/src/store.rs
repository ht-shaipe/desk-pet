use rusqlite::Connection;
use std::path::Path;

use crate::Result;
use crate::migrate::migrate_json;
use crate::models::{ChatEntry, ClipEntry, Favorite, LauncherConfig, LauncherItem, Note};

const DB_FILE: &str = "deskpet.db";

/// 目录的成员路径范围 [lo, hi):目录下文件 path 均以 "dir/" 开头。
/// 利用 BINARY 排序取 hi = lo 末字节 +1,可走 path 唯一索引的范围扫描;
/// 根目录("/")的 base 为空,匹配全部绝对路径。
/// 文档/图库共用:嵌套目录共享同一份索引,成员关系按路径前缀而非 dir_id。
pub(crate) fn dir_bounds(dir_path: &str) -> (String, String) {
    let sep = std::path::MAIN_SEPARATOR;
    let base = dir_path.trim_end_matches(sep);
    let lo = format!("{base}{sep}");
    // hi = lo 末字节(分隔符)+1:分隔符必为单字节 ASCII,+1 后仍是合法 UTF-8
    let mut hi_bytes = lo.as_bytes().to_vec();
    let last = hi_bytes.len() - 1;
    hi_bytes[last] += 1;
    let hi = String::from_utf8(hi_bytes).expect("分隔符 +1 仍是合法 UTF-8");
    (lo, hi)
}

/// path 是否位于 dir 目录之下(按路径前缀)。
pub(crate) fn path_under(path: &str, dir: &str) -> bool {
    let sep = std::path::MAIN_SEPARATOR;
    let base = dir.trim_end_matches(sep);
    if base.is_empty() {
        return path.starts_with(sep);
    }
    path.starts_with(&format!("{base}{sep}"))
}

/// 当前 Unix 时间戳（毫秒）。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 与前端 FAV_CATEGORIES 对齐：“其他”的序号（19 项中的最后一项）。
const LEGACY_NOTE_CATEGORY_OTHER: i64 = 18;

/// 在给定事务内批量写收藏，返回真实写入条数。
/// 合并策略 last-write-wins：本地 update_time 比传入新时整条跳过，
/// 避免云端 pull / 旧导入文件静默覆盖用户已编辑、尚未推送的收藏。
fn upsert_in_tx(tx: &rusqlite::Transaction, items: &[Favorite], now: &str) -> Result<usize> {
    let mut count = 0usize;
    {
        let mut existing =
            tx.prepare("SELECT create_time, update_time FROM favorite WHERE url = ?1")?;
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO favorite
             (url, name, description, fav_type, payload, languages, stars, forks,
              tags, category, category_name, create_time, update_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )?;
        for f in items {
            let url = f.url.trim();
            if url.is_empty() {
                continue;
            }
            // 只把 QueryReturnedNoRows 当作“不存在”，真实 DB 错误必须上抛
            let prev: Option<(String, String)> = existing
                .query_row([url], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(Some)
                .or_else(|e| {
                    if e == rusqlite::Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                })?;
            let (prev_create, prev_update) = match prev {
                Some((c, u)) => (Some(c), Some(u)),
                None => (None, None),
            };
            let create_time = if f.create_time.trim().is_empty() {
                prev_create.unwrap_or_else(|| now.to_string())
            } else {
                f.create_time.trim().to_string()
            };
            let update_time = if f.update_time.trim().is_empty() {
                now.to_string()
            } else {
                f.update_time.trim().to_string()
            };
            if let Some(local) = prev_update {
                if !local.is_empty() && update_time < local {
                    // 本地较新，保留本地记录
                    continue;
                }
            }
            insert.execute(rusqlite::params![
                url,
                f.name,
                f.description,
                f.fav_type,
                f.payload,
                serde_json::to_string(&f.languages).unwrap_or_else(|_| "[]".into()),
                f.stars,
                f.forks,
                serde_json::to_string(&f.tags).unwrap_or_else(|_| "[]".into()),
                f.category,
                f.category_name,
                create_time,
                update_time,
            ])?;
            count += 1;
        }
    }
    Ok(count)
}

/// UTC ISO8601（YYYY-MM-DDTHH:MM:SSZ），手写避免引入 chrono。
fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso_from_secs(secs)
}

/// Unix 秒 -> "YYYY-MM-DDTHH:MM:SSZ"（UTC）。
fn iso_from_secs(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    // civil_from_days（Howard Hinnant 算法）：天数 -> 年月日
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z",
        h = rem / 3600,
        m = rem % 3600 / 60,
        s = rem % 60
    )
}

/// 文档管理已随「图库仅图片」定位移除：启动时清理历史 doc 索引行与缩略图（幂等）。
/// doc 行是早期版本扫描入库的，不清掉会持续污染去重/搜索结果。
fn purge_doc_items(conn: &Connection, dir: &Path) {
    let thumbs: Vec<String> = match conn
        .prepare("SELECT thumb FROM image_item WHERE doc = 1 AND thumb != ''")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Ok(v) => v,
        Err(_) => return,
    };
    for t in &thumbs {
        let _ = std::fs::remove_file(dir.join("image_thumbs").join(t));
    }
    let _ = conn.execute(
        "DELETE FROM image_item_tag WHERE item_id IN (SELECT id FROM image_item WHERE doc = 1)",
        [],
    );
    let _ = conn.execute("DELETE FROM image_item WHERE doc = 1", []);
}

/// 清理孤儿图线条目:路径不被任何图库目录覆盖(目录已移除但索引残留,
/// 如旧版全盘扫描目录移除时未清干净的 /System 资源图)。
/// 与 remove_image_dir 的覆盖语义一致:无目录覆盖即不索引。缩略图与标签关联一并清理。
fn purge_orphan_image_items(conn: &Connection, dir: &Path) {
    let dirs: Vec<String> = match conn
        .prepare("SELECT path FROM image_dir")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Ok(v) => v,
        Err(_) => return,
    };
    let items: Vec<(i64, String, String)> = match conn
        .prepare("SELECT id, path, thumb FROM image_item")
        .and_then(|mut s| {
            s.query_map(
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Ok(v) => v,
        Err(_) => return,
    };
    let victims: Vec<(i64, String)> = items
        .into_iter()
        .filter(|(_, p, _)| !dirs.iter().any(|d| path_under(p, d)))
        .map(|(id, _, thumb)| (id, thumb))
        .collect();
    if victims.is_empty() {
        return;
    }
    // 先删行再删文件:事务失败只是残留孤儿缩略图文件(无害),
    // 反过来会造成"行在文件丢"的破图且不会自愈(mtime 未变不重扫)
    if let Ok(tx) = conn.unchecked_transaction() {
        for (id, _) in &victims {
            let _ = tx.execute("DELETE FROM image_item_tag WHERE item_id = ?1", rusqlite::params![id]);
            let _ = tx.execute("DELETE FROM image_item WHERE id = ?1", rusqlite::params![id]);
        }
        let _ = tx.commit();
    }
    for (_, t) in &victims {
        if !t.is_empty() {
            let _ = std::fs::remove_file(dir.join("image_thumbs").join(t));
        }
    }
}

/// Unix 毫秒 -> ISO（旧便签 updated_at 用）。
fn iso_from_ms(ms: u64) -> String {
    iso_from_secs(ms / 1000)
}

/// 旧便签迁入收藏时的名称：取首行非空文本，最多 24 字，空则回落「便签」。
fn legacy_note_name(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(24).collect())
        .unwrap_or_else(|| "便签".into())
}

const DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL DEFAULT '#fef3c7',
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS clipboard (
    id TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'text',
    copy_count INTEGER NOT NULL DEFAULT 1,
    sig TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS launcher (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT NOT NULL DEFAULT '',
    item_type TEXT NOT NULL DEFAULT 'app',
    value TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#909399',
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS chat_message (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_calls TEXT,
    tool_call_id TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS favorite (
    url TEXT PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    fav_type TEXT NOT NULL DEFAULT 'repo',
    payload TEXT NOT NULL DEFAULT '{}',
    languages TEXT NOT NULL DEFAULT '[]',
    stars INTEGER NOT NULL DEFAULT 0,
    forks INTEGER NOT NULL DEFAULT 0,
    tags TEXT NOT NULL DEFAULT '[]',
    category INTEGER NOT NULL DEFAULT 0,
    category_name TEXT NOT NULL DEFAULT '',
    create_time TEXT NOT NULL DEFAULT '',
    update_time TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS image_dir (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    full_disk INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT 0,
    last_scan_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS image_item (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dir_id INTEGER NOT NULL,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    ext TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    taken_at INTEGER NOT NULL DEFAULT 0,
    camera TEXT NOT NULL DEFAULT '',
    is_fav INTEGER NOT NULL DEFAULT 0,
    doc INTEGER NOT NULL DEFAULT 0,
    thumb TEXT NOT NULL DEFAULT '',
    hash TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_image_item_taken ON image_item(taken_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_image_item_dir ON image_item(dir_id);
CREATE INDEX IF NOT EXISTS idx_image_item_hash ON image_item(hash);
CREATE TABLE IF NOT EXISTS image_tag (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS image_item_tag (
    item_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (item_id, tag_id)
);
CREATE TABLE IF NOT EXISTS doc_dir (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    added_at INTEGER NOT NULL DEFAULT 0,
    last_scan_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS doc_item (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dir_id INTEGER NOT NULL,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    ext TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_doc_item_dir ON doc_item(dir_id);
CREATE INDEX IF NOT EXISTS idx_doc_item_name ON doc_item(name);
CREATE TABLE IF NOT EXISTS ai_usage_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool TEXT NOT NULL,
    ts INTEGER NOT NULL,
    model TEXT NOT NULL DEFAULT '',
    project TEXT NOT NULL DEFAULT '',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_write INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    dedup_key TEXT UNIQUE
);
CREATE INDEX IF NOT EXISTS idx_ai_usage_event_ts ON ai_usage_event(ts);
CREATE INDEX IF NOT EXISTS idx_ai_usage_event_tool_ts ON ai_usage_event(tool, ts);
CREATE TABLE IF NOT EXISTS ai_usage_file_state (
    path TEXT NOT NULL,
    tool TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    parsed_lines INTEGER NOT NULL DEFAULT 0,
    last_input INTEGER NOT NULL DEFAULT 0,
    last_output INTEGER NOT NULL DEFAULT 0,
    last_cache_read INTEGER NOT NULL DEFAULT 0,
    last_cache_write INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (path, tool)
);";

/// 本地存储：SQLite（WAL），打开时建表并按需迁移旧 JSON 数据。
pub struct Store {
    /// crate 内模块共享(gallery.rs 等同 crate impl 也需访问)
    pub(crate) conn: Connection,
}

impl Store {
    /// 打开（或创建）`dir/deskpet.db`。
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(DB_FILE))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(DDL)?;
        // 旧表无 kind 列时补加（ALTER TABLE ADD COLUMN 幂等性：列已存在则报错，忽略）
        let _ = conn.execute("ALTER TABLE clipboard ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'", []);
        // 旧表无 copy_count 列时补加（同上，幂等）
        let _ = conn.execute("ALTER TABLE clipboard ADD COLUMN copy_count INTEGER NOT NULL DEFAULT 1", []);
        // 旧表无 sig 列时补加（同上，幂等；图片内容签名）
        let _ = conn.execute("ALTER TABLE clipboard ADD COLUMN sig TEXT NOT NULL DEFAULT ''", []);
        // 旧 chat_message 表无工具调用列时补加（同上，幂等；AI 工具调用持久化）
        let _ = conn.execute("ALTER TABLE chat_message ADD COLUMN tool_calls TEXT", []);
        let _ = conn.execute("ALTER TABLE chat_message ADD COLUMN tool_call_id TEXT", []);
        // 旧 ai_usage_event 无 cost_usd 列时补加（同上，幂等；OpenCode 真实成本）
        let _ = conn.execute("ALTER TABLE ai_usage_event ADD COLUMN cost_usd REAL", []);
        migrate_json(&conn, dir)?;
        purge_doc_items(&conn, dir);
        purge_orphan_image_items(&conn, dir);
        // 便签功能并入收藏：启动即把 legacy notes 表迁入 favorite（幂等）
        let store = Self { conn };
        store.migrate_legacy_notes()?;
        Ok(store)
    }

    /// 旧便签数据（迁移数据源，便签功能已并入收藏）。
    pub fn get_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, content, color, updated_at FROM notes ORDER BY rowid")?;
        let rows = stmt.query_map([], |row| {
            Ok(Note {
                id: row.get(0)?,
                content: row.get(1)?,
                color: row.get(2)?,
                updated_at: row.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 旧便签模块退役（并入收藏便签）：一次性迁移 legacy notes 表 -> favorite。
    /// - url = stash://note/<id>（与 stash 扩展删除约定一致）
    /// - fav_type = note，正文/颜色载入 payload.content / payload.color
    /// - 迁移成功后清空 notes 表 -> 幂等，重复调用返回 0
    pub fn migrate_legacy_notes(&self) -> Result<u64> {
        let notes = self.get_notes()?;
        if notes.is_empty() {
            return Ok(0);
        }
        let items = notes
            .iter()
            .map(|n| Favorite {
                url: format!("stash://note/{}", n.id),
                name: legacy_note_name(&n.content),
                description: String::new(),
                fav_type: "note".into(),
                payload: serde_json::json!({ "content": n.content, "color": n.color }).to_string(),
                languages: vec![],
                stars: 0,
                forks: 0,
                tags: vec![],
                category: LEGACY_NOTE_CATEGORY_OTHER,
                category_name: "其他".into(),
                create_time: iso_from_ms(n.updated_at),
                update_time: iso_from_ms(n.updated_at),
            })
            .collect::<Vec<_>>();
        // 写入与清空旧表在同一个事务里提交，避免两提交点之间的“半迁移”窗口
        let tx = self.conn.unchecked_transaction()?;
        let migrated = upsert_in_tx(&tx, &items, &iso_now())? as u64;
        tx.execute("DELETE FROM notes", [])?;
        tx.commit()?;
        Ok(migrated)
    }

    /// 剪贴板历史（按更新时间倒序）。
    pub fn get_clipboard(&self) -> Result<Vec<ClipEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, text, kind, copy_count, sig, updated_at FROM clipboard ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(ClipEntry {
                id: row.get(0)?,
                text: row.get(1)?,
                kind: row.get(2)?,
                copy_count: row.get::<_, i64>(3)? as u32,
                sig: row.get(4)?,
                updated_at: row.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 全量替换剪贴板历史快照。
    pub fn save_clipboard(&self, entries: &[ClipEntry]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM clipboard", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO clipboard (id, text, kind, copy_count, sig, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for e in entries {
                stmt.execute(rusqlite::params![e.id, e.text, e.kind, e.copy_count, e.sig, e.updated_at as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 全部收藏（新 → 旧；create_time 为 ISO 字符串排序）。
    pub fn get_favorites(&self) -> Result<Vec<Favorite>> {
        let mut stmt = self.conn.prepare(
            "SELECT url, name, description, fav_type, payload, languages, stars, forks,
                    tags, category, category_name, create_time, update_time
             FROM favorite
             ORDER BY create_time DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let langs: String = row.get(5)?;
            let tags: String = row.get(8)?;
            Ok(Favorite {
                url: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                fav_type: row.get(3)?,
                payload: row.get(4)?,
                languages: serde_json::from_str(&langs).unwrap_or_default(),
                stars: row.get(6)?,
                forks: row.get(7)?,
                tags: serde_json::from_str(&tags).unwrap_or_default(),
                category: row.get(9)?,
                category_name: row.get(10)?,
                create_time: row.get(11)?,
                update_time: row.get(12)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 批量 upsert 收藏（url 主键查重），返回实际入库条数。
    /// 合并（导入 / 云拉取）时保留已有 create_time；update_time 为空补当前。
    pub fn upsert_favorites(&self, items: &[Favorite]) -> Result<usize> {
        let now = iso_now();
        let tx = self.conn.unchecked_transaction()?;
        let count = upsert_in_tx(&tx, items, &now)?;
        tx.commit()?;
        Ok(count)
    }

    /// 批量删除收藏（按 url）。
    pub fn delete_favorites(&self, urls: &[String]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut count = 0usize;
        {
            let mut stmt = tx.prepare("DELETE FROM favorite WHERE url = ?1")?;
            for url in urls {
                count += stmt.execute([url])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// 清空全部收藏。
    pub fn clear_favorites(&self) -> Result<()> {
        self.conn.execute("DELETE FROM favorite", [])?;
        Ok(())
    }

    /// 全部启动器条目（按保存顺序）。
    pub fn get_launcher(&self) -> Result<LauncherConfig> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, icon, item_type, value, color FROM launcher ORDER BY sort_order",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LauncherItem {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                item_type: row.get(3)?,
                value: row.get(4)?,
                color: row.get(5)?,
            })
        })?;
        Ok(LauncherConfig {
            items: rows.filter_map(|r| r.ok()).collect(),
        })
    }

    /// 全量替换启动器条目。
    pub fn save_launcher(&self, items: &[LauncherItem]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM launcher", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO launcher (id, name, icon, item_type, value, color, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (i, item) in items.iter().enumerate() {
                stmt.execute(rusqlite::params![
                    item.id,
                    item.name,
                    item.icon,
                    item.item_type,
                    item.value,
                    item.color,
                    i as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 全部聊天消息（按写入顺序）。
    pub fn list_chat_messages(&self) -> Result<Vec<ChatEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content, tool_calls, tool_call_id, created_at FROM chat_message ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ChatEntry {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                tool_calls: row.get(3)?,
                tool_call_id: row.get(4)?,
                created_at: row.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 追加一条聊天消息，返回带 id 的完整记录。
    /// assistant 工具轮携带 `tool_calls`（JSON），tool 结果消息携带 `tool_call_id`。
    pub fn add_chat_message(
        &self,
        role: &str,
        content: &str,
        tool_calls: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> Result<ChatEntry> {
        let created_at = now_ms();
        self.conn.execute(
            "INSERT INTO chat_message (role, content, tool_calls, tool_call_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![role, content, tool_calls, tool_call_id, created_at as i64],
        )?;
        Ok(ChatEntry {
            id: self.conn.last_insert_rowid(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: tool_calls.map(|s| s.to_string()),
            tool_call_id: tool_call_id.map(|s| s.to_string()),
            created_at,
        })
    }

    /// 删除一条聊天消息。
    pub fn delete_chat_message(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM chat_message WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 删除指定 id 及其之后的所有聊天消息（含该条），返回删除条数。
    /// 用于「重新执行」：截断某条用户消息起的会话后重新发送。
    pub fn delete_chat_messages_from(&self, id: i64) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM chat_message WHERE id >= ?1", [id])?;
        Ok(n)
    }

    /// 清空全部聊天消息。
    pub fn clear_chat_messages(&self) -> Result<()> {
        self.conn.execute("DELETE FROM chat_message", [])?;
        Ok(())
    }

    /// 迁移旧 settings.json 中的聊天历史（仅当表为空时导入），返回导入条数。
    pub fn migrate_chat_messages(
        &self,
        messages: &[crate::settings::ChatMessage],
    ) -> Result<usize> {
        if messages.is_empty() {
            return Ok(0);
        }
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM chat_message", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chat_message (role, content, created_at) VALUES (?1, ?2, ?3)",
            )?;
            for m in messages {
                stmt.execute(rusqlite::params![m.role, m.content, now_ms() as i64])?;
            }
        }
        tx.commit()?;
        Ok(messages.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_purges_orphan_image_items() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        {
            let s = Store::open(&path).unwrap();
            // 正常目录与其成员
            s.conn
                .execute(
                    "INSERT INTO image_dir (path, full_disk, added_at) VALUES ('/pics', 0, 0)",
                    [],
                )
                .unwrap();
            s.conn
                .execute(
                    "INSERT INTO image_item (dir_id, path, name, ext) VALUES (1, '/pics/a.jpg', 'a.jpg', 'jpg')",
                    [],
                )
                .unwrap();
            // 孤儿:目录行已删(旧版残留),路径不被任何目录覆盖
            s.conn
                .execute(
                    "INSERT INTO image_item (dir_id, path, name, ext, thumb) VALUES (99, '/System/x.jpg', 'x.jpg', 'jpg', 'aa/bb.jpg')",
                    [],
                )
                .unwrap();
            // 被现有目录覆盖的"脏"归属(dir_id 指向已删目录)不应被清
            s.conn
                .execute(
                    "INSERT INTO image_item (dir_id, path, name, ext) VALUES (99, '/pics/b.jpg', 'b.jpg', 'jpg')",
                    [],
                )
                .unwrap();
        }
        // 重开触发启动清理
        let s2 = Store::open(&path).unwrap();
        let orphan: i64 = s2
            .conn
            .query_row("SELECT COUNT(*) FROM image_item WHERE path = '/System/x.jpg'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphan, 0, "孤儿条目被清理");
        let kept: i64 = s2
            .conn
            .query_row("SELECT COUNT(*) FROM image_item WHERE path IN ('/pics/a.jpg', '/pics/b.jpg')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 2, "被目录覆盖的条目保留(不管 dir_id 归属)");
    }

    fn sample_notes() -> Vec<Note> {
        vec![
            Note {
                id: "n1".into(),
                content: "第一条".into(),
                color: "#fef3c7".into(),
                updated_at: 1000,
            },
            Note {
                id: "n2".into(),
                content: "第二条".into(),
                color: "#dbeafe".into(),
                updated_at: 2000,
            },
        ]
    }

    #[test]
    fn migrates_legacy_notes_into_favorites() {
        let dir = tempfile::tempdir().unwrap();
        // 直接把旧便签写进 notes 表（模拟旧版数据）
        {
            let store = Store::open(dir.path()).unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO notes (id, content, color, updated_at) VALUES ('n1', '第一行\n正文内容', '#fce7f3', 1698200000000)",
                    [],
                )
                .unwrap();
        }

        // open() 自动完成迁移
        let store = Store::open(dir.path()).unwrap();
        assert!(store.get_notes().unwrap().is_empty(), "迁移后 notes 表应被清空");

        let fav = store
            .get_favorites()
            .unwrap()
            .into_iter()
            .find(|f| f.url == "stash://note/n1")
            .expect("legacy note 应迁为收藏行");
        assert_eq!(fav.fav_type, "note");
        assert_eq!(fav.name, "第一行");
        assert_eq!(fav.category_name, "其他");
        let payload: serde_json::Value = serde_json::from_str(&fav.payload).unwrap();
        assert_eq!(payload["content"], "第一行\n正文内容");
        assert_eq!(payload["color"], "#fce7f3");
        assert_eq!(fav.create_time, "2023-10-25T02:13:20Z");
        assert_eq!(fav.update_time, "2023-10-25T02:13:20Z");

        // 幂等：再次打开（再迁移）返回 0 且无副作用
        assert_eq!(Store::open(dir.path()).unwrap().migrate_legacy_notes().unwrap(), 0);
        assert_eq!(
            store
                .get_favorites()
                .unwrap()
                .iter()
                .filter(|f| f.url == "stash://note/n1")
                .count(),
            1
        );
    }

    #[test]
    fn clipboard_roundtrip_ordered_by_updated_at_desc() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let entries = vec![
            ClipEntry {
                id: "c1".into(),
                text: "旧".into(),
                kind: "text".into(),
                updated_at: 1000,
                copy_count: 3,
                sig: String::new(),
            },
            ClipEntry {
                id: "c2".into(),
                text: "新".into(),
                kind: "text".into(),
                updated_at: 2000,
                copy_count: 1,
                sig: String::new(),
            },
        ];
        store.save_clipboard(&entries).unwrap();
        let loaded = store.get_clipboard().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "c2");
        assert_eq!(loaded[1].id, "c1");
        assert_eq!(loaded[1].copy_count, 3);
    }

    #[test]
    fn favorite_serde_keys_are_snake_case() {
        // Tauri 传输契约：与 TS 类型（src/types Favorite）字段名一致，
        // 若加回 rename_all = "camelCase" 会导致前端读到 undefined。
        let item = Favorite {
            fav_type: "repo".into(),
            category_name: "AI/Agent".into(),
            create_time: "2026-10-25T00:00:00Z".into(),
            ..Default::default()
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["fav_type"], "repo");
        assert_eq!(json["category_name"], "AI/Agent");
        assert_eq!(json["create_time"], "2026-10-25T00:00:00Z");
        let back: Favorite = serde_json::from_value(json).unwrap();
        assert_eq!(back, item);
    }

    #[test]
    fn upsert_skips_items_older_than_local() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let favorite = |name: &str, create: &str, update: &str| Favorite {
            url: "x://1".into(),
            name: name.into(),
            create_time: create.into(),
            update_time: update.into(),
            ..Default::default()
        };
        store
            .upsert_favorites(&[favorite("local", "2026-01-01T00:00:00Z", "2026-10-01T00:00:00Z")])
            .unwrap();
        // 云端旧数据（update_time 更早）不应覆盖本地较新的编辑
        store
            .upsert_favorites(&[favorite("stale", "2026-05-01T00:00:00Z", "2026-09-01T00:00:00Z")])
            .unwrap();
        let list = store.get_favorites().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "local");
        assert_eq!(list[0].update_time, "2026-10-01T00:00:00Z");
        // 本地更旧的记录可以被更新的数据覆盖
        store
            .upsert_favorites(&[favorite("newer", "2026-05-01T00:00:00Z", "2026-10-20T00:00:00Z")])
            .unwrap();
        let list = store.get_favorites().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "newer");
    }

    #[test]
    fn favorite_upsert_merges_delists_and_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let item = |url: &str, name: &str, create: &str, star: i64| Favorite {
            url: url.into(),
            name: name.into(),
            fav_type: "repo".into(),
            stars: star,
            create_time: create.into(),
            ..Default::default()
        };
        // 插入两条：a 新 b 旧
        store
            .upsert_favorites(&[item("b://x", "b", "2026-01-01T00:00:00Z", 1), item("a://x", "a", "2026-10-01T00:00:00Z", 5)])
            .unwrap();
        let list = store.get_favorites().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].url, "a://x");
        assert_eq!(list[0].stars, 5);

        // 同 url 再入库（云端合并）：create_time 缺省时保留旧的
        store
            .upsert_favorites(&[Favorite {
                url: "a://x".into(),
                name: "a2".into(),
                stars: 9,
                ..Default::default()
            }])
            .unwrap();
        let list = store.get_favorites().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a2");
        assert_eq!(list[0].create_time, "2026-10-01T00:00:00Z");
        assert_eq!(list[0].stars, 9);
        assert!(list[0].update_time.starts_with("20"));

        // 批量删除 + 空项跳过
        let removed = store
            .delete_favorites(&["b://x".into(), "missing://".into()])
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.get_favorites().unwrap().len(), 1);

        store.clear_favorites().unwrap();
        assert!(store.get_favorites().unwrap().is_empty());
    }

    #[test]
    fn launcher_roundtrip_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let items = vec![
            LauncherItem {
                id: "l1".into(),
                name: "GitHub".into(),
                icon: "G".into(),
                item_type: "url".into(),
                value: "https://github.com".into(),
                color: "#409eff".into(),
            },
            LauncherItem {
                id: "l2".into(),
                name: "终端".into(),
                icon: ">".into(),
                item_type: "app".into(),
                value: "Terminal".into(),
                color: "#67c23a".into(),
            },
        ];
        store.save_launcher(&items).unwrap();
        assert_eq!(store.get_launcher().unwrap().items, items);
    }

    #[test]
    fn migrated_legacy_json_favorites_and_bak() {
        let dir = tempfile::tempdir().unwrap();
        // 预置旧版 JSON 数据
        std::fs::write(
            dir.path().join("notes.json"),
            serde_json::to_string(&sample_notes()).unwrap(),
        )
        .unwrap();

        let store = Store::open(dir.path()).unwrap();
        // JSON -> notes 临时表 -> 自动并入收藏
        let n1 = store
            .get_favorites()
            .unwrap()
            .into_iter()
            .find(|f| f.url == "stash://note/n1")
            .expect("旧 JSON 便签应迁入收藏");
        let payload: serde_json::Value = serde_json::from_str(&n1.payload).unwrap();
        assert_eq!(payload["content"], "第一条");
        assert_eq!(payload["color"], "#fef3c7");
        // 原始 JSON 备份为 .bak
        assert!(dir.path().join("notes.json.bak").exists());
        assert!(!dir.path().join("notes.json").exists());
    }

    #[test]
    fn chat_messages_roundtrip_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(store.list_chat_messages().unwrap().is_empty());

        let m1 = store.add_chat_message("user", "你好", None, None).unwrap();
        let calls = r#"[{"id":"c1","type":"function","function":{"name":"open_app","arguments":"{}"}}]"#;
        let m2 = store
            .add_chat_message("assistant", "", Some(calls), None)
            .unwrap();
        let m3 = store
            .add_chat_message("tool", r#"{"ok":true,"text":"已启动"}"#, None, Some("c1"))
            .unwrap();
        assert!(m1.id < m2.id && m2.id < m3.id);
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].content, "你好");
        assert_eq!(list[1].role, "assistant");
        assert_eq!(list[1].tool_calls.as_deref(), Some(calls));
        assert_eq!(list[2].tool_call_id.as_deref(), Some("c1"));

        store.delete_chat_message(m1.id).unwrap();
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, m2.id);
        assert_eq!(list[1].tool_call_id.as_deref(), Some("c1"));

        store.clear_chat_messages().unwrap();
        assert!(store.list_chat_messages().unwrap().is_empty());
    }

    #[test]
    fn chat_messages_delete_from_truncates_tail() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let m1 = store.add_chat_message("user", "第一问", None, None).unwrap();
        let m2 = store.add_chat_message("assistant", "第一答", None, None).unwrap();
        let m3 = store.add_chat_message("user", "第二问", None, None).unwrap();
        store.add_chat_message("assistant", "第二答", None, None).unwrap();

        let n = store.delete_chat_messages_from(m3.id).unwrap();
        assert_eq!(n, 2);
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, m1.id);
        assert_eq!(list[1].id, m2.id);

        // 首条也支持整段截断（清空效果）
        let n = store.delete_chat_messages_from(m1.id).unwrap();
        assert_eq!(n, 2);
        assert!(store.list_chat_messages().unwrap().is_empty());
    }

    #[test]
    fn migrates_settings_chat_history_only_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let msgs = vec![crate::settings::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        assert_eq!(store.migrate_chat_messages(&msgs).unwrap(), 1);
        // 表非空时跳过导入，避免覆盖已删除的内容
        assert_eq!(store.migrate_chat_messages(&msgs).unwrap(), 0);
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].content, "hi");
    }
}
