//! 图片管理器:独立窗口、图库目录增量扫描、缩略图缓存、img:// 协议数据源、
//! 标签与收藏、精确去重(同尺寸 → blake3 → 分组)。
//! 架构参考 lap:索引/元数据进 SQLite,缩略图落盘缓存,图片经自定义协议 + WebView 缓存展示。

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fast_image_resize::FilterType;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use walkdir::WalkDir;

use crate::paths::data_dir;
use crate::state::AppState;

/// 支持解码的照片扩展名(HEIC/RAW 暂不支持,二期)。
const PHOTO_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];
/// 全盘扫描跳过的系统目录。
const FULL_DISK_SKIPS: &[&str] = &[
    "/System",
    "/Library",
    "/private",
    "/Applications",
    "/Volumes",
    "/dev",
    "/opt",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/var",
    "C:\\Windows",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    "C:\\ProgramData",
];
/// 缩略图长边上限。
const THUMB_EDGE: u32 = 512;
/// 去重专用任务 key(与 dir_id 空间隔离)。
const DEDUP_TASK_KEY: i64 = -1;

/// 协议响应字节类型。
type CowBytes = std::borrow::Cow<'static, [u8]>;



/* ---------------- 窗口 ---------------- */

/// 打开(或聚焦)图片管理器窗口,返回是否为新创建。
/// view: "compress" 直落压缩视图,其余落照片浏览;窗口已开时经事件切换视图。
#[tauri::command]
pub async fn images_open(app: AppHandle, view: Option<String>) -> Result<bool, String> {
    let tab = view.unwrap_or_default();
    let url = if tab.is_empty() {
        "index.html?view=images".to_string()
    } else {
        format!("index.html?view=images&tab={tab}")
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            "images",
            WebviewUrl::App(url.into()),
        )
        .title("资料库")
        .inner_size(1100.0, 760.0)
        .min_inner_size(860.0, 600.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .build()
        .is_ok();
        // 窗口已开:按需切换到目标视图
        if !created {
            if let Some(window) = app_for_window.get_webview_window("images") {
                if !tab.is_empty() {
                    use tauri::Emitter;
                    let _ = window.emit_to("images", "images://set-view", tab.clone());
                }
            }
        }
        // 隐藏 Dock(accessory)模式下新窗口不会自动前置,显式拉起
        if let Some(window) = app_for_window.get_webview_window("images") {
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

/* ---------------- 图库目录 ---------------- */

#[tauri::command]
pub fn images_add_dir(
    app: AppHandle,
    path: String,
) -> Result<deskpet_store::ImageDir, String> {
    let trimmed = path.trim().to_string();
    if trimmed.is_empty() {
        return Err("目录不能为空".into());
    }
    // 根路径（"/"、"C:\"）自动按全盘目录处理：扫描时跳过系统目录
    let full_disk = Path::new(&trimmed).parent().is_none();
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    store
        .add_image_dir(&trimmed, full_disk)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn images_list_dirs(app: AppHandle) -> Result<Vec<deskpet_store::ImageDir>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.list_image_dirs().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[tauri::command]
pub fn images_remove_dir(app: AppHandle, id: i64) -> Result<(), String> {
    let thumbs = {
        let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
        store.remove_image_dir(id).map_err(|e| e.to_string())?
    };
    remove_thumb_files(&data_dir(&app).map_err(|e| e.to_string())?, &thumbs);
    Ok(())
}

/* ---------------- 扫描 ---------------- */

/// 启动某图库目录的增量扫描(后台线程,进度经 images://scan-progress 事件推送)。
#[tauri::command]
pub async fn images_scan_start(app: AppHandle, dir_id: i64) -> Result<(), String> {
    let (root, full_disk) = {
        let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
        let dirs = store.list_image_dirs().map_err(|e| e.to_string())?;
        match dirs.into_iter().find(|d| d.id == dir_id) {
            Some(d) => (d.path, d.full_disk),
            None => return Err("图库目录不存在".into()),
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut tasks = state.image_tasks.lock().unwrap();
        if tasks.contains_key(&dir_id) {
            return Err("该目录扫描进行中".into());
        }
        tasks.insert(dir_id, cancel.clone());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        scan_worker(app2, dir_id, PathBuf::from(&root), full_disk, cancel);
    });
    Ok(())
}

/// 取消进行中的任务(扫描/去重共用)。
#[tauri::command]
pub fn images_task_cancel(app: AppHandle, task_key: i64) -> Result<(), String> {
    if let Some(cancel) = app.state::<AppState>().image_tasks.lock().unwrap().get(&task_key) {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    dir_id: i64,
    scanned: u64,
    added: u64,
    updated: u64,
    removed: u64,
    /// 缩略图生成进度(并行阶段)
    thumbs_done: u64,
    thumbs_total: u64,
    done: bool,
}

fn scan_worker(app: AppHandle, dir_id: i64, root: PathBuf, full_disk: bool, cancel: Arc<AtomicBool>) {
    let data_dir = match data_dir(&app) {
        Ok(d) => d,
        Err(_) => return,
    };
    let mut seen: HashSet<String> = HashSet::new();
    let (mut scanned, mut added, mut updated) = (0u64, 0u64, 0u64);
    let mut emit_tick = 0u64;
    // 待生成缩略图的条目(id, path):索引段收集、并行段消费
    let queue = Mutex::new(VecDeque::<(i64, String)>::new());

    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let p = e.path();
            // 隐藏目录/文件一律跳过
            if p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                return false;
            }
            // 全盘模式跳过系统目录(POSIX 按首组件忽略大小写;Windows 按前缀组件)
            if full_disk && p.is_dir() {
                #[cfg(target_os = "windows")]
                let skip = FULL_DISK_SKIPS.iter().any(|s| p.starts_with(s));
                #[cfg(not(target_os = "windows"))]
                let skip = {
                    let first = p.components().next().and_then(|c| c.as_os_str().to_str());
                    first.is_some_and(|f| FULL_DISK_SKIPS.iter().any(|s| f.eq_ignore_ascii_case(s)))
                };
                if skip {
                    return false;
                }
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
        if !PHOTO_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        let (size, mtime) = (meta.len(), mtime_secs(&meta));
        let path_str = path.to_string_lossy().to_string();
        seen.insert(path_str.clone());
        scanned += 1;

        // 增量:size+mtime 未变直接跳过(不解码)
        let stamp = {
            let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
            store.get_image_item_stamp(&path_str).ok().flatten()
        };
        if let Some((_, old_size, old_mtime)) = stamp {
            if old_size == size && old_mtime == mtime {
                continue;
            }
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let (width, height, taken_at, camera) = photo_meta(path, mtime);
        let m = deskpet_store::ImageItemMeta {
            dir_id,
            path: path_str.clone(),
            name,
            ext,
            size,
            mtime,
            width,
            height,
            taken_at,
            camera,
            doc: false,
        };
        // 一次锁内完成入库并取回条目,减少与 UI 线程的锁争用
        let outcome = {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            match store.upsert_image_item(&m) {
                Ok((id, created)) => {
                    let item = store.get_image_item(id).ok().flatten();
                    Some((id, created, item))
                }
                Err(_) => None,
            }
        };
        let Some((item_id, created, item)) = outcome else {
            continue;
        };
        let _ = item;
        if created {
            added += 1;
        } else {
            updated += 1;
        }
        // 缩略图延后到并行阶段生成,索引段只做元数据,保持快
        queue.lock().unwrap().push_back((item_id, path_str.clone()));

        emit_tick += 1;
        if emit_tick % 20 == 0 {
            let _ = app.emit_to(
                "images",
                "images://scan-progress",
                ScanProgress {
                    dir_id,
                    scanned,
                    added,
                    updated,
                    removed: 0,
                    thumbs_done: 0,
                    thumbs_total: queue.lock().unwrap().len() as u64,
                    done: false,
                },
            );
        }
    }

    // 清理已从磁盘消失的条目(只删索引与缩略图缓存,不动用户文件)。
    // 取消时 seen 不完整,跳过清理,否则会把未扫到的既有索引误删!
    let mut removed = 0u64;
    let mut stale_thumbs: Vec<String> = Vec::new();
    if !cancel.load(Ordering::Relaxed) {
        {
            let state = app.state::<AppState>();
            let store = state.store.lock().unwrap();
            if let Ok((items, _)) = store.query_image_items(&deskpet_store::ImageQuery {
                dir_id: Some(dir_id),
                limit: u32::MAX / 2,
                ..Default::default()
            }) {
                for item in items {
                    if !seen.contains(&item.path) {
                        if !item.thumb.is_empty() {
                            stale_thumbs.push(item.thumb);
                        }
                        let _ = store.delete_image_item(item.id);
                        removed += 1;
                    }
                }
            }
            let _ = store.touch_image_dir_scan(dir_id);
        }
        remove_thumb_files(&data_dir, &stale_thumbs);
    }

    // 并行缩略图阶段:索引已完成可浏览,缩略图后台多线程补齐
    let thumbs_total = queue.lock().unwrap().len() as u64;
    let thumbs_done = Arc::new(AtomicU64::new(0));
    if thumbs_total > 0 {
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 6);
        let app_ref = &app;
        let data_ref = &data_dir;
        let queue_ref = &queue;
        let done_ref = &thumbs_done;
        let cancel_ref = &cancel;
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(move || loop {
                    if cancel_ref.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some((item_id, path)) = queue_ref.lock().unwrap().pop_front() else {
                        return;
                    };
                    let _ = path;
                    let item = {
                        let state = app_ref.state::<AppState>();
                        let store = state.store.lock().unwrap();
                        store.get_image_item(item_id).ok().flatten()
                    };
                    let Some(item) = item else { continue };
                    if let Ok(rel) = ensure_thumbnail(data_ref, &item) {
                        let state = app_ref.state::<AppState>();
                        let store = state.store.lock().unwrap();
                        if store.set_image_thumb(item_id, &rel).is_ok() {
                            let _ = app_ref.emit_to("images", "images://thumb-ready", ThumbReady { id: item_id });
                        }
                    }
                    let n = done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 4 == 0 || n == thumbs_total {
                        let _ = app_ref.emit_to(
                            "images",
                            "images://scan-progress",
                            ScanProgress {
                                dir_id,
                                scanned,
                                added,
                                updated,
                                removed: 0,
                                thumbs_done: n,
                                thumbs_total,
                                done: false,
                            },
                        );
                    }
                });
            }
        });
    }

    let _ = app.emit_to(
        "images",
        "images://scan-progress",
        ScanProgress {
            dir_id,
            scanned,
            added,
            updated,
            removed,
            thumbs_done: thumbs_done.load(Ordering::Relaxed),
            thumbs_total,
            done: true,
        },
    );
    let state = app.state::<AppState>();
    state.image_tasks.lock().unwrap().remove(&dir_id);
}

/// 缩略图生成完成事件(前端据此刷新对应网格图)。
#[derive(Serialize, Clone)]
struct ThumbReady {
    id: i64,
}

/// 照片元数据:宽高(仅读文件头)+ EXIF 拍摄时间/相机(宽容解析,失败回退 mtime)。
fn photo_meta(path: &Path, mtime: u64) -> (u32, u32, u64, String) {
    let (mut width, mut height) = (0u32, 0u32);
    if let Ok(file) = fs::File::open(path) {
        // into_dimensions 只读文件头取宽高,不解码像素
        if let Ok(reader) = image::ImageReader::new(BufReader::new(file)).with_guessed_format() {
            if let Ok(dims) = reader.into_dimensions() {
                width = dims.0;
                height = dims.1;
            }
        }
        if let Ok(file2) = fs::File::open(path) {
            if let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file2)) {
                if let Some(secs) = exif_taken_at(&exif) {
                    return (width, height, secs, exif_camera(&exif));
                }
                return (width, height, mtime, exif_camera(&exif));
            }
        }
    }
    (width, height, mtime, String::new())
}

/// EXIF DateTimeOriginal → Unix 秒(EXIF 无时区,按 UTC 折算,仅用于分组排序)。
fn exif_taken_at(exif: &exif::Exif) -> Option<u64> {
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let exif::Value::Ascii(parts) = &field.value else {
        return None;
    };
    let Some(first) = parts.first() else {
        return None;
    };
    let text = String::from_utf8_lossy(first);
    // "YYYY:MM:DD HH:MM:SS"
    let b = text.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |s: &[u8]| -> Option<i64> { std::str::from_utf8(s).ok()?.trim().parse().ok() };
    let (y, mo, d) = (num(&b[0..4])?, num(&b[5..7])?, num(&b[8..10])?);
    let (h, mi, s) = (num(&b[11..13])?, num(&b[14..16])?, num(&b[17..19])?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some(civil_to_epoch(y, mo, d, h, mi, s))
}

fn exif_camera(exif: &exif::Exif) -> String {
    let get = |tag: exif::Tag| -> String {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| match &f.value {
                exif::Value::Ascii(parts) => {
                    parts.first().map(|b| String::from_utf8_lossy(b).to_string())
                }
                _ => None,
            })
            .unwrap_or_default()
            .trim()
            .trim_end_matches('\u{0}')
            .to_string()
    };
    let (make, model) = (get(exif::Tag::Make), get(exif::Tag::Model));
    match (make.is_empty(), model.is_empty()) {
        (true, true) => String::new(),
        (true, false) => model,
        (false, false) => format!("{make} {model}"),
        (false, true) => make,
    }
}

/// 天文算法:民用日期时间 → Unix 秒(Howard Hinnant days_from_civil)。
fn civil_to_epoch(y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86_400 + h * 3600 + mi * 60 + s).max(0) as u64
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/* ---------------- 缩略图 ---------------- */

/// 生成(或复用)条目缩略图:长边 512 WebP q80,落盘 data_dir/image_thumbs/{桶}/{id}.webp。
/// 返回相对路径(写入 DB 的 thumb 字段);解码失败返回 Err(前端显示占位图标)。
fn ensure_thumbnail(data_dir: &Path, item: &deskpet_store::ImageItem) -> Result<String, String> {
    if !item.thumb.is_empty()
        && fs::metadata(data_dir.join("image_thumbs").join(&item.thumb)).is_ok()
    {
        return Ok(item.thumb.clone());
    }
    // in-flight 去重:并行扫描线程与 img:// 协议按需生成可能同时命中同一张
    static INFLIGHT: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
    let inflight = INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()));
    if !inflight.lock().unwrap().insert(item.id) {
        return Err("缩略图正在其他线程生成".into());
    }
    let result = build_thumbnail(data_dir, item);
    inflight.lock().unwrap().remove(&item.id);
    result
}

fn build_thumbnail(data_dir: &Path, item: &deskpet_store::ImageItem) -> Result<String, String> {
    let bytes = fs::read(&item.path).map_err(|e| format!("读取原图失败: {e}"))?;
    let img = deskpet_os::imaging::decode_rgba(&bytes).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    let m = w.max(h);
    let resized = if m > THUMB_EDGE {
        let scale = THUMB_EDGE as f64 / m as f64;
        let nw = ((w as f64 * scale).round() as u32).max(1);
        let nh = ((h as f64 * scale).round() as u32).max(1);
        deskpet_os::imaging::resize_rgba(&img, nw, nh, FilterType::Bilinear)
    } else {
        img
    };
    let encoded = deskpet_os::imaging::encode_fmt(&resized, deskpet_os::imaging::EncFmt::Webp, 80)
        .map_err(|e| e.to_string())?;
    let rel = format!("{:02x}/{}.webp", (item.id / 256) % 256, item.id);
    let full = data_dir.join("image_thumbs").join(&rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&full, encoded).map_err(|e| e.to_string())?;
    Ok(rel)
}

fn remove_thumb_files(data_dir: &Path, thumbs: &[String]) {
    for t in thumbs {
        let _ = fs::remove_file(data_dir.join("image_thumbs").join(t));
    }
}

/// 用系统默认程序打开文件。
#[tauri::command]
pub fn images_open_path(path: String) -> Result<(), String> {
    deskpet_os::files::open_path(Path::new(&path));
    Ok(())
}

/* ---------------- 查询 / 详情 / 收藏 / 删除 ---------------- */

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageQueryDto {
    pub dir_id: Option<i64>,
    pub fav_only: bool,
    pub doc_only: bool,
    pub tag_id: Option<i64>,
    pub search: String,
    pub limit: u32,
    pub offset: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePage {
    pub items: Vec<deskpet_store::ImageItem>,
    pub has_more: bool,
    pub total: u64,
}

#[tauri::command]
pub async fn images_query(app: AppHandle, query: ImageQueryDto) -> Result<ImagePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let q = deskpet_store::ImageQuery {
            dir_id: query.dir_id,
            fav_only: query.fav_only,
            doc_only: query.doc_only,
            photo_only: !query.doc_only,
            tag_id: query.tag_id,
            search: query.search,
            limit: query.limit,
            offset: query.offset,
        };
        let (items, has_more) = store.query_image_items(&q).map_err(|e| e.to_string())?;
        let total = store.count_image_items(&q).map_err(|e| e.to_string())?;
        Ok(ImagePage {
            items,
            has_more,
            total,
        })
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDetail {
    pub item: deskpet_store::ImageItem,
    pub tags: Vec<deskpet_store::ImageTag>,
}

#[tauri::command]
pub async fn images_detail(app: AppHandle, id: i64) -> Result<Option<ImageDetail>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let Some(item) = store.get_image_item(id).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let tags = store.get_item_tags(id).map_err(|e| e.to_string())?;
        Ok(Some(ImageDetail { item, tags }))
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[tauri::command]
pub async fn images_set_fav(app: AppHandle, id: i64, fav: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.set_image_fav(id, fav).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

/// 删除单个条目:原图移入废纸篓 + 清索引与缩略图。
#[tauri::command]
pub async fn images_delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    let (path, thumb) = {
        let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
        let item = store
            .get_image_item(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "条目不存在".to_string())?;
        (item.path, item.thumb)
    };
    trash_delete(Path::new(&path))?;
    let thumbs = {
        let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
        store.delete_image_item(id).map_err(|e| e.to_string())?;
        vec![thumb]
    };
    remove_thumb_files(&data_dir(&app).map_err(|e| e.to_string())?, &thumbs);
    Ok(())
}

/* ---------------- 标签 ---------------- */

#[tauri::command]
pub async fn images_list_tags(app: AppHandle) -> Result<Vec<deskpet_store::ImageTag>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.list_image_tags().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[tauri::command]
pub async fn images_add_tag(
    app: AppHandle,
    name: String,
    color: String,
) -> Result<deskpet_store::ImageTag, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("标签名不能为空".into());
        }
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.add_image_tag(&name, &color).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTagPair {
    pub item_id: i64,
    pub tag_id: i64,
}

/// 全部条目-标签关系对(缩略图色点渲染用)。
#[tauri::command]
pub async fn images_item_tag_pairs(app: AppHandle) -> Result<Vec<ItemTagPair>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let pairs = store.list_item_tag_pairs().map_err(|e| e.to_string())?;
        Ok(pairs
            .into_iter()
            .map(|(item_id, tag_id)| ItemTagPair { item_id, tag_id })
            .collect())
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

#[tauri::command]
pub async fn images_rename_tag(
    app: AppHandle,
    id: i64,
    name: String,
    color: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .rename_image_tag(id, name.trim(), &color)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

#[tauri::command]
pub async fn images_delete_tag(app: AppHandle, id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.delete_image_tag(id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

#[tauri::command]
pub async fn images_set_item_tag(
    app: AppHandle,
    item_id: i64,
    tag_id: i64,
    on: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store
            .set_item_tag(item_id, tag_id, on)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("操作失败: {e}"))?
}

/* ---------------- 精确去重 ---------------- */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DedupProgress {
    total: u64,
    done: u64,
    done_flag: bool,
}

/// 启动去重哈希:仅对同尺寸且未算过哈希的文件做 blake3,后台线程执行。
#[tauri::command]
pub async fn images_dedup_start(app: AppHandle) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut tasks = state.image_tasks.lock().unwrap();
        if tasks.contains_key(&DEDUP_TASK_KEY) {
            return Err("去重扫描进行中".into());
        }
        tasks.insert(DEDUP_TASK_KEY, cancel.clone());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let candidates = {
            let state = app2.state::<AppState>();
        let store = state.store.lock().unwrap();
            store.image_hash_candidates().unwrap_or_default()
        };
        let total = candidates.len() as u64;
        let mut done = 0u64;
        for (id, path) in candidates {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(hash) = file_blake3(Path::new(&path)) {
                let state = app2.state::<AppState>();
        let store = state.store.lock().unwrap();
                let _ = store.set_image_hash(id, &hash);
            }
            done += 1;
            if done % 10 == 0 {
                let _ = app2.emit_to(
                    "images",
                    "images://dedup-progress",
                    DedupProgress {
                        total,
                        done,
                        done_flag: false,
                    },
                );
            }
        }
        let _ = app2.emit_to(
            "images",
            "images://dedup-progress",
            DedupProgress {
                total,
                done,
                done_flag: true,
            },
        );
        let state = app2.state::<AppState>();
        state.image_tasks.lock().unwrap().remove(&DEDUP_TASK_KEY);
    });
    Ok(())
}

#[tauri::command]
pub async fn images_dup_groups(
    app: AppHandle,
) -> Result<Vec<deskpet_store::ImageDupGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.image_dup_groups().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("查询失败: {e}"))?
}

/// 批量移废纸篓(重复清理/多选删除),一次性提交全部路径——
/// trash crate 默认走 Finder 删除、每次调用都播一次系统音效,
/// 逐个循环会产生连续提示音;delete_all 合并为一次 Finder 删除,只播一次。
/// 成功后清对应索引与缩略图。
#[tauri::command]
pub async fn images_delete_paths(app: AppHandle, paths: Vec<String>, ids: Vec<i64>) -> Result<(), String> {
    let data = data_dir(&app).map_err(|e| e.to_string())?;
    let mut stale_thumbs = Vec::new();
    trash_delete_all(&paths)?;
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        for id in ids {
            if let Ok(Some(thumb)) = store.delete_image_item(id) {
                stale_thumbs.push(thumb);
            }
        }
    }
    remove_thumb_files(&data, &stale_thumbs);
    Ok(())
}

/* ---------------- img:// 协议 ---------------- */

fn mime_by_ext(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// img:// 协议处理:thumb/{id} 或 full/{id}。路径一律按 id 从 DB 反查,杜绝目录遍历。
/// 注意:调用方须在阻塞线程执行(解码原图是重活,严禁占用主线程)。
pub fn handle_img_protocol(app: &AppHandle, uri: &str) -> tauri::http::Response<CowBytes> {
    use tauri::http::header::{CACHE_CONTROL, CONTENT_TYPE};

    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(CowBytes::Borrowed(b"not found" as &[u8]))
            .unwrap()
    };
    // 兼容 img://localhost/thumb/3 与 Windows 的 http://img.localhost/thumb/3 两种形态
    let (kind, rest) = if let Some(pos) = uri.find("/thumb/") {
        ("thumb", &uri[pos + "/thumb/".len()..])
    } else if let Some(pos) = uri.find("/full/") {
        ("full", &uri[pos + "/full/".len()..])
    } else {
        return not_found();
    };
    let id_str = rest.split(['?', '&', '/']).next().unwrap_or("");
    let Ok(id) = id_str.parse::<i64>() else {
        return not_found();
    };
    let Ok(data_dir) = data_dir(app) else {
        return not_found();
    };
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    let Ok(Some(item)) = store.get_image_item(id) else {
        return not_found();
    };
    drop(store);

    match kind {
        "thumb" => {
            // 扫描进行中不按需生成:流水线会产出并推 thumb-ready 刷新,
            // 避免首次渲染几十个并发解码任务挤占 CPU
            if item.thumb.is_empty() {
                let tasks_active = {
                    let state = app.state::<AppState>();
                    !state.image_tasks.lock().unwrap().is_empty()
                };
                if tasks_active {
                    return not_found();
                }
            }
            // 未生成则按需生成(与扫描期生成同一实现,结果幂等)
            let thumb = match ensure_thumbnail(&data_dir, &item) {
                Ok(rel) => {
                    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
                    let _ = store.set_image_thumb(id, &rel);
                    rel
                }
                Err(_) => return not_found(),
            };
            match fs::read(data_dir.join("image_thumbs").join(&thumb)) {
                Ok(bytes) => tauri::http::Response::builder()
                    .status(200)
                    .header(CONTENT_TYPE, "image/webp")
                    .header(CACHE_CONTROL, "max-age=31536000, immutable")
                    .body(CowBytes::Owned(bytes))
                    .unwrap(),
                Err(_) => not_found(),
            }
        }
        "full" => match fs::read(&item.path) {
            Ok(bytes) => tauri::http::Response::builder()
                .status(200)
                .header(CONTENT_TYPE, mime_by_ext(&item.ext))
                .header(CACHE_CONTROL, "max-age=31536000, immutable")
                .body(CowBytes::Owned(bytes))
                .unwrap(),
            Err(_) => not_found(),
        },
        _ => not_found(),
    }
}

/* ---------------- 工具 ---------------- */

fn trash_delete(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    trash::delete(path).map_err(|e| format!("移入废纸篓失败: {e}"))
}

/// 批量移入废纸篓:一次性提交全部路径。trash crate 在 macOS 默认走 Finder 删除,
/// 每次调用都会播放一次系统废纸篓音效;delete_all 把所有路径合并为一次 Finder
/// 删除,音效只播一次。
/// 跳过已不存在的路径:delete_all 会先 canonicalize,任一路径缺失都让整批失败;
/// 已被外部移动/删除的文件无需入篓,直接清索引即可。
fn trash_delete_all(paths: &[String]) -> Result<(), String> {
    let refs: Vec<&Path> = paths
        .iter()
        .map(|p| Path::new(p.as_str()))
        .filter(|p| p.exists())
        .collect();
    if refs.is_empty() {
        return Ok(());
    }
    trash::delete_all(refs).map_err(|e| format!("移入废纸篓失败: {e}"))
}

/// 文件 blake3(流式 64KB 分块)。
fn file_blake3(path: &Path) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
