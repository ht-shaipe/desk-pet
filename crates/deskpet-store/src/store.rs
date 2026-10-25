use rusqlite::Connection;
use std::path::Path;

use crate::Result;
use crate::migrate::migrate_json;
use crate::models::{ChatEntry, ClipEntry, Favorite, LauncherConfig, LauncherItem, Note};

const DB_FILE: &str = "deskpet.db";

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
);";

/// 本地存储：SQLite（WAL），打开时建表并按需迁移旧 JSON 数据。
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开（或创建）`dir/deskpet.db`。
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(DB_FILE))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(DDL)?;
        migrate_json(&conn, dir)?;
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
            .prepare("SELECT id, text, updated_at FROM clipboard ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(ClipEntry {
                id: row.get(0)?,
                text: row.get(1)?,
                updated_at: row.get::<_, i64>(2)? as u64,
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
                "INSERT OR REPLACE INTO clipboard (id, text, updated_at) VALUES (?1, ?2, ?3)",
            )?;
            for e in entries {
                stmt.execute(rusqlite::params![e.id, e.text, e.updated_at as i64])?;
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
            "SELECT id, role, content, created_at FROM chat_message ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ChatEntry {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 追加一条聊天消息，返回带 id 的完整记录。
    pub fn add_chat_message(&self, role: &str, content: &str) -> Result<ChatEntry> {
        let created_at = now_ms();
        self.conn.execute(
            "INSERT INTO chat_message (role, content, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![role, content, created_at as i64],
        )?;
        Ok(ChatEntry {
            id: self.conn.last_insert_rowid(),
            role: role.to_string(),
            content: content.to_string(),
            created_at,
        })
    }

    /// 删除一条聊天消息。
    pub fn delete_chat_message(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM chat_message WHERE id = ?1", [id])?;
        Ok(())
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
                updated_at: 1000,
            },
            ClipEntry {
                id: "c2".into(),
                text: "新".into(),
                updated_at: 2000,
            },
        ];
        store.save_clipboard(&entries).unwrap();
        let loaded = store.get_clipboard().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "c2");
        assert_eq!(loaded[1].id, "c1");
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

        let m1 = store.add_chat_message("user", "你好").unwrap();
        let m2 = store.add_chat_message("assistant", "你好！").unwrap();
        assert!(m1.id < m2.id);
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].content, "你好");
        assert_eq!(list[1].role, "assistant");

        store.delete_chat_message(m1.id).unwrap();
        let list = store.list_chat_messages().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, m2.id);

        store.clear_chat_messages().unwrap();
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
