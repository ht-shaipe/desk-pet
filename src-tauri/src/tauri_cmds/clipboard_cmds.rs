use deskpet_store::ClipEntry;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const MAX_ENTRIES: usize = 500;
const MAX_CHARS: usize = 4000;
const POLL_INTERVAL_MS: u64 = 1000;
/// 图片 RGBA 字节数上限（约 12MB，超出跳过）
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

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

/// FNV-1a 64 位哈希（图片内容签名，跨进程/重启稳定）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
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

/// 删除磁盘上的剪贴板图片文件（条目被删除/截断/清空时调用）。
fn cleanup_image_files(app: &AppHandle, filenames: &[String]) {
    let Ok(dir) = crate::paths::data_dir(app) else {
        return;
    };
    let img_dir = dir.join("clip_images");
    for name in filenames {
        let _ = std::fs::remove_file(img_dir.join(name));
    }
}

#[tauri::command]
pub fn get_clipboard_history(app: AppHandle) -> Vec<ClipEntry> {
    app.state::<AppState>().clipboard.lock().unwrap().clone()
}

#[tauri::command]
pub fn clear_clipboard_history(app: AppHandle) -> Result<(), String> {
    let image_files: Vec<String> = {
        let state = app.state::<AppState>();
        state
            .clipboard
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.kind == "image")
            .map(|e| e.text.clone())
            .collect()
    };
    cleanup_image_files(&app, &image_files);
    app.state::<AppState>().clipboard.lock().unwrap().clear();
    persist(&app)
}

#[tauri::command]
pub fn delete_clipboard_entry(app: AppHandle, id: String) -> Result<(), String> {
    let image_files: Vec<String> = {
        let state = app.state::<AppState>();
        state
            .clipboard
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.id == id && e.kind == "image")
            .map(|e| e.text.clone())
            .collect()
    };
    cleanup_image_files(&app, &image_files);
    {
        let state = app.state::<AppState>();
        state.clipboard.lock().unwrap().retain(|e| e.id != id);
    }
    persist(&app)
}

/// 批量删除剪贴板条目（按类型/日期条件清空时一次提交，避免逐条全量落库）。
#[tauri::command]
pub fn delete_clipboard_entries(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let image_files: Vec<String> = {
        let state = app.state::<AppState>();
        let list = state.clipboard.lock().unwrap();
        let id_set: std::collections::HashSet<&String> = ids.iter().collect();
        list.iter()
            .filter(|e| id_set.contains(&e.id) && e.kind == "image")
            .map(|e| e.text.clone())
            .collect()
    };
    cleanup_image_files(&app, &image_files);
    {
        let state = app.state::<AppState>();
        let id_set: std::collections::HashSet<&String> = ids.iter().collect();
        state
            .clipboard
            .lock()
            .unwrap()
            .retain(|e| !id_set.contains(&e.id));
    }
    persist(&app)
}

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), String> {
    deskpet_os::clipboard::copy_text(&text).map_err(|e| e.to_string())
}

/// 将 PNG 图片复制到系统剪贴板（按条目 id 查找文件）。
#[tauri::command]
pub fn copy_image_to_clipboard(app: AppHandle, filename: String) -> Result<(), String> {
    let dir = crate::paths::data_dir(&app)?;
    let path = dir.join("clip_images").join(&filename);
    let payload = deskpet_os::clipboard::read_png(&path)
        .map_err(|e| e.to_string())?;
    let mut board = deskpet_os::clipboard::Clipboard::new().map_err(|e| e.to_string())?;
    board.write_image(&payload).map_err(|e| e.to_string())
}

/// 返回剪贴板图片目录的完整路径（前端用 convertFileSrc 拼接 asset URL）。
#[tauri::command]
pub fn get_clip_image_dir(app: AppHandle) -> Result<String, String> {
    let dir = crate::paths::data_dir(&app)?;
    Ok(dir.join("clip_images").to_string_lossy().to_string())
}

/// 启动剪贴板轮询线程：每秒读取一次系统剪贴板，捕获新文本/图片并广播事件。
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
        let mut last_image_sig: Option<u64> = None;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            // 先尝试读文本；空文本或读取失败时 fall through 到图片检测
            let text = board
                .read_text()
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());

            if let Some(text) = text {
                if last_text.as_deref() == Some(text.as_str()) {
                    continue;
                }
                last_text = Some(text.clone());
                last_image_sig = None;

                let stored = truncate_chars(&text, MAX_CHARS);
                let orphan_imgs: Vec<String> = {
                    let state = app.state::<AppState>();
                    let mut list = state.clipboard.lock().unwrap();
                    if list.first().map(|e| e.text.as_str()) == Some(stored.as_str()) {
                        continue;
                    }
                    // 全局去重：相同内容的历史条目上移为新条目，并累计复制次数
                    let prev = list.iter().position(|e| e.kind == "text" && e.text == stored);
                    let count = prev.map_or(1, |i| list[i].copy_count.saturating_add(1));
                    if let Some(i) = prev {
                        list.remove(i);
                    }
                    let entry = ClipEntry {
                        id: format!("c-{}", now_millis()),
                        text: stored,
                        kind: "text".into(),
                        updated_at: now_millis(),
                        copy_count: count,
                        sig: String::new(),
                    };
                    list.insert(0, entry);
                    if list.len() > MAX_ENTRIES {
                        let orphans: Vec<String> = list[MAX_ENTRIES..]
                            .iter()
                            .filter(|e| e.kind == "image")
                            .map(|e| e.text.clone())
                            .collect();
                        list.truncate(MAX_ENTRIES);
                        orphans
                    } else {
                        Vec::new()
                    }
                };
                cleanup_image_files(&app, &orphan_imgs);
                let _ = persist(&app);
                let _ = app.emit("clipboard://changed", ());
                continue;
            }

            // 文本为空或读取失败，尝试读图片
            let Ok(Some(img)) = board.read_image() else {
                continue;
            };
            if img.rgba.len() > MAX_IMAGE_BYTES {
                continue;
            }
            // 内容级签名：对解码后的 RGBA 字节做哈希（宽高/字节数会误判同尺寸不同图）
            let sig = fnv1a64(&img.rgba);
            if last_image_sig == Some(sig) {
                continue;
            }
            last_image_sig = Some(sig);
            last_text = None;
            let sig_str = format!("fnv64:{sig:016x}");

            // 历史同签名条目：命中则复用其 id/文件名（缩略图不失效）并累计次数
            let (entry_id, filename, count) = {
                let state = app.state::<AppState>();
                let list = state.clipboard.lock().unwrap();
                match list.iter().position(|e| e.kind == "image" && e.sig == sig_str) {
                    Some(i) => (
                        list[i].id.clone(),
                        list[i].text.clone(),
                        list[i].copy_count.saturating_add(1),
                    ),
                    None => {
                        let filename = format!("c-{}.png", now_millis());
                        (filename[..filename.len() - 4].to_string(), filename, 1)
                    }
                }
            };

            // 保存 PNG 到 app_data_dir/clip_images/（复用旧文件名时原地覆盖）
            {
                let Ok(dir) = crate::paths::data_dir(&app) else {
                    continue;
                };
                let img_dir = dir.join("clip_images");
                let _ = std::fs::create_dir_all(&img_dir);
                let path = img_dir.join(&filename);
                if deskpet_os::clipboard::save_png(&path, &img).is_err() {
                    continue;
                }
            }

            let orphan_imgs: Vec<String> = {
                let state = app.state::<AppState>();
                let mut list = state.clipboard.lock().unwrap();
                // 图片内容级去重：同签名旧条目上移为新条目
                list.retain(|e| !(e.kind == "image" && e.sig == sig_str));
                let entry = ClipEntry {
                    id: entry_id,
                    text: filename,
                    kind: "image".into(),
                    updated_at: now_millis(),
                    copy_count: count,
                    sig: sig_str,
                };
                list.insert(0, entry);
                if list.len() > MAX_ENTRIES {
                    let orphans: Vec<String> = list[MAX_ENTRIES..]
                        .iter()
                        .filter(|e| e.kind == "image")
                        .map(|e| e.text.clone())
                        .collect();
                    list.truncate(MAX_ENTRIES);
                    orphans
                } else {
                    Vec::new()
                }
            };
            cleanup_image_files(&app, &orphan_imgs);
            let _ = persist(&app);
            let _ = app.emit("clipboard://changed", ());
        }
    });
}
