use deskpet_store::ClipEntry;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const MAX_ENTRIES: usize = 100;
const MAX_CHARS: usize = 4000;
const POLL_INTERVAL_MS: u64 = 1000;

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
    let snapshot = app.state::<AppState>().clipboard.lock().unwrap().clone();
    app.state::<AppState>()
        .store
        .lock()
        .unwrap()
        .save_clipboard(&snapshot)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_clipboard_history(app: AppHandle) -> Vec<ClipEntry> {
    app.state::<AppState>().clipboard.lock().unwrap().clone()
}

#[tauri::command]
pub fn clear_clipboard_history(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>().clipboard.lock().unwrap().clear();
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
    deskpet_os::clipboard::copy_text(&text).map_err(|e| e.to_string())
}

/// 启动剪贴板轮询线程：每秒读取一次系统剪贴板，捕获新文本并广播事件。
pub fn start_poll(app: AppHandle) {
    // 启动时载入持久化的历史（SQLite）
    {
        let state = app.state::<AppState>();
        let history = state
            .store
            .lock()
            .unwrap()
            .get_clipboard()
            .unwrap_or_default();
        *state.clipboard.lock().unwrap() = history;
    }

    std::thread::spawn(move || {
        let mut board = match deskpet_os::clipboard::Clipboard::new() {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut last_text: Option<String> = None;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            let Ok(text) = board.read_text() else {
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
