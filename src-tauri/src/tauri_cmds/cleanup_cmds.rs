//! 磁盘清理：扫描 `.log` 文件、Rust `target` 与前端 `node_modules`，
//! 通过本地 SSE（`http://127.0.0.1:<random>/cleanup/events/<job>`）持续推送进度。

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime},
};

use axum::{
    extract::{Path as AxumPath, State},
    http::{header, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
    Router,
};
use serde::Serialize;
use tokio::sync::{mpsc, OnceCell};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

static SERVICE: OnceCell<Arc<CleanupService>> = OnceCell::const_new();
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

/// 从选定目录向下扫描的最大层级
const MAX_DEPTH: usize = 12;
/// 单次扫描最多收集的条目数，避免误选大目录后占用过多内存
const MAX_CANDIDATES: usize = 20_000;
/// 统计单个目录体积时最多访问的子目录数
const MAX_DIR_VISITS: usize = 200_000;

/// 不进入的系统/隐藏目录（node_modules、target 由扫描逻辑单独处理）
const SKIP_DIRS: &[&str] = &[
    "Library",
    "AppData",
    "Windows",
    "System32",
    "Program Files",
    "Program Files (x86)",
    "$Recycle.Bin",
    "System Volume Information",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanupCandidate {
    id: String,
    kind: String,
    path: String,
    bytes: u64,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum CleanupEvent {
    Started {
        mode: &'static str,
        root: String,
    },
    Progress {
        dirs: u64,
        files: u64,
        bytes: u64,
        errors: u64,
        current: String,
    },
    Found {
        items: Vec<CleanupCandidate>,
    },
    ScanDone {
        count: usize,
        bytes: u64,
        errors: u64,
        truncated: bool,
    },
    Deleting {
        id: String,
        kind: String,
        path: String,
        bytes: u64,
    },
    Deleted {
        id: String,
        ok: bool,
        bytes: u64,
        error: Option<String>,
    },
    DeleteDone {
        removed: usize,
        failed: usize,
        bytes: u64,
    },
    Cancelled {
        mode: &'static str,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStream {
    job_id: String,
    url: String,
}

/// 指定路径所在磁盘卷的空间情况（字节）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageInfo {
    total: u64,
    available: u64,
}

/// 查询 `root` 所在磁盘卷的总容量与可用空间。
#[tauri::command]
pub async fn disk_cleanup_usage(root: String) -> Result<DiskUsageInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(root.trim());
        let probe = root.canonicalize().unwrap_or(root);
        deskpet_os::disk::disk_usage(&probe)
            .map(|usage| DiskUsageInfo {
                total: usage.total,
                available: usage.available,
            })
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TargetKind {
    Logs,
    Target,
    NodeModules,
}

impl TargetKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "logs" => Some(Self::Logs),
            "target" => Some(Self::Target),
            "node_modules" => Some(Self::NodeModules),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Target => "target",
            Self::NodeModules => "node_modules",
        }
    }
}

struct ScanResult {
    /// 规范后的真实根路径，删除时做安全校验
    root: PathBuf,
    /// 用户输入的根路径前缀，仅用于展示（如 /tmp 对应 /private/tmp）
    display_root: PathBuf,
    candidates: Vec<CleanupCandidate>,
}

struct ScanOutcome {
    result: ScanResult,
    bytes: u64,
    errors: u64,
    truncated: bool,
}

struct JobEntry {
    rx: Option<mpsc::UnboundedReceiver<CleanupEvent>>,
    cancel: Arc<AtomicBool>,
}

struct CleanupService {
    port: u16,
    jobs: Mutex<HashMap<String, JobEntry>>,
    /// 扫描结果暂存，删除任务按 scan job id 取用
    scans: Arc<Mutex<HashMap<String, Arc<ScanResult>>>>,
}

impl CleanupService {
    fn create_job(&self) -> (String, mpsc::UnboundedSender<CleanupEvent>, Arc<AtomicBool>) {
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = format!("{millis:x}-{}", JOB_SEQ.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.lock().unwrap().insert(
            id.clone(),
            JobEntry {
                rx: Some(rx),
                cancel: cancel.clone(),
            },
        );
        (id, tx, cancel)
    }
}

/// 懒启动本地 SSE 服务，端口由系统分配，仅监听回环地址。
async fn service() -> Result<Arc<CleanupService>, String> {
    SERVICE
        .get_or_try_init(|| async {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| format!("无法启动清理进度服务: {e}"))?;
            let port = listener
                .local_addr()
                .map_err(|e| format!("无法获取清理进度服务端口: {e}"))?
                .port();
            let svc = Arc::new(CleanupService {
                port,
                jobs: Mutex::new(HashMap::new()),
                scans: Arc::new(Mutex::new(HashMap::new())),
            });
            let router = Router::new()
                .route("/cleanup/events/{job}", get(events))
                .with_state(svc.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(err) = axum::serve(listener, router).await {
                    eprintln!("disk cleanup SSE server stopped: {err}");
                }
            });
            Ok::<_, String>(svc)
        })
        .await
        .cloned()
}

async fn events(
    State(svc): State<Arc<CleanupService>>,
    AxumPath(job_id): AxumPath<String>,
) -> Response {
    let rx = {
        let mut jobs = svc.jobs.lock().unwrap();
        jobs.get_mut(&job_id).and_then(|job| job.rx.take())
    };
    let Some(rx) = rx else {
        return (StatusCode::NOT_FOUND, "cleanup job not found").into_response();
    };

    let stream = UnboundedReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| {
            "{\"type\":\"error\",\"message\":\"serialize error\"}".to_string()
        });
        Ok::<_, Infallible>(Event::default().data(data))
    });

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response();
    // WebView 的 origin 是 tauri://localhost / http://tauri.localhost，本地服务需放行跨域读取
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

/// 启动扫描任务，返回该任务的 SSE 地址。
#[tauri::command]
pub async fn disk_cleanup_scan(
    root: String,
    targets: Vec<String>,
    log_days: u32,
) -> Result<CleanupStream, String> {
    let display_root = PathBuf::from(root.trim());
    let root = display_root
        .canonicalize()
        .map_err(|e| format!("无法访问目录: {e}"))?;
    if !root.is_dir() {
        return Err("请选择有效的目录".to_string());
    }
    if root.parent().is_none() {
        return Err("请选择用户目录或项目目录，不建议直接扫描磁盘根目录".to_string());
    }

    let kinds: HashSet<TargetKind> = targets
        .iter()
        .filter_map(|item| TargetKind::parse(item))
        .collect();
    if kinds.is_empty() {
        return Err("请至少选择一种清理范围".to_string());
    }

    let svc = service().await?;
    // 新扫描替换旧的暂存结果，避免反复扫描累积占用内存
    svc.scans.lock().unwrap().clear();
    let (job_id, tx, cancel) = svc.create_job();
    let url = format!(
        "http://127.0.0.1:{}/cleanup/events/{}",
        svc.port, job_id
    );
    let scans = svc.scans.clone();
    let store_id = job_id.clone();
    let log_days = log_days.min(3650);

    tauri::async_runtime::spawn_blocking(move || {
        if let Some(outcome) = run_scan(&root, &display_root, &kinds, log_days, &tx, &cancel) {
            let count = outcome.result.candidates.len();
            // 先落库再通知完成，避免前端收到 scanDone 后立刻删除时取不到结果
            scans
                .lock()
                .unwrap()
                .insert(store_id, Arc::new(outcome.result));
            let _ = tx.send(CleanupEvent::ScanDone {
                count,
                bytes: outcome.bytes,
                errors: outcome.errors,
                truncated: outcome.truncated,
            });
        }
    });

    Ok(CleanupStream { job_id, url })
}

/// 启动删除任务，返回新的 SSE 地址。
/// `ids` 为前端勾选的候选项 id；缺省或为空时删除全部扫描结果。
#[tauri::command]
pub async fn disk_cleanup_delete(
    scan_job_id: String,
    ids: Option<Vec<String>>,
) -> Result<CleanupStream, String> {
    let svc = service().await?;
    let scan = svc
        .scans
        .lock()
        .unwrap()
        .get(&scan_job_id)
        .cloned()
        .ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;

    let scan = match ids {
        Some(ids) if !ids.is_empty() => {
            let selected: HashSet<String> = ids.into_iter().collect();
            let candidates = scan
                .candidates
                .iter()
                .filter(|item| selected.contains(&item.id))
                .cloned()
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                return Err("所选删除项已失效，请重新扫描".to_string());
            }
            Arc::new(ScanResult {
                root: scan.root.clone(),
                display_root: scan.display_root.clone(),
                candidates,
            })
        }
        _ => scan,
    };

    let (job_id, tx, cancel) = svc.create_job();
    let url = format!(
        "http://127.0.0.1:{}/cleanup/events/{}",
        svc.port, job_id
    );
    let scans = svc.scans.clone();

    tauri::async_runtime::spawn_blocking(move || {
        run_delete(scan, &tx, &cancel);
        // 删除结束后无论成功与否都丢弃结果，重试需要重新扫描
        scans.lock().unwrap().remove(&scan_job_id);
    });

    Ok(CleanupStream { job_id, url })
}

/// 请求停止扫描或删除任务（幂等）。
#[tauri::command]
pub async fn disk_cleanup_cancel(job_id: String) -> Result<(), String> {
    let svc = service().await?;
    if let Some(job) = svc.jobs.lock().unwrap().get(&job_id) {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// 扫描文件系统；返回 `None` 表示任务被取消（已发送 Cancelled 事件）。
/// `root` 为规范化后的真实路径，`display_root` 为用户输入的展示路径。
fn run_scan(
    root: &Path,
    display_root: &Path,
    kinds: &HashSet<TargetKind>,
    log_days: u32,
    tx: &mpsc::UnboundedSender<CleanupEvent>,
    cancel: &AtomicBool,
) -> Option<ScanOutcome> {
    if tx
        .send(CleanupEvent::Started {
            mode: "scan",
            root: display_root.to_string_lossy().to_string(),
        })
        .is_err()
    {
        return None;
    }

    let mut candidates: Vec<CleanupCandidate> = Vec::new();
    let mut batch: Vec<CleanupCandidate> = Vec::new();
    let mut seq = 0usize;
    let mut dirs = 0u64;
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut errors = 0u64;
    let mut truncated = false;
    let mut last_emit = Instant::now();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    'walk: while let Some((dir, depth)) = stack.pop() {
        if cancelled(tx, cancel, "scan") {
            return None;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        dirs += 1;

        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                let _ = tx.send(CleanupEvent::Cancelled { mode: "scan" });
                return None;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if file_type.is_dir() {
                if name == "target" {
                    let is_rust = path
                        .parent()
                        .map(|parent| parent.join("Cargo.toml").is_file())
                        .unwrap_or(false);
                    if kinds.contains(&TargetKind::Target) && is_rust {
                        let _ = tx.send(CleanupEvent::Progress {
                            dirs,
                            files,
                            bytes,
                            errors,
                            current: to_display(&path, root, display_root).to_string_lossy().to_string(),
                        });
                        let Some(size) = dir_size(&path, cancel) else {
                            let _ = tx.send(CleanupEvent::Cancelled { mode: "scan" });
                            return None;
                        };
                        if push_candidate(
                            &mut candidates,
                            &mut batch,
                            &mut seq,
                            TargetKind::Target,
                            &path,
                            root,
                            display_root,
                            size,
                        ) {
                            bytes = bytes.saturating_add(size);
                        }
                    }
                    // 无论是否清理，都不进入构建产物内部
                    continue;
                }
                if name == "node_modules" {
                    let is_frontend = path
                        .parent()
                        .map(|parent| parent.join("package.json").is_file())
                        .unwrap_or(false);
                    if kinds.contains(&TargetKind::NodeModules) && is_frontend {
                        let _ = tx.send(CleanupEvent::Progress {
                            dirs,
                            files,
                            bytes,
                            errors,
                            current: to_display(&path, root, display_root).to_string_lossy().to_string(),
                        });
                        let Some(size) = dir_size(&path, cancel) else {
                            let _ = tx.send(CleanupEvent::Cancelled { mode: "scan" });
                            return None;
                        };
                        if push_candidate(
                            &mut candidates,
                            &mut batch,
                            &mut seq,
                            TargetKind::NodeModules,
                            &path,
                            root,
                            display_root,
                            size,
                        ) {
                            bytes = bytes.saturating_add(size);
                        }
                    }
                    continue;
                }
                if should_skip_dir(&name) {
                    continue;
                }
                if depth < MAX_DEPTH {
                    stack.push((path.clone(), depth + 1));
                }
            } else if file_type.is_file() {
                files += 1;
                if kinds.contains(&TargetKind::Logs) && is_log_file(&name) {
                    if let Ok(meta) = entry.metadata() {
                        if is_old_enough(&meta, log_days) {
                            let size = meta.len();
                            if push_candidate(
                                &mut candidates,
                                &mut batch,
                                &mut seq,
                                TargetKind::Logs,
                                &path,
                                root,
                                display_root,
                                size,
                            ) {
                                bytes = bytes.saturating_add(size);
                            }
                        }
                    }
                }
            }

            if batch.len() >= 40 || last_emit.elapsed() >= Duration::from_millis(150) {
                if !flush_found(tx, &mut batch)
                    || tx
                        .send(CleanupEvent::Progress {
                            dirs,
                            files,
                            bytes,
                            errors,
                            current: to_display(&path, root, display_root).to_string_lossy().to_string(),
                        })
                        .is_err()
                {
                    return None;
                }
                last_emit = Instant::now();
            }

            if candidates.len() >= MAX_CANDIDATES {
                truncated = true;
                break 'walk;
            }
        }
    }

    if cancelled(tx, cancel, "scan") {
        return None;
    }
    if !flush_found(tx, &mut batch) {
        return None;
    }
    let _ = tx.send(CleanupEvent::Progress {
        dirs,
        files,
        bytes,
        errors,
        current: display_root.to_string_lossy().to_string(),
    });

    Some(ScanOutcome {
        result: ScanResult {
            root: root.to_path_buf(),
            display_root: display_root.to_path_buf(),
            candidates,
        },
        bytes,
        errors,
        truncated,
    })
}

fn run_delete(
    scan: Arc<ScanResult>,
    tx: &mpsc::UnboundedSender<CleanupEvent>,
    cancel: &AtomicBool,
) -> bool {
    if tx
        .send(CleanupEvent::Started {
            mode: "delete",
            root: scan.display_root.to_string_lossy().to_string(),
        })
        .is_err()
    {
        return false;
    }

    let mut removed = 0usize;
    let mut failed = 0usize;
    let mut bytes = 0u64;

    for candidate in &scan.candidates {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(CleanupEvent::Cancelled { mode: "delete" });
            return false;
        }
        // 候选路径可能带展示前缀（如 /tmp），先还原为规范路径再做越界校验
        let path = PathBuf::from(&candidate.path);
        let real_path = to_canonical(&path, &scan.root, &scan.display_root)
            .canonicalize()
            .unwrap_or_else(|_| path.clone());
        if real_path.as_path() == scan.root.as_path() || !real_path.starts_with(&scan.root) {
            failed += 1;
            let _ = tx.send(CleanupEvent::Deleted {
                id: candidate.id.clone(),
                ok: false,
                bytes: 0,
                error: Some("路径越界，已跳过".to_string()),
            });
            continue;
        }
        let _ = tx.send(CleanupEvent::Deleting {
            id: candidate.id.clone(),
            kind: candidate.kind.clone(),
            path: candidate.path.clone(),
            bytes: candidate.bytes,
        });

        let result = if candidate.kind == TargetKind::Logs.as_str() {
            fs::remove_file(&real_path)
        } else {
            fs::remove_dir_all(&real_path)
        };
        match result {
            Ok(()) => {
                removed += 1;
                bytes = bytes.saturating_add(candidate.bytes);
                let _ = tx.send(CleanupEvent::Deleted {
                    id: candidate.id.clone(),
                    ok: true,
                    bytes: candidate.bytes,
                    error: None,
                });
            }
            Err(err) => {
                failed += 1;
                let _ = tx.send(CleanupEvent::Deleted {
                    id: candidate.id.clone(),
                    ok: false,
                    bytes: 0,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    let _ = tx.send(CleanupEvent::DeleteDone {
        removed,
        failed,
        bytes,
    });
    true
}

fn cancelled(
    tx: &mpsc::UnboundedSender<CleanupEvent>,
    cancel: &AtomicBool,
    mode: &'static str,
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(CleanupEvent::Cancelled { mode });
        true
    } else {
        false
    }
}

fn push_candidate(
    candidates: &mut Vec<CleanupCandidate>,
    batch: &mut Vec<CleanupCandidate>,
    seq: &mut usize,
    kind: TargetKind,
    path: &Path,
    root: &Path,
    display_root: &Path,
    bytes: u64,
) -> bool {
    // 非 UTF-8 路径无法无损传回前端，直接跳过，避免删除时误伤
    let display = to_display(path, root, display_root);
    let Some(path) = display.to_str() else {
        return false;
    };
    *seq += 1;
    let candidate = CleanupCandidate {
        id: format!("c{seq}"),
        kind: kind.as_str().to_string(),
        path: path.to_string(),
        bytes,
    };
    candidates.push(candidate.clone());
    batch.push(candidate);
    true
}

/// 把规范路径前缀替换回用户输入的展示路径（如 /private/tmp → /tmp）。
fn to_display<'a>(path: &'a Path, root: &Path, display_root: &Path) -> std::borrow::Cow<'a, Path> {
    match path.strip_prefix(root) {
        Ok(rest) => display_root.join(rest),
        Err(_) => return path.to_path_buf().into(),
    }
    .into()
}

/// 把展示路径前缀还原为规范路径（如 /tmp → /private/tmp）。
fn to_canonical(path: &Path, root: &Path, display_root: &Path) -> PathBuf {
    match path.strip_prefix(display_root) {
        Ok(rest) => root.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

fn flush_found(
    tx: &mpsc::UnboundedSender<CleanupEvent>,
    batch: &mut Vec<CleanupCandidate>,
) -> bool {
    if batch.is_empty() {
        return true;
    }
    let items = std::mem::take(batch);
    tx.send(CleanupEvent::Found { items }).is_ok()
}

/// 统计目录体积；遇到取消请求时返回 None。
fn dir_size(path: &Path, cancel: &AtomicBool) -> Option<u64> {
    let mut total = 0u64;
    let mut visited = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        visited += 1;
        if visited > MAX_DIR_VISITS {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    Some(total)
}

fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.')
        || SKIP_DIRS
            .iter()
            .any(|skipped| name.eq_ignore_ascii_case(skipped))
}

fn is_log_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
}

fn is_old_enough(meta: &fs::Metadata, days: u32) -> bool {
    if days == 0 {
        return true;
    }
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        // 修改时间在未来，保守跳过
        return false;
    };
    age >= Duration::from_secs(u64::from(days) * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn scan_all(root: &Path) -> ScanOutcome {
        let kinds = HashSet::from([
            TargetKind::Logs,
            TargetKind::Target,
            TargetKind::NodeModules,
        ]);
        // macOS 上 tempdir 的 /var 前缀本身是符号链接，统一用规范路径避免误判越界
        let root = &root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        run_scan(root, root, &kinds, 0, &tx, &AtomicBool::new(false)).expect("扫描应完成")
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("proj/target/debug")).unwrap();
        fs::write(root.join("proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("proj/target/debug/app"), vec![0u8; 1024]).unwrap();

        fs::create_dir_all(root.join("web/node_modules/dep")).unwrap();
        fs::write(root.join("web/package.json"), "{}").unwrap();
        fs::write(root.join("web/node_modules/dep/index.js"), "x").unwrap();

        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/app.log"), "hello").unwrap();
        fs::write(root.join("logs/keep.txt"), "hello").unwrap();

        // 没有 Cargo.toml / package.json 的同名目录不应被判定为构建产物
        fs::create_dir_all(root.join("assets/target")).unwrap();
        fs::create_dir_all(root.join("static/node_modules")).unwrap();
        // 隐藏目录不扫描
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/debug.log"), "hidden").unwrap();

        tmp
    }

    #[test]
    fn scan_finds_logs_target_and_node_modules() {
        let tmp = fixture();
        let outcome = scan_all(tmp.path());

        assert_eq!(outcome.result.candidates.len(), 3);
        let log = outcome
            .result
            .candidates
            .iter()
            .find(|item| item.kind == "logs")
            .unwrap();
        assert!(log.path.ends_with("app.log"));
        assert_eq!(log.bytes, 5);

        let target = outcome
            .result
            .candidates
            .iter()
            .find(|item| item.kind == "target")
            .unwrap();
        assert!(target.path.ends_with("proj/target"));
        assert_eq!(target.bytes, 1024);

        let modules = outcome
            .result
            .candidates
            .iter()
            .find(|item| item.kind == "node_modules")
            .unwrap();
        assert!(modules.path.ends_with("web/node_modules"));
        assert_eq!(modules.bytes, 1);

        assert!(outcome.result.candidates.iter().all(|item| {
            !item.path.contains(".hidden") && !item.path.contains("assets/target")
        }));
    }

    #[test]
    fn delete_removes_only_scanned_candidates() {
        let tmp = fixture();
        let outcome = scan_all(tmp.path());
        let (tx, _rx) = mpsc::unbounded_channel();
        assert!(run_delete(
            Arc::new(outcome.result),
            &tx,
            &AtomicBool::new(false)
        ));

        assert!(!tmp.path().join("proj/target").exists());
        assert!(!tmp.path().join("web/node_modules").exists());
        assert!(!tmp.path().join("logs/app.log").exists());
        assert!(tmp.path().join("proj/Cargo.toml").exists());
        assert!(tmp.path().join("web/package.json").exists());
        assert!(tmp.path().join("logs/keep.txt").exists());
        assert!(tmp.path().join("assets/target").exists());
        assert!(tmp.path().join("static/node_modules").exists());
    }

    /// 前端只勾选部分候选时，未勾选项必须保留
    #[test]
    fn delete_honors_selected_ids() {
        let tmp = fixture();
        let outcome = scan_all(tmp.path());
        let log = outcome
            .result
            .candidates
            .iter()
            .find(|item| item.kind == "logs")
            .unwrap()
            .id
            .clone();

        // 按命令层的过滤语义重建子集后删除
        let selected: HashSet<String> = [log].into_iter().collect();
        let subset = ScanResult {
            root: outcome.result.root.clone(),
            display_root: outcome.result.display_root.clone(),
            candidates: outcome
                .result
                .candidates
                .iter()
                .filter(|item| selected.contains(&item.id))
                .cloned()
                .collect(),
        };
        assert_eq!(subset.candidates.len(), 1);
        let (tx, _rx) = mpsc::unbounded_channel();
        assert!(run_delete(Arc::new(subset), &tx, &AtomicBool::new(false)));

        assert!(!tmp.path().join("logs/app.log").exists(), "勾选项应被删除");
        assert!(tmp.path().join("proj/target").exists(), "未勾选的 target 应保留");
        assert!(
            tmp.path().join("web/node_modules").exists(),
            "未勾选的 node_modules 应保留"
        );
    }

    /// 模拟 /tmp → /private/tmp：候选路径应展示用户输入的符号链接前缀，删除时还原为真实路径
    #[cfg(unix)]
    #[test]
    fn symlinked_root_is_displayed_as_input_path() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let real_root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(real_root.join("logs")).unwrap();
        fs::write(real_root.join("logs/app.log"), "x").unwrap();

        let outer = tempfile::tempdir().unwrap();
        let link = outer
            .path()
            .canonicalize()
            .unwrap()
            .join("tmp-link");
        symlink(&real_root, &link).unwrap();

        let kinds = HashSet::from([TargetKind::Logs]);
        let (tx, _rx) = mpsc::unbounded_channel();
        let outcome = run_scan(&real_root, &link, &kinds, 0, &tx, &AtomicBool::new(false))
            .expect("扫描应完成");
        let candidate = &outcome.result.candidates[0];
        assert!(
            candidate.path.starts_with(&link.to_string_lossy().to_string()),
            "候选路径应使用用户输入前缀: {}",
            candidate.path
        );

        let (tx, _rx) = mpsc::unbounded_channel();
        assert!(run_delete(Arc::new(outcome.result), &tx, &AtomicBool::new(false)));
        assert!(!real_root.join("logs/app.log").exists(), "应通过符号链接前缀删除真实文件");
    }

    #[test]
    fn log_file_matching_is_case_insensitive() {
        assert!(is_log_file("APP.LOG"));
        assert!(is_log_file("debug.log"));
        assert!(!is_log_file("log"));
        assert!(!is_log_file("notes.log.txt"));
    }

    #[test]
    fn sse_endpoint_streams_events() {
        let svc = tauri::async_runtime::block_on(service()).expect("SSE 服务应启动");
        let (job_id, tx, _cancel) = svc.create_job();
        tx.send(CleanupEvent::Started {
            mode: "scan",
            root: "/tmp".to_string(),
        })
        .unwrap();
        tx.send(CleanupEvent::ScanDone {
            count: 1,
            bytes: 1024,
            errors: 0,
            truncated: false,
        })
        .unwrap();
        drop(tx);

        let mut stream = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n"
        )
        .unwrap();

        let mut response = String::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    response.push_str(&String::from_utf8_lossy(&buf[..read]));
                    if response.contains("\"type\":\"scanDone\"") {
                        break;
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(err) => panic!("读取 SSE 响应失败: {err}"),
            }
        }

        let lower = response.to_ascii_lowercase();
        assert!(lower.contains("http/1.1 200"));
        assert!(lower.contains("access-control-allow-origin: *"));
        assert!(response.contains("\"type\":\"started\""));
        assert!(response.contains("\"type\":\"scanDone\""));
    }
}
