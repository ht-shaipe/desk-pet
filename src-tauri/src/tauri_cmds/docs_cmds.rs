//! 文档管理:独立窗口、文档目录增量扫描、索引查询、移废纸篓删除。
//! 与图库(images_*)完全分表分库逻辑,互不干扰;Markdown 转换复用 document_convert(anydoc)。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use walkdir::WalkDir;

use crate::state::AppState;

/// 索引的文档扩展名。
const DOC_EXTS: &[&str] = &[
    "pdf", "doc", "docx", "ppt", "pptx", "xls", "xlsx", "odt", "ods", "odp", "rtf", "epub", "csv",
    "md", "txt",
];

/* ---------------- 窗口 ---------------- */

/// 打开(或聚焦)文档管理窗口,返回是否为新创建。
#[tauri::command]
pub async fn docs_open(app: AppHandle) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            "docs",
            WebviewUrl::App("index.html?view=docs".into()),
        )
        .title("文档管理")
        .inner_size(1100.0, 760.0)
        .min_inner_size(860.0, 600.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .build()
        .is_ok();
        if let Some(window) = app_for_window.get_webview_window("docs") {
            #[cfg(target_os = "macos")]
            if let Ok(ns_window) = window.ns_window() {
                unsafe {
                    deskpet_os::window::round_window(ns_window, 12.0);
                }
            }
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = tx.send(created);
    })
    .map_err(|err| err.to_string())?;
    rx.await.map_err(|err| err.to_string())
}

/* ---------------- 文档目录 ---------------- */

#[tauri::command]
pub async fn docs_add_dir(app: AppHandle, path: String) -> Result<deskpet_store::DocDir, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = path.trim().to_string();
        if trimmed.is_empty() {
            return Err("目录不能为空".into());
        }
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.add_doc_dir(&trimmed).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

#[tauri::command]
pub async fn docs_list_dirs(app: AppHandle) -> Result<Vec<deskpet_store::DocDir>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.list_doc_dirs().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[tauri::command]
pub async fn docs_remove_dir(app: AppHandle, id: i64) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.remove_doc_dir(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

/* ---------------- 扫描 ---------------- */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DocsScanProgress {
    dir_id: i64,
    scanned: u64,
    added: u64,
    updated: u64,
    removed: u64,
    done: bool,
}

/// 启动某文档目录的增量扫描(后台线程,进度经 docs://scan-progress 推送)。
#[tauri::command]
pub async fn docs_scan_start(app: AppHandle, dir_id: i64) -> Result<(), String> {
    let root = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let dirs = store.list_doc_dirs().map_err(|e| e.to_string())?;
        match dirs.into_iter().find(|d| d.id == dir_id) {
            Some(d) => d.path,
            None => return Err("文档目录不存在".into()),
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut tasks = state.doc_tasks.lock().unwrap();
        if tasks.contains_key(&dir_id) {
            return Err("该目录扫描进行中".into());
        }
        tasks.insert(dir_id, cancel.clone());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        docs_scan_worker(app2, dir_id, PathBuf::from(root), cancel);
    });
    Ok(())
}

/// 取消进行中的文档扫描。
#[tauri::command]
pub fn docs_task_cancel(app: AppHandle, dir_id: i64) -> Result<(), String> {
    if let Some(cancel) = app.state::<AppState>().doc_tasks.lock().unwrap().get(&dir_id) {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn docs_scan_worker(app: AppHandle, dir_id: i64, root: PathBuf, cancel: Arc<AtomicBool>) {
    let mut seen: HashSet<String> = HashSet::new();
    let (mut scanned, mut added, mut updated) = (0u64, 0u64, 0u64);
    let mut emit_tick = 0u64;

    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let p = e.path();
            if p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                return false;
            }
            true
        });

    for entry in walker.flatten() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        else {
            continue;
        };
        if !DOC_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        let (size, mtime) = (meta.len(), mtime_secs(&meta));
        let path_str = path.to_string_lossy().to_string();
        seen.insert(path_str.clone());
        scanned += 1;

        // 增量:size+mtime 未变直接跳过
        let stamp = {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            store.get_doc_item_stamp(&path_str).ok().flatten()
        };
        if let Some((id, old_size, old_mtime)) = stamp {
            if old_size == size && old_mtime == mtime {
                continue;
            }
            // 内容变化:删旧入新
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            let _ = store.delete_doc_item(id);
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let m = deskpet_store::DocItemMeta {
            dir_id,
            path: path_str.clone(),
            name,
            ext,
            size,
            mtime,
        };
        let created = {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            store.add_doc_item(&m).is_ok()
        };
        if created {
            added += 1;
        } else {
            updated += 1;
        }

        emit_tick += 1;
        if emit_tick % 20 == 0 {
            let _ = app.emit_to(
                "docs",
                "docs://scan-progress",
                DocsScanProgress {
                    dir_id,
                    scanned,
                    added,
                    updated,
                    removed: 0,
                    done: false,
                },
            );
        }
    }

    // 清理磁盘上已消失的条目
    // 取消时 seen 不完整,跳过清理,防止误删未扫到的既有索引
    let mut removed = 0u64;
    if !cancel.load(Ordering::Relaxed) {
        {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            if let Ok((items, _)) = store.query_doc_items(&deskpet_store::DocQuery {
                dir_id: Some(dir_id),
                limit: u32::MAX / 2,
                ..Default::default()
            }) {
                for item in items {
                    if !seen.contains(&item.path) {
                        let _ = store.delete_doc_item(item.id);
                        removed += 1;
                    }
                }
            }
            let _ = store.touch_doc_dir_scan(dir_id);
        }
    }

    let _ = app.emit_to(
        "docs",
        "docs://scan-progress",
        DocsScanProgress {
            dir_id,
            scanned,
            added,
            updated,
            removed,
            done: true,
        },
    );
    let state = app.state::<AppState>();
    state.doc_tasks.lock().unwrap().remove(&dir_id);
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/* ---------------- 查询 / 删除 ---------------- */

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocQueryDto {
    pub dir_id: Option<i64>,
    pub search: String,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub limit: u32,
    pub offset: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPage {
    pub items: Vec<deskpet_store::DocItem>,
    pub has_more: bool,
    pub total: u64,
}

#[tauri::command]
pub async fn docs_query(app: AppHandle, query: DocQueryDto) -> Result<DocPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let q = deskpet_store::DocQuery {
            dir_id: query.dir_id,
            search: query.search,
            sort_by: query.sort_by,
            sort_order: query.sort_order,
            limit: query.limit,
            offset: query.offset,
        };
        let (items, has_more) = store.query_doc_items(&q).map_err(|e| e.to_string())?;
        let total = store.count_doc_items(&q).map_err(|e| e.to_string())?;
        Ok(DocPage {
            items,
            has_more,
            total,
        })
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

/// 删除文档:移废纸篓 + 清索引。文件已不存在时视为成功(仅清索引)。
#[tauri::command]
pub async fn docs_delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            let item = store
                .get_doc_item(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "条目不存在".to_string())?;
            item.path
        };
        if Path::new(&path).exists() {
            trash::delete(&path).map_err(|e| format!("移入废纸篓失败: {e}"))?;
        }
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.delete_doc_item(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("删除失败: {e}"))?
}
