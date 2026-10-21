use arboard::Clipboard;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use super::config::{read_json, write_json};
use crate::state::AppState;

const MAX_ENTRIES: usize = 100;
const MAX_CHARS: usize = 4000;
const CLIPBOARD_FILE: &str = "clipboard.json";
const POLL_INTERVAL_MS: u64 = 1000;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClipEntry {
    pub id: String,
    pub text: String,
    pub updated_at: u64,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn persist(app: &AppHandle) -> Result<(), String> {
    let snapshot = app
        .state::<AppState>()
        .clipboard
        .lock()
        .unwrap()
        .clone();
    write_json(app, CLIPBOARD_FILE, &snapshot)
}

#[tauri::command]
pub fn get_clipboard_history(app: AppHandle) -> Vec<ClipEntry> {
    app.state::<AppState>()
        .clipboard
        .lock()
        .unwrap()
        .clone()
}

#[tauri::command]
pub fn clear_clipboard_history(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .clipboard
        .lock()
        .unwrap()
        .clear();
    persist(&app)
}

#[tauri::command]
pub fn delete_clipboard_entry(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        state.clipboard.lock().unwrap().retain(|e| e.id != id);
    }
    persist(&app)
}

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), String> {
    let mut board = Clipboard::new().map_err(|e| format!("无法访问剪贴板: {e}"))?;
    board
        .set_text(text)
        .map_err(|e| format!("写入剪贴板失败: {e}"))
}

/// 启动剪贴板轮询线程：每秒读取一次系统剪贴板，捕获新文本并广播事件。
pub fn start_poll(app: AppHandle) {
    // 启动时载入持久化的历史
    {
        let history: Vec<ClipEntry> = read_json(&app, CLIPBOARD_FILE).unwrap_or_default();
        let state = app.state::<AppState>();
        *state.clipboard.lock().unwrap() = history;
    }

    std::thread::spawn(move || {
        let mut board = match Clipboard::new() {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut last_text: Option<String> = None;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            let Ok(text) = board.get_text() else {
                continue;
            };
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            if last_text.as_deref() == Some(text.as_str()) {
                continue;
            }
            last_text = Some(text.clone());

            let stored = truncate_chars(&text, MAX_CHARS);
            {
                let state = app.state::<AppState>();
                let mut list = state.clipboard.lock().unwrap();
                // 顶部去重：与最新一条相同则跳过
                if list.first().map(|e| e.text.as_str()) == Some(stored.as_str()) {
                    continue;
                }
                // 全局去重：相同内容的历史条目上移为新条目
                list.retain(|e| e.text != stored);
                let entry = ClipEntry {
                    id: format!("c-{}", now_millis()),
                    text: stored,
                    updated_at: now_millis(),
                };
                list.insert(0, entry);
                if list.len() > MAX_ENTRIES {
                    list.truncate(MAX_ENTRIES);
                }
            }
            let _ = persist(&app);
            let _ = app.emit("clipboard://changed", ());
        }
    });
}
