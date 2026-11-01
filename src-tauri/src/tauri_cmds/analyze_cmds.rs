//! 磁盘分析窗口：全树扫描、squarified 布局、标记、复核与删除。
//!
//! 与面板清理共享 deskpet-disk 的分类/删除核心与本地 SSE 服务；
//! 树缓存按 scanId 保留最近 2 份（LRU），窗口关闭时显式释放。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use deskpet_disk::{
    removal::RemovalMode,
    tree::{self, ScanTree, TreeLimits},
    treemap::{self, LayoutOptions, Metric},
};
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use super::cleanup_cmds::{
    CleanupEvent, CleanupStream, FailureDto, PlanResponse, PlanTargetDto, retire_job, service,
};

/// 树缓存：LRU 保留最近 2 个 scanId，防止多次分析累积内存。
/// 缓存条目：插入时间用于 TTL 兜底（文档 6.4：10 分钟）。
struct TreeEntry {
    id: String,
    tree: Arc<ScanTree>,
    inserted_at: std::time::Instant,
}

const TREE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

fn trees() -> &'static Mutex<Vec<TreeEntry>> {
    static TREES: std::sync::OnceLock<Mutex<Vec<TreeEntry>>> = std::sync::OnceLock::new();
    TREES.get_or_init(|| Mutex::new(Vec::new()))
}

/// 清理过期条目（每次访问时惰性执行）。
fn purge_expired(guard: &mut Vec<TreeEntry>) {
    guard.retain(|entry| entry.inserted_at.elapsed() < TREE_TTL);
}

fn cache_tree(id: String, tree: Arc<ScanTree>) {
    let mut guard = trees().lock().unwrap();
    purge_expired(&mut guard);
    guard.retain(|entry| entry.id != id);
    guard.insert(
        0,
        TreeEntry {
            id,
            tree,
            inserted_at: std::time::Instant::now(),
        },
    );
    guard.truncate(2);
}

fn take_tree(id: &str) -> Option<Arc<ScanTree>> {
    let mut guard = trees().lock().unwrap();
    purge_expired(&mut guard);
    guard
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.tree.clone())
}

fn drop_tree(id: &str) {
    trees().lock().unwrap().retain(|entry| entry.id != id);
}

/// 家目录：HOME 未设置时返回 None——删除安全策略（家目录保护、
/// 拒绝整仓删除等）依赖它，缺失时调用方必须拒绝生成/执行删除计划。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// 展示路径 → 树节点路径：允许 `/tmp` → `/private/tmp` 这类系统别名，
/// 但先拒绝 `..`，避免把穿越路径规范化成一个合法祖先。
fn canonical_tree_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    path.canonicalize().ok()
}

/// 删除计划路径：只规范化父目录，最后一级交给 `removal::plan` 做
/// symlink/身份检查；这样既支持 `/tmp` 这类根别名，又不会跟随用户标记的
/// 最后一级符号链接。
fn canonical_delete_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let parent = path.parent()?;
    let name = path.file_name()?;
    Some(parent.canonicalize().ok()?.join(name))
}

/// 打开（或聚焦）磁盘分析窗口，返回是否为新创建。
#[tauri::command]
pub async fn disk_analyzer_open(app: AppHandle) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_for_window = app.clone();
    app.run_on_main_thread(move || {
        let created = WebviewWindowBuilder::new(
            &app_for_window,
            "analyzer",
            WebviewUrl::App("index.html?view=analyzer".into()),
        )
        .title("磁盘分析")
        .inner_size(960.0, 680.0)
        .min_inner_size(720.0, 520.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .build()
        .is_ok();
        // 隐藏 Dock（accessory）模式下新窗口不会自动前置，显式拉起
        if let Some(window) = app_for_window.get_webview_window("analyzer") {
            // 无边框透明窗：内容层裁圆角（与主面板同一套外观语言）。
            // ns_window 是 macOS 专属 API，Windows 由系统自动圆角，无需处理。
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

/// 启动全树扫描，返回 SSE 进度地址（scanId 即 jobId）。
#[tauri::command]
pub async fn disk_analyze_scan(root: String) -> Result<CleanupStream, String> {
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
    let (job_id, tx, cancel) = svc.create_job();
    let url = format!("http://127.0.0.1:{}/cleanup/events/{}", svc.port, job_id);
    let store_id = job_id.clone();
    let job_service = svc.clone();
    let job_entry_id = job_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _ = tx.send(CleanupEvent::Started {
            mode: "scan",
            root: display_root.to_string_lossy().to_string(),
        });
        let scan = tree::scan_tree(
            &root,
            &display_root,
            home.as_deref(),
            TreeLimits::default(),
            &cancel,
            &mut |progress| {
                let _ = tx.send(CleanupEvent::Progress {
                    dirs: progress.dirs,
                    files: progress.files,
                    bytes: progress.bytes_seen,
                    errors: progress.errors,
                    current: progress.current.to_string_lossy().to_string(),
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
                let scan_tree = Arc::new(output);
                // 先写缓存再发 scanDone：前端收到事件后立刻发起 layout/marks，
                // 晚一步写缓存会导致刚完成就“扫描结果已失效”。
                cache_tree(store_id, Arc::clone(&scan_tree));
                let _ = tx.send(CleanupEvent::ScanDone {
                    count: scan_tree.stats.dirs as usize,
                    bytes: scan_tree.stats.total_bytes,
                    files: scan_tree.stats.files,
                    errors: scan_tree.stats.errors,
                    truncated: scan_tree.stats.truncated,
                    complete: scan_tree.stats.complete,
                    duration_ms: scan_tree.stats.duration_ms,
                });
            }
        }
        // 任务结束（完成/取消/失败/SSE 断连）后延迟清理 job 表，防止无限增长
        retire_job(job_service, job_entry_id);
    });

    Ok(CleanupStream { job_id, url })
}

/// 布局请求的度量。
fn parse_metric(metric: &str) -> Result<Metric, String> {
    match metric {
        "bytes" => Ok(Metric::Bytes),
        "files" => Ok(Metric::Files),
        other => Err(format!("未知度量: {other}")),
    }
}

/// 计算当前视图的 treemap tile。
#[tauri::command]
pub async fn disk_analyze_layout(
    scan_id: String,
    path: String,
    width: f64,
    height: f64,
    max_depth: u8,
    metric: String,
    filter: Option<String>,
) -> Result<treemap::LayoutResult, String> {
    let scan = take_tree(&scan_id).ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    let target = canonical_tree_path(&path).unwrap_or_else(|| PathBuf::from(&path));
    let metric = parse_metric(&metric)?;
    // locate/layout 会遍历整棵子树，放到阻塞线程池避免卡住异步运行时
    tauri::async_runtime::spawn_blocking(move || -> Result<treemap::LayoutResult, String> {
        let node = tree::locate(&scan.tree, &scan.root, &target)
            .ok_or_else(|| "路径不在扫描结果中".to_string())?;
        let opts = LayoutOptions {
            max_depth: (max_depth as usize).clamp(1, 4),
            ..LayoutOptions::default()
        };
        Ok(treemap::layout(
            node,
            &target,
            width.max(120.0),
            height.max(120.0),
            metric,
            filter.as_deref().filter(|value| !value.trim().is_empty()),
            &opts,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 分析侧栏"值得看看"：树上顶层可回收大目录。
#[tauri::command]
pub async fn disk_analyze_insights(
    scan_id: String,
) -> Result<Vec<deskpet_disk::insights::Insight>, String> {
    let scan = take_tree(&scan_id).ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    // 遍历树聚合，放到阻塞线程池避免卡住异步运行时
    Ok(tauri::async_runtime::spawn_blocking(move || {
        deskpet_disk::insights::insights_from_tree(
            &scan.tree,
            &scan.root,
            deskpet_disk::insights::MIN_INSIGHT_BYTES,
            8,
        )
    })
    .await
    .map_err(|e| e.to_string())?)
}

/// 标记项的当前状态（重扫后刷新大小、检出已消失项）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkInfo {
    path: String,
    bytes: u64,
    files: u64,
    exists: bool,
}

/// 解析标记列表在当前树中的大小。
#[tauri::command]
pub async fn disk_analyze_marks(
    scan_id: String,
    paths: Vec<String>,
) -> Result<Vec<MarkInfo>, String> {
    let scan = take_tree(&scan_id).ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    // canonicalize/locate 是文件系统访问，放到阻塞线程池避免卡住异步运行时
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|path| {
                let target = canonical_tree_path(path).unwrap_or_else(|| PathBuf::from(path));
                match tree::locate(&scan.tree, &scan.root, &target) {
                    Some(node) => MarkInfo {
                        path: path.clone(),
                        bytes: node.bytes,
                        files: node.files,
                        exists: true,
                    },
                    None => MarkInfo {
                        path: path.clone(),
                        bytes: 0,
                        files: 0,
                        exists: false,
                    },
                }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// 目标的危险度（来自树节点）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDanger {
    pub git_repos: u64,
    pub non_reclaimable_bytes: u64,
}

/// 目标的危险度（来自树节点）；树中定位不到时返回 None。
/// 定位失败必须按 fail-closed 处理：宁可拦截也不能放行整目录删除。
fn target_danger(scan: &ScanTree, path: &Path) -> Option<TargetDanger> {
    let (git_repos, non_reclaimable_bytes) = tree::locate(&scan.tree, &scan.root, path)
        .map(deskpet_disk::tree::danger_stats)?;
    Some(TargetDanger {
        git_repos,
        non_reclaimable_bytes,
    })
}

/// 整目录删除策略（文档"安全不变量"的收紧）：**整目录删除只对"整棵子树
/// 都是可回收内容"的目录开放**（danger_stats 归零，如 target/Caches/旧下载）。
/// 含 git 仓库、或任何非可回收内容（源码/文档等）的目录，两种模式都拦截——
/// 用户需要下钻只标记可回收部分，杜绝"误删整个工作目录"的可能。
/// 树中定位不到的目录同样拦截（fail-closed）。
fn apply_danger_policy(scan: &ScanTree, plan: &mut deskpet_disk::removal::Plan) {
    let mut dangerous: Vec<deskpet_disk::removal::Blocked> = Vec::new();
    plan.targets.retain(|target| {
        let Some(danger) = target_danger(scan, &target.path) else {
            dangerous.push(deskpet_disk::removal::Blocked {
                path: target.path.clone(),
                reason: "无法在扫描结果中定位该目录，请重新扫描".to_string(),
            });
            return false;
        };
        let hit = danger.git_repos > 0 || danger.non_reclaimable_bytes > 0;
        if hit {
            let mut reasons: Vec<String> = Vec::new();
            if danger.git_repos > 0 {
                reasons.push(format!(
                    "包含 {} 个 git 仓库（可能含未提交工作与历史）",
                    danger.git_repos
                ));
            }
            if danger.non_reclaimable_bytes > 0 {
                reasons.push(format!(
                    "包含约 {} 非可回收内容（源码/文档等）",
                    fmt_mb(danger.non_reclaimable_bytes)
                ));
            }
            reasons.push("请下钻只标记可回收部分".to_string());
            dangerous.push(deskpet_disk::removal::Blocked {
                path: target.path.clone(),
                reason: reasons.join("；"),
            });
        }
        !hit
    });
    plan.blocked.extend(dangerous);
    plan.reclaim = plan.targets.iter().map(|target| target.bytes).sum();
}

fn fmt_mb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

/// 从树缓存解析请求路径为 (真实路径, 字节, 是否目录)。
/// 未知路径/穿越路径必须整体拒绝，不能把原字符串原样交给删除层。
fn resolve_tree_paths(
    scan: &ScanTree,
    paths: &[String],
) -> Result<Vec<(PathBuf, u64, bool)>, String> {
    let mut requested = Vec::with_capacity(paths.len());
    for path in paths {
        let target = canonical_delete_path(path).unwrap_or_else(|| PathBuf::from(path));
        let node = tree::locate(&scan.tree, &scan.root, &target)
            .ok_or_else(|| "所选目录不在扫描结果中，请重新扫描".to_string())?;
        requested.push((target, node.bytes, true));
    }
    Ok(requested)
}

fn plan_response(
    plan: &deskpet_disk::removal::Plan,
    dangers: &HashMap<String, TargetDanger>,
) -> PlanResponse {
    let display = |path: &Path| path.to_string_lossy().to_string();
    PlanResponse {
        targets: plan
            .targets
            .iter()
            .map(|target| {
                let danger = dangers.get(&target.path.to_string_lossy().to_string());
                PlanTargetDto {
                    path: display(&target.path),
                    bytes: target.bytes,
                    is_dir: target.is_dir,
                    git_repos: danger.map(|d| d.git_repos),
                    non_reclaimable_bytes: danger.map(|d| d.non_reclaimable_bytes),
                }
            })
            .collect(),
        covered: plan
            .covered
            .iter()
            .map(|covered| PlanTargetDto {
                path: covered.path.to_string_lossy().to_string(),
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
                path: blocked.path.to_string_lossy().to_string(),
                error: blocked.reason.clone(),
            })
            .collect(),
        reclaim: plan.reclaim,
    }
}

/// 计算标记的删除计划（复核清单）。
#[tauri::command]
pub async fn disk_analyze_plan(
    scan_id: String,
    paths: Vec<String>,
) -> Result<PlanResponse, String> {
    if paths.is_empty() {
        return Err("请先标记要清理的目录".to_string());
    }
    let scan = take_tree(&scan_id).ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    let home =
        home_dir().ok_or_else(|| "无法确定家目录，已拒绝生成删除计划".to_string())?;
    // plan/危险度检查涉及文件系统访问与子树遍历，放到阻塞线程池
    let (plan, dangers) =
        tauri::async_runtime::spawn_blocking(move || -> Result<
            (deskpet_disk::removal::Plan, HashMap<String, TargetDanger>),
            String,
        > {
            let requested = resolve_tree_paths(&scan, &paths)?;
            let mut plan = deskpet_disk::removal::plan(&scan.root, &home, &requested);
            // 复核清单与删除执行使用同一套危险策略：含 git 仓库/非可回收内容的整目录
            // 移入拒绝清单（带原因），targets 只保留允许整体删除的项——所见即所删
            apply_danger_policy(&scan, &mut plan);
            let mut dangers = HashMap::new();
            for target in &plan.targets {
                if let Some(danger) = target_danger(&scan, &target.path) {
                    dangers.insert(target.path.to_string_lossy().to_string(), danger);
                }
            }
            Ok((plan, dangers))
        })
        .await
        .map_err(|e| e.to_string())??;
    Ok(plan_response(&plan, &dangers))
}

/// 删除标记项（trash/permanent），返回 SSE 进度地址。
#[tauri::command]
pub async fn disk_analyze_delete(
    scan_id: String,
    paths: Vec<String>,
    mode: String,
) -> Result<CleanupStream, String> {
    if paths.is_empty() {
        return Err("请先标记要清理的目录".to_string());
    }
    let removal_mode = match mode.as_str() {
        "trash" => RemovalMode::Trash,
        "permanent" => RemovalMode::Permanent,
        other => return Err(format!("未知删除模式: {other}")),
    };
    let scan = take_tree(&scan_id).ok_or_else(|| "扫描结果已失效，请重新扫描".to_string())?;
    let home = home_dir().ok_or_else(|| "无法确定家目录，已拒绝删除".to_string())?;
    // plan/危险度检查涉及文件系统访问与子树遍历，放到阻塞线程池
    let plan_scan = scan.clone();
    let plan_home = home.clone();
    let plan = tauri::async_runtime::spawn_blocking(
        move || -> Result<deskpet_disk::removal::Plan, String> {
            let requested = resolve_tree_paths(&plan_scan, &paths)?;
            let mut plan = deskpet_disk::removal::plan(&plan_scan.root, &plan_home, &requested);
            if plan.targets.is_empty() {
                return Err("所选项目均未通过安全检查，请查看被拒绝原因".to_string());
            }
            // 整目录删除的口子收紧：只有**整棵子树都是可回收内容**的目录才允许整体删除
            // （danger_stats 归零）；含 git 仓库或任何非可回收内容（源码/文档等）的目录，
            // 两种模式都拦截，提示用户下钻只标记可回收部分
            apply_danger_policy(&plan_scan, &mut plan);
            Ok(plan)
        },
    )
    .await
    .map_err(|e| e.to_string())??;
    if plan.targets.is_empty() {
        return Err(
            "标记的目录包含 git 仓库或非可回收内容（源码/文档等），不能整体删除；请下钻只标记可回收部分"
                .to_string(),
        );
    }

    let svc = service().await?;
    let (job_id, tx, cancel) = svc.create_job();
    let url = format!("http://127.0.0.1:{}/cleanup/events/{}", svc.port, job_id);
    let job_service = svc.clone();
    let job_entry_id = job_id.clone();
    // 计划里所有被拒绝项都来自本次标记；逐项透传为 Deleted{ok:false}，
    // 并在 DeleteDone 的 failed/failures 中保留，避免部分删除时最终提示
    // “全部成功”而实际上有目标被危险策略拦截。
    let policy_blocked: Vec<(String, String)> = plan
        .blocked
        .iter()
        .map(|blocked| {
            (
                blocked.path.to_string_lossy().to_string(),
                blocked.reason.clone(),
            )
        })
        .collect();

    tauri::async_runtime::spawn_blocking(move || {
        let _ = tx.send(CleanupEvent::Started {
            mode: "delete",
            root: scan.display_root.to_string_lossy().to_string(),
        });
        for (path, reason) in &policy_blocked {
            let _ = tx.send(CleanupEvent::Deleted {
                path: path.clone(),
                ok: false,
                bytes: 0,
                error: Some(reason.clone()),
            });
        }
        let mode_label = match removal_mode {
            RemovalMode::Trash => "trash",
            RemovalMode::Permanent => "permanent",
        };
        let outcome = deskpet_disk::removal::execute(
            &plan,
            removal_mode,
            &home,
            &cancel,
            &mut |target| {
                let _ = tx.send(CleanupEvent::Deleting {
                    path: target.path.to_string_lossy().to_string(),
                    bytes: target.bytes,
                    mode: mode_label,
                });
            },
            &mut |target, ok, bytes, error| {
                let _ = tx.send(CleanupEvent::Deleted {
                    path: target.path.to_string_lossy().to_string(),
                    ok,
                    bytes,
                    error: error.map(str::to_string),
                });
            },
        );
        let mut failures: Vec<FailureDto> = policy_blocked
            .iter()
            .map(|(path, error)| FailureDto {
                path: path.clone(),
                error: error.clone(),
            })
            .collect();
        failures.extend(outcome.failures.iter().map(|failure| FailureDto {
            path: failure.path.to_string_lossy().to_string(),
            error: failure.error.clone(),
        }));
        let _ = tx.send(CleanupEvent::DeleteDone {
            removed: outcome.removed,
            failed: outcome.failed.saturating_add(policy_blocked.len() as u64),
            bytes: outcome.estimated_bytes,
            freed_bytes: outcome.free_after.saturating_sub(outcome.free_before),
            free_before: outcome.free_before,
            free_after: outcome.free_after,
            failures,
        });
        // 删除后树已过期：直接丢弃，前端会自动重扫
        drop_tree(&scan_id);
        // 删除任务结束（完成/失败）后延迟清理 job 表
        retire_job(job_service, job_entry_id);
    });

    Ok(CleanupStream { job_id, url })
}

/// 释放树缓存（窗口关闭时调用）。
#[tauri::command]
pub async fn disk_analyze_close(scan_id: String) -> Result<(), String> {
    drop_tree(&scan_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::AtomicBool};

    #[cfg(unix)]
    #[test]
    fn display_alias_resolves_but_final_symlink_is_not_followed_for_delete() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        let alias = tmp.path().join("alias");
        symlink(&root, &alias).unwrap();
        let via_alias = alias.join("sub");
        assert_eq!(
            canonical_delete_path(via_alias.to_str().unwrap()),
            Some(root.join("sub"))
        );

        let final_link = root.join("final-link");
        symlink(root.join("sub"), &final_link).unwrap();
        assert_eq!(
            canonical_delete_path(final_link.to_str().unwrap()),
            Some(final_link.clone()),
            "删除路径只规范化父目录，最后一级符号链接不能被跟随"
        );
        assert_eq!(
            canonical_tree_path(final_link.to_str().unwrap()),
            Some(root.join("sub"))
        );
        assert!(
            canonical_delete_path(&format!("{}/../escape", root.display())).is_none(),
            "含 .. 的展示路径必须拒绝"
        );
    }

    /// 整目录删除策略：含 .git 或非可回收内容的目录必须被移入 blocked。
    /// 临时目录 + 真实 scan_tree 构造，直接调用 apply_danger_policy 验证。
    #[test]
    fn danger_policy_blocks_wholesale_delete_of_work_dirs() {
        use deskpet_disk::tree::{TreeLimits, scan_tree};

        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap().join("home");
        // workspace/proj：源码 + target(可再生) + .git 仓库
        let proj = home.join("workspace/proj");
        fs::create_dir_all(proj.join("target/debug")).unwrap();
        fs::create_dir_all(proj.join(".git")).unwrap();
        fs::create_dir_all(proj.join("src")).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]").unwrap();
        fs::write(proj.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(proj.join("target/debug/app"), vec![0u8; 4096]).unwrap();
        // 纯缓存目录（整棵子树可再生）——整体删除应放行
        let caches = home.join("Library/Caches");
        fs::create_dir_all(&caches).unwrap();
        fs::write(caches.join("f.bin"), vec![0u8; 512]).unwrap();

        let scan = Arc::new(
            scan_tree(
                &home,
                &home,
                Some(&home),
                TreeLimits::default(),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .expect("扫描应完成"),
        );

        let workspace_path = home.join("workspace");
        let caches_path = home.join("Library/Caches");
        let requested: Vec<(PathBuf, u64, bool)> = vec![
            (workspace_path.clone(), 0, true),
            (caches_path.clone(), 0, true),
        ];
        let mut plan = deskpet_disk::removal::plan(&scan.root, &home, &requested);
        assert_eq!(plan.targets.len(), 2, "两者最初都进入计划");

        apply_danger_policy(&scan, &mut plan);

        // workspace（含 .git 与源码）被拦截，Caches（纯可再生）放行
        assert!(
            !plan.targets.iter().any(|t| t.path == workspace_path),
            "工作目录不允许整体删除"
        );
        assert!(
            plan.targets.iter().any(|t| t.path == caches_path),
            "纯可再生目录允许整体删除"
        );
        assert_eq!(plan.blocked.len(), 1);
        assert!(plan.blocked[0].reason.contains("git 仓库"));
        // 可释放字节只统计放行项
        assert_eq!(
            plan.reclaim,
            plan.targets.iter().map(|t| t.bytes).sum::<u64>()
        );
    }
}
