use rusqlite::Connection;
use std::fs;
use std::path::Path;

use crate::Result;
use crate::models::{ClipEntry, LauncherConfig, Note};

/// 表为空且存在旧 JSON 文件时导入，导入成功后将 JSON 重命名为 .bak 备份。
pub fn migrate_json(conn: &Connection, dir: &Path) -> Result<()> {
    let json_path = |name: &str| dir.join(format!("{name}.json"));

    // 便签
    let note_count: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
    if note_count == 0
        && let Ok(list) = read_json_file::<Vec<Note>>(&json_path("notes"))
    {
        let tx = conn.unchecked_transaction()?;
        for n in &list {
            tx.execute(
                "INSERT OR REPLACE INTO notes (id, content, color, updated_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![n.id, n.content, n.color, n.updated_at as i64],
            )?;
        }
        tx.commit()?;
        backup_json(&json_path("notes"));
    }

    // 剪贴板
    let clip_count: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard", [], |r| r.get(0))?;
    if clip_count == 0
        && let Ok(list) = read_json_file::<Vec<ClipEntry>>(&json_path("clipboard"))
    {
        let tx = conn.unchecked_transaction()?;
        for e in &list {
            tx.execute(
                "INSERT OR REPLACE INTO clipboard (id, text, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![e.id, e.text, e.updated_at as i64],
            )?;
        }
        tx.commit()?;
        backup_json(&json_path("clipboard"));
    }

    // 启动器
    let launcher_count: i64 = conn.query_row("SELECT COUNT(*) FROM launcher", [], |r| r.get(0))?;
    if launcher_count == 0
        && let Ok(cfg) = read_json_file::<LauncherConfig>(&json_path("launcher"))
    {
        let tx = conn.unchecked_transaction()?;
        for (i, item) in cfg.items.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO launcher (id, name, icon, item_type, value, color, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    item.id,
                    item.name,
                    item.icon,
                    item.item_type,
                    item.value,
                    item.color,
                    i as i64
                ],
            )?;
        }
        tx.commit()?;
        backup_json(&json_path("launcher"));
    }

    Ok(())
}

/// 读取并反序列化 JSON 文件。
fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn backup_json(path: &Path) {
    let _ = fs::rename(path, path.with_extension("json.bak"));
}
