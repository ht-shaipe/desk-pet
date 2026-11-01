//! 磁盘清理：规则化候选扫描 + 安全删除，通过本地 SSE
//! （`http://127.0.0.1:<random>/cleanup/events/<job>`）持续推送进度。
//!
//! 扫描/计量/分类/删除逻辑都在 `deskpet-disk` crate（可独立测试）；
//! 这里只做任务编排：SSE 事件流、候选缓存、展示路径映射。

use std::{
    collections::HashMap,
    convert::Infallible,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use deskpet_disk::{
    classify::{Category, Reclaim, Risk},
    removal::{self, RemovalMode},
    scan::{self, ScanLimits, ScanOptions},
};
use serde::Serialize;
use tokio::sync::{OnceCell, mpsc};
use tokio_stream::StreamExt;

static SERVICE: OnceCell<Arc<CleanupService>> = OnceCell::const_new();
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateDto {
    /// 展示路径（保留用户输入前缀），同时作为稳定 key
    path: String,
    name: String,
    kind: &'static str,
    bytes: u64,
    files: u64,
    /// 最近修改时间（Unix 秒，0 = 未知）
    modified: i64,
    rule_id: &'static str,
    category: Category,
    reclaim: Option<Reclaim>,
    risk: Risk,
    /// 目录体积是否完整（扫描预算内完成）
    complete: bool,
    /// 扫描完成后是否默认勾选
    default_selected: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewEntryDto {
    pub(crate) name: String,
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureDto {
    pub(crate) path: String,
    pub(crate) error: String,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum CleanupEvent {
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
        items: Vec<CandidateDto>,
    },
    Preview {
        entries: Vec<PreviewEntryDto>,
        elapsed_ms: u64,
    },
    ScanDone {
        count: usize,
        bytes: u64,
        files: u64,
        errors: u64,
        truncated: bool,
        complete: bool,
        duration_ms: u64,
    },
    Deleting {
        path: String,
        bytes: u64,
        mode: &'static str,
    },
    Deleted {
        path: String,
        ok: bool,
        bytes: u64,
        error: Option<String>,
    },
    DeleteDone {
        removed: u64,
        failed: u64,
        bytes: u64,
        freed_bytes: u64,
        free_before: u64,
        free_after: u64,
        failures: Vec<FailureDto>,
    },
    Cancelled {
        mode: &'static str,
    },
    /// 扫描/删除在开始前或运行中失败，前端必须结束对应阶段。
    Failed {
        mode: &'static str,
        error: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanTargetDto {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) is_dir: bool,
    /// 永久删除危险度（仅分析窗口计划携带；面板计划为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) git_repos: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) non_reclaimable_bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStream {
    pub(crate) job_id: String,
    pub(crate) url: String,
}

/// 指定路径所在磁盘卷的空间情况（字节）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageInfo {
    total: u64,
    available: u64,
}

/// 可扫描卷（根分区 + 外接/网络卷）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeDto {
    path: String,
    total: u64,
    available: u64,
}

/// 列出可扫描的真实卷（过滤伪文件系统与系统私有挂载）。
#[tauri::command]
pub async fn disk_list_volumes() -> Result<Vec<VolumeDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(deskpet_disk::space::list_volumes()
            .into_iter()
            .map(|volume| VolumeDto {
                path: volume.path,
                total: volume.total,
                available: volume.available,
            })
            .collect())
    })
    .await
    .map_err(|err| err.to_string())?
}

/// 打开 macOS「完全磁盘访问权限」设置页（扫描权限不足时的引导，不阻塞扫描）。
#[tauri::command]
pub async fn disk_open_full_disk_access() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
            .spawn()
            .map_err(|err| err.to_string())?;
    }
    Ok(())
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

/// 扫描缓存：展示路径 -> 删除所需的真实路径与计量。
pub(crate) struct CachedCandidate {
    pub(crate) real: PathBuf,
    pub(crate) bytes: u64,
    pub(crate) is_dir: bool,
    pub(crate) reclaim: Option<Reclaim>,
    pub(crate) risk: Risk,
}

pub(crate) struct ScanResult {
    /// 规范后的真实根路径，删除时做安全校验
    pub(crate) root: PathBuf,
    /// 用户输入的根路径前缀，仅用于展示（如 /tmp 对应 /private/tmp）
    pub(crate) display_root: PathBuf,
    /// 家目录（守卫与家目录相对规则使用）
    pub(crate) home: Option<PathBuf>,
    pub(crate) candidates: HashMap<String, CachedCandidate>,
}

pub(crate) struct JobEntry {
    /// SSE 接收器共享化：断线重连（或并发连接）复用同一个 receiver，
    /// 不再被首个连接 `take()` 独占导致重连 404。
    pub(crate) rx: Arc<Mutex<mpsc::UnboundedReceiver<CleanupEvent>>>,
    pub(crate) cancel: Arc<AtomicBool>,
}

pub(crate) struct CleanupService {
    pub(crate) port: u16,
    pub(crate) jobs: Mutex<HashMap<String, JobEntry>>,
    /// 扫描结果暂存，删除任务按 scan job id 取用
    pub(crate) scans: Arc<Mutex<HashMap<String, Arc<ScanResult>>>>,
}

impl CleanupService {
    pub(crate) fn create_job(
        &self,
    ) -> (String, mpsc::UnboundedSender<CleanupEvent>, Arc<AtomicBool>) {
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
                rx: Arc::new(Mutex::new(rx)),
                cancel: cancel.clone(),
            },
        );
        (id, tx, cancel)
    }

    /// 任务结束后移除条目：`jobs` 表不随扫描/删除次数无限增长，
    /// 未连接 SSE 的任务也不再让 unbounded receiver 无限缓冲事件。
    pub(crate) fn remove_job(&self, id: &str) {
        self.jobs.lock().unwrap().remove(id);
    }
}

/// 任务结束后延迟移除 job 条目：EventSource 若在任务完成之后才连接或
/// 重连，宽限期内仍能读到缓冲事件；宽限期一过条目被丢弃，`jobs` 表
/// 不随任务次数无限增长。
pub(crate) fn retire_job(service: Arc<CleanupService>, job_id: String) {
    const JOB_GRACE: Duration = Duration::from_secs(30);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(JOB_GRACE).await;
        service.remove_job(&job_id);
    });
}

/// 懒启动本地 SSE 服务，端口由系统分配，仅监听回环地址。
pub(crate) async fn service() -> Result<Arc<CleanupService>, String> {
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

/// SSE 端点只对本应用的 WebView 放行：进度事件包含完整本地路径，
/// 任意网页都不应读到。开发模式额外放行 Vite dev server（HMR）来源。
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    const ALLOWED: &[&str] = &[
        // macOS/iOS WebView
        "tauri://localhost",
        // Windows/Android WebView
        "http://tauri.localhost",
        "https://tauri.localhost",
        // Linux WebView
        "http://localhost",
    ];
    if ALLOWED.contains(&origin) {
        return true;
    }
    #[cfg(debug_assertions)]
    if origin.starts_with("http://localhost:") || origin.starts_with("http://127.0.0.1:") {
        return true;
    }
    false
}

/// 共享 receiver 的轮询流：多个 SSE 连接（重连/并发）复用同一个
/// `UnboundedReceiver`，连接断开只丢掉本流的 Arc 克隆。
struct SharedRxStream(Arc<Mutex<mpsc::UnboundedReceiver<CleanupEvent>>>);

impl tokio_stream::Stream for SharedRxStream {
    type Item = CleanupEvent;
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<CleanupEvent>> {
        let mut rx = self.0.lock().unwrap();
        rx.poll_recv(cx)
    }
}

async fn events(
    State(svc): State<Arc<CleanupService>>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Response {
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    let Some(rx) = svc
        .jobs
        .lock()
        .unwrap()
        .get(&job_id)
        .map(|job| job.rx.clone())
    else {
        return (StatusCode::NOT_FOUND, "cleanup job not found").into_response();
    };

    // 共享 receiver 轮询：连接断开只丢掉本流的 Arc 克隆，任务条目里的
    // receiver 仍在，EventSource 重连可以继续接收（不再 404）。
    let stream = SharedRxStream(rx).map(|event| {
        let data = serde_json::to_string(&event)
            .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"serialize error\"}".to_string());
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

/// 启动规则化扫描任务，返回该任务的 SSE 地址。
#[tauri::command]
pub async fn disk_cleanup_scan(root: String, log_days: u32) -> Result<CleanupStream, String> {
    let display_root = PathBuf::from(root.trim());
    let root = display_root
        .canonicalize()
        .map_err(|e| format!("无法访问目录: {e}"))?;
    if !root.is_dir() {
        return Err("请选择有效的目录".to_string());
    }
    deskpet_disk::removal::validate_scan_root(&root)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);

    let svc = service().await?;
    // 新扫描替换旧的暂存结果，避免反复扫描累积占用内存
    svc.scans.lock().unwrap().clear();
    let (job_id, tx, cancel) = svc.create_job();
    let url = format!("http://127.0.0.1:{}/cleanup/events/{}", svc.port, job_id);
    let scans = svc.scans.clone();
    let store_id = job_id.clone();
    let job_service = svc.clone();
    let job_entry_id = job_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        'work: {
            let started_ok = tx
                .send(CleanupEvent::Started {
                    mode: "scan",
                    root: display_root.to_string_lossy().to_string(),
                })
                .is_ok();
            if !started_ok {
                break 'work;
            }
        let scan = scan::scan_candidates(
            &root,
            home.as_deref(),
            ScanOptions {
                log_days: log_days.min(3650),
            },
            ScanLimits::default(),
            &cancel,
            &mut |progress| {
                let _ = tx.send(CleanupEvent::Progress {
                    dirs: progress.dirs,
                    files: progress.files,
                    bytes: progress.candidate_bytes,
                    errors: progress.errors,
                    current: to_display(&progress.current, &root, &display_root)
                        .to_string_lossy()
                        .to_string(),
                });
            },
            &mut |snapshot| {
                let _ = tx.send(CleanupEvent::Preview {
                    entries: snapshot
                        .entries
                        .iter()
                        .map(|entry| PreviewEntryDto {
                            name: entry.name.clone(),
                            bytes: entry.bytes,
                            files: entry.files,
                        })
                        .collect(),
                    elapsed_ms: snapshot.elapsed_ms,
                });
            },
        );
        match scan {
            Err(deskpet_disk::DiskError::Cancelled) => {
                let _ = tx.send(CleanupEvent::Cancelled { mode: "scan" });
            }
            Err(err) => {
                let _ = tx.send(CleanupEvent::Failed {
                    mode: "scan",
                    error: err.to_string(),
                });
            }
            Ok(output) => {
                let mut candidates = HashMap::new();
                let mut dtos: Vec<CandidateDto> = Vec::new();
                let mut total_bytes = 0u64;
                for item in &output.candidates {
                    // 非 UTF-8 展示路径无法无损传回前端，直接跳过，避免删除时误伤
                    let display = to_display(&item.path, &root, &display_root);
                    let Some(display) = display.to_str() else {
                        continue;
                    };
                    // 只统计真正能回传给前端并进入缓存的项目；否则 ScanDone
                    // 的字节数会包含用户根本无法勾选/删除的路径。
                    total_bytes = total_bytes.saturating_add(item.bytes);
                    candidates.insert(
                        display.to_string(),
                        CachedCandidate {
                            real: item.path.clone(),
                            bytes: item.bytes,
                            is_dir: matches!(item.kind, deskpet_disk::classify::Kind::Dir),
                            reclaim: item.reclaim,
                            risk: item.risk,
                        },
                    );
                    dtos.push(CandidateDto {
                        path: display.to_string(),
                        name: item.name.clone(),
                        kind: match item.kind {
                            deskpet_disk::classify::Kind::Dir => "dir",
                            deskpet_disk::classify::Kind::File => "file",
                        },
                        bytes: item.bytes,
                        files: item.files,
                        modified: item.modified,
                        rule_id: item.rule_id,
                        category: item.category,
                        reclaim: item.reclaim,
                        risk: item.risk,
                        complete: item.complete,
                        default_selected: item.default_selected,
                    });
                }
                // 分批推送候选（实时边扫边推留给 P3 的实时预览），避免单个超大事件
                for batch in dtos.chunks(200) {
                    if tx
                        .send(CleanupEvent::Found {
                            items: batch.to_vec(),
                        })
                        .is_err()
                    {
                        break 'work;
                    }
                }
                let count = candidates.len();
                scans.lock().unwrap().insert(
                    store_id,
                    Arc::new(ScanResult {
                        root,
                        display_root,
                        home,
                        candidates,
                    }),
                );
                let _ = tx.send(CleanupEvent::ScanDone {
                    count,
                    bytes: total_bytes,
                    files: output.stats.files,
                    errors: output.stats.errors,
                    truncated: output.stats.truncated,
                    complete: output.stats.complete,
                    duration_ms: 0,
                });
            }
        }
        } // 'work：任务结束（完成/取消/失败/SSE 断连）后延迟清理 job 表，防止无限增长
        retire_job(job_service, job_entry_id);
    });

    Ok(CleanupStream { job_id, url })
}

/// 计算删除计划：targets / covered / blocked 与同卷可释放字节。
/// `paths` 为前端勾选的展示路径，必须非空（空选择不删除）。
#[tauri::command]
pub async fn disk_cleanup_plan(
    scan_job_id: String,
    paths: Vec<String>,
) -> Result<PlanResponse, String> {
    if paths.is_empty() {
        return Err("请先勾选要清理的项目".to_string());
    }
    let svc = service().await?;
    let scan = svc
        .scans
        .lock()
        .unwrap()
        .get(&scan_job_id)
        .cloned()
        .ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    // 安全策略依赖家目录（拒绝整仓删除、家目录保护等），缺失时拒绝生成计划
    let Some(home) = scan.home.clone() else {
        return Err("无法确定家目录，已拒绝生成删除计划".to_string());
    };
    // 计划需要遍历目录/读取元数据，放到阻塞线程池，避免卡住异步运行时
    let plan_scan = scan.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || -> Result<removal::Plan, String> {
        let requested = resolve_request(&plan_scan, &paths)?;
        let mut plan = removal::plan(&plan_scan.root, &home, &requested);
        apply_panel_policy(&plan_scan, &mut plan);
        Ok(plan)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(to_plan_response(&scan, &plan))
}

/// 面板候选是整目录删除：信息位（reclaim=None）和高风险项（Risky）不允许
/// 整体删除；Caution 项只能由用户逐项显式勾选。
fn apply_panel_policy(scan: &ScanResult, plan: &mut removal::Plan) {
    let mut blocked = Vec::new();
    plan.targets.retain(|target| {
        let candidate = scan
            .candidates
            .values()
            .find(|candidate| candidate.real == target.path);
        let Some(candidate) = candidate else {
            blocked.push(removal::Blocked {
                path: target.path.clone(),
                reason: "找不到该目录的扫描记录，请重新扫描".to_string(),
            });
            return false;
        };
        if candidate.reclaim.is_none() || candidate.risk == Risk::Risky {
            blocked.push(removal::Blocked {
                path: target.path.clone(),
                reason: "信息项或高风险项不能整体删除，请展开后只选择可回收内容".to_string(),
            });
            false
        } else {
            true
        }
    });
    plan.blocked.extend(blocked);
    plan.reclaim = plan.targets.iter().map(|target| target.bytes).sum();
}

/// 计划响应（展示路径口径）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanResponse {
    pub(crate) targets: Vec<PlanTargetDto>,
    pub(crate) covered: Vec<PlanTargetDto>,
    pub(crate) blocked: Vec<FailureDto>,
    pub(crate) reclaim: u64,
}

/// 启动删除任务，返回新的 SSE 地址。
/// `paths` 为前端勾选的展示路径，必须显式非空；`mode` 为 `trash` 或 `permanent`。
#[tauri::command]
pub async fn disk_cleanup_delete(
    scan_job_id: String,
    paths: Vec<String>,
    mode: String,
) -> Result<CleanupStream, String> {
    if paths.is_empty() {
        return Err("请先勾选要清理的项目".to_string());
    }
    let removal_mode = match mode.as_str() {
        "trash" => RemovalMode::Trash,
        "permanent" => RemovalMode::Permanent,
        _ => return Err(format!("未知删除模式: {mode}")),
    };

    let svc = service().await?;
    let scan = svc
        .scans
        .lock()
        .unwrap()
        .get(&scan_job_id)
        .cloned()
        .ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    // 安全策略依赖家目录，缺失时拒绝删除
    let Some(home) = scan.home.clone() else {
        return Err("无法确定家目录，已拒绝删除".to_string());
    };
    // 计划需要遍历目录/读取元数据，放到阻塞线程池，避免卡住异步运行时
    let plan_scan = scan.clone();
    let plan_home = home.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || -> Result<removal::Plan, String> {
        let requested = resolve_request(&plan_scan, &paths)?;
        let mut plan = removal::plan(&plan_scan.root, &plan_home, &requested);
        if plan.targets.is_empty() {
            return Err("所选项目均未通过安全检查，请查看被拒绝原因".to_string());
        }
        apply_panel_policy(&plan_scan, &mut plan);
        Ok(plan)
    })
    .await
    .map_err(|e| e.to_string())??;
    if plan.targets.is_empty() {
        return Err("所选项目均未通过安全检查（信息项/高风险项），请查看被拒绝原因".to_string());
    }

    let (job_id, tx, cancel) = svc.create_job();
    let url = format!("http://127.0.0.1:{}/cleanup/events/{}", svc.port, job_id);
    let scans = svc.scans.clone();
    let job_service = svc.clone();
    let job_entry_id = job_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _ = tx.send(CleanupEvent::Started {
            mode: "delete",
            root: scan.display_root.to_string_lossy().to_string(),
        });
        let mode_label = match removal_mode {
            RemovalMode::Trash => "trash",
            RemovalMode::Permanent => "permanent",
        };
        let outcome = removal::execute(
            &plan,
            removal_mode,
            &home,
            &cancel,
            &mut |target| {
                let _ = tx.send(CleanupEvent::Deleting {
                    path: to_display(&target.path, &scan.root, &scan.display_root)
                        .to_string_lossy()
                        .to_string(),
                    bytes: target.bytes,
                    mode: mode_label,
                });
            },
            &mut |target, ok, bytes, error| {
                let _ = tx.send(CleanupEvent::Deleted {
                    path: to_display(&target.path, &scan.root, &scan.display_root)
                        .to_string_lossy()
                        .to_string(),
                    ok,
                    bytes,
                    error: error.map(str::to_string),
                });
            },
        );
        let _ = tx.send(CleanupEvent::DeleteDone {
            removed: outcome.removed,
            failed: outcome.failed,
            bytes: outcome.estimated_bytes,
            freed_bytes: outcome.free_after.saturating_sub(outcome.free_before),
            free_before: outcome.free_before,
            free_after: outcome.free_after,
            failures: outcome
                .failures
                .iter()
                .map(|failure| FailureDto {
                    path: to_display(&failure.path, &scan.root, &scan.display_root)
                        .to_string_lossy()
                        .to_string(),
                    error: failure.error.clone(),
                })
                .collect(),
        });
        // 删除结束后无论成功与否都丢弃结果，重试需要重新扫描
        scans.lock().unwrap().remove(&scan_job_id);
        // 删除任务结束（完成/失败）后延迟清理 job 表
        retire_job(job_service, job_entry_id);
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

/// 把前端勾选的展示路径还原为缓存中的真实路径与计量；未知路径直接判为失效。
fn resolve_request(
    scan: &ScanResult,
    paths: &[String],
) -> Result<Vec<(PathBuf, u64, bool)>, String> {
    let mut requested = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(cached) = scan.candidates.get(path) else {
            return Err("所选删除项已失效，请重新扫描".to_string());
        };
        requested.push((cached.real.clone(), cached.bytes, cached.is_dir));
    }
    Ok(requested)
}

fn to_plan_response(scan: &ScanResult, plan: &removal::Plan) -> PlanResponse {
    let display = |path: &Path| {
        to_display(path, &scan.root, &scan.display_root)
            .to_string_lossy()
            .to_string()
    };
    PlanResponse {
        targets: plan
            .targets
            .iter()
            .map(|target| PlanTargetDto {
                path: display(&target.path),
                bytes: target.bytes,
                is_dir: target.is_dir,
                git_repos: None,
                non_reclaimable_bytes: None,
            })
            .collect(),
        covered: plan
            .covered
            .iter()
            .map(|covered| PlanTargetDto {
                path: display(&covered.path),
                bytes: covered.bytes,
                is_dir: true,
                git_repos: None,
                non_reclaimable_bytes: None,
            })
            .collect(),
        blocked: plan
            .blocked
            .iter()
            .map(|blocked| FailureDto {
                path: display(&blocked.path),
                error: blocked.reason.clone(),
            })
            .collect(),
        reclaim: plan.reclaim,
    }
}

/// 把规范路径前缀替换回用户输入的展示路径（如 /private/tmp → /tmp）。
fn to_display<'a>(path: &'a Path, root: &Path, display_root: &Path) -> std::borrow::Cow<'a, Path> {
    match path.strip_prefix(root) {
        Ok(rest) => display_root.join(rest),
        Err(_) => path.to_path_buf(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
    };

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
            files: 1,
            errors: 0,
            truncated: false,
            complete: true,
            duration_ms: 0,
        })
        .unwrap();
        drop(tx);

        let mut stream = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: tauri://localhost\r\nAccept: text/event-stream\r\n\r\n"
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

    /// 读取一行响应（\r\n 结尾），用于状态行断言。
    fn read_status_line(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
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
        String::from_utf8_lossy(&line).to_string()
    }

    #[test]
    fn sse_endpoint_rejects_foreign_origin() {
        let svc = tauri::async_runtime::block_on(service()).expect("SSE 服务应启动");
        let (job_id, tx, _cancel) = svc.create_job();
        tx.send(CleanupEvent::Started {
            mode: "scan",
            root: "/tmp".to_string(),
        })
        .unwrap();
        drop(tx);

        let mut stream = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
        write!(
            stream,
            "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\nAccept: text/event-stream\r\n\r\n"
        )
        .unwrap();
        let status = read_status_line(&mut stream);
        assert!(status.to_ascii_lowercase().contains("403"), "got: {status}");
    }

    #[test]
    fn sse_endpoint_rejects_missing_origin() {
        let svc = tauri::async_runtime::block_on(service()).expect("SSE 服务应启动");
        let (job_id, tx, _cancel) = svc.create_job();
        drop(tx);

        let mut stream = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
        write!(
            stream,
            "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n"
        )
        .unwrap();
        let status = read_status_line(&mut stream);
        assert!(status.to_ascii_lowercase().contains("403"), "got: {status}");
    }

    #[test]
    fn sse_endpoint_allows_reconnect_after_first_client() {
        let svc = tauri::async_runtime::block_on(service()).expect("SSE 服务应启动");
        let (job_id, tx, _cancel) = svc.create_job();
        tx.send(CleanupEvent::Started {
            mode: "scan",
            root: "/tmp".to_string(),
        })
        .unwrap();
        // 第一个客户端连上后立刻断开，第二个客户端仍应能连接并读到缓冲事件
        {
            let mut first = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
            first
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            write!(
                first,
                "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: tauri://localhost\r\nAccept: text/event-stream\r\n\r\n"
            )
            .unwrap();
            let mut buf = [0u8; 256];
            let _ = first.read(&mut buf);
            drop(first);
        }

        let mut second = std::net::TcpStream::connect(("127.0.0.1", svc.port)).unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            second,
            "GET /cleanup/events/{job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: tauri://localhost\r\nAccept: text/event-stream\r\n\r\n"
        )
        .unwrap();
        let status = read_status_line(&mut second);
        assert!(status.to_ascii_lowercase().contains("200"), "got: {status}");
    }

    #[test]
    fn display_mapping_round_trips_symlink_prefix() {
        let root = Path::new("/private/tmp/scan");
        let display_root = Path::new("/tmp/scan");
        let path = Path::new("/private/tmp/scan/logs/app.log");
        let display = to_display(path, root, display_root);
        assert_eq!(display, Path::new("/tmp/scan/logs/app.log"));
    }

    #[test]
    fn panel_policy_blocks_info_risky_and_missing_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let info = root.join("info");
        let risky = root.join("risky");
        let safe = root.join("safe");
        let missing = root.join("missing");
        for path in [&info, &risky, &safe, &missing] {
            fs::create_dir_all(path).unwrap();
        }

        let candidate = |real: &Path, reclaim: Option<Reclaim>, risk: Risk| {
            (
                real.to_string_lossy().to_string(),
                CachedCandidate {
                    real: real.to_path_buf(),
                    bytes: 10,
                    is_dir: true,
                    reclaim,
                    risk,
                },
            )
        };
        let scan = ScanResult {
            root: root.clone(),
            display_root: root.clone(),
            home: Some(home.clone()),
            candidates: HashMap::from([
                candidate(&info, None, Risk::Safe),
                candidate(&risky, Some(Reclaim::Temporary), Risk::Risky),
                candidate(&safe, Some(Reclaim::Regenerable), Risk::Safe),
            ]),
        };
        let requested = [&info, &risky, &safe, &missing]
            .into_iter()
            .map(|path| (path.clone(), 10, true))
            .collect::<Vec<_>>();
        let mut plan = removal::plan(&root, &home, &requested);
        assert_eq!(plan.targets.len(), 4);

        apply_panel_policy(&scan, &mut plan);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].path, safe);
        assert_eq!(plan.blocked.len(), 3);
        assert_eq!(plan.reclaim, 10);
        assert!(
            plan.blocked
                .iter()
                .any(|item| item.path == missing && item.reason.contains("扫描记录"))
        );
    }
}
