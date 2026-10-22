use rusqlite::Connection;
use std::path::Path;

use crate::Result;
use crate::migrate::migrate_json;
use crate::models::{ChatEntry, ClipEntry, LauncherConfig, LauncherItem, Note};

const DB_FILE: &str = "deskpet.db";

/// 当前 Unix 时间戳（毫秒）。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
        Ok(Self { conn })
    }

    /// 全部便签（按写入顺序）。
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

    /// 全量替换便签。
    pub fn save_notes(&self, notes: &[Note]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM notes", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO notes (id, content, color, updated_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for n in notes {
                stmt.execute(rusqlite::params![
                    n.id,
                    n.content,
                    n.color,
                    n.updated_at as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
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
    fn notes_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(store.get_notes().unwrap().is_empty());

        store.save_notes(&sample_notes()).unwrap();
        assert_eq!(store.get_notes().unwrap(), sample_notes());

        // 全量替换
        store.save_notes(&sample_notes()[..1]).unwrap();
        assert_eq!(store.get_notes().unwrap().len(), 1);
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
    fn migrates_legacy_json_when_tables_empty() {
        let dir = tempfile::tempdir().unwrap();
        // 预置旧版 JSON 数据
        std::fs::write(
            dir.path().join("notes.json"),
            serde_json::to_string(&sample_notes()).unwrap(),
        )
        .unwrap();

        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.get_notes().unwrap(), sample_notes());
        // 导入成功后 JSON 备份为 .bak
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
