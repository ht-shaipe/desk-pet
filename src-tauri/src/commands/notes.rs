use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: String,
    pub content: String,
    pub color: String,
    pub updated_at: u64,
}

#[tauri::command]
pub fn get_notes(app: AppHandle) -> Vec<Note> {
    let state = app.state::<crate::state::AppState>();
    let conn = state.db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT id, content, color, updated_at FROM notes ORDER BY rowid",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| {
        Ok(Note {
            id: row.get(0)?,
            content: row.get(1)?,
            color: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
        })
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
pub fn save_notes(app: AppHandle, notes: Vec<Note>) -> Result<(), String> {
    let state = app.state::<crate::state::AppState>();
    let conn = state.db.lock().unwrap();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM notes", [])
        .map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO notes (id, content, color, updated_at) VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| e.to_string())?;
        for n in &notes {
            stmt.execute(rusqlite::params![n.id, n.content, n.color, n.updated_at as i64])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}
