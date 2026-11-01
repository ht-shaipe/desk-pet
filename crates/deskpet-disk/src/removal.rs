//! 删除：安全守卫、删除计划、回收站/永久删除。
//!
//! 安全不变量（见 docs/disktree-integration-plan.md 第 8 节）：
//! - 只删扫描根之下、用户明确标记的路径；根、家目录、系统树一律拒绝；
//! - 挂载点本身、内部包含挂载点的目录都拒绝（不跨挂载点删除）；
//! - 永久删除从扫描根逐级 `openat(O_NOFOLLOW)` + `unlinkat`，符号链接拒绝；
//! - 删前复查：重新读挂载表、重新检查目标身份，扫描后变化就不删；
//! - 空选择不删除（命令层强制显式非空路径列表）。

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde::Serialize;

use crate::space::space_info;

/// 删除模式：回收站（默认，可恢复）/ 永久删除（显式选择）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemovalMode {
    Trash,
    Permanent,
}

/// 计划内的删除目标。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub path: PathBuf,
    pub bytes: u64,
    pub is_dir: bool,
    /// 所在设备（删前复查跨设备变化）
    pub dev: u64,
    /// inode：删前复查目标身份，避免同名文件/目录被替换后误删
    pub ino: u64,
}

/// 被父目录目标吸收的标记（不再单独删除，避免重复计费）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Covered {
    pub path: PathBuf,
    pub bytes: u64,
}

/// 被安全规则拒绝的项及原因。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    pub path: PathBuf,
    pub reason: String,
}

/// 删除计划：真正要删的、被吸收的、被拒绝的，一目了然。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub root: PathBuf,
    pub targets: Vec<Target>,
    pub covered: Vec<Covered>,
    pub blocked: Vec<Blocked>,
    /// 同卷可释放（分配大小口径）
    pub reclaim: u64,
}

/// Unix 系统树：一律拒绝删除/扫描。允许 /private 下的临时目录（macOS
/// 的 `tempfile` 与用户临时文件），但只限明确的临时前缀。
const SYSTEM_TREES: &[&str] = &[
    "/System",
    "/Library",
    "/private",
    "/usr",
    "/bin",
    "/sbin",
    "/opt/homebrew",
    "/nix",
    "/cores",
    "/Network",
    "/dev",
    "/etc",
    "/proc",
    "/sys",
    "/run",
    "/boot",
    "/root",
];

/// 永久删除的最大递归深度。
const MAX_TRAVERSE_DEPTH: usize = 512;

/// 允许位于 `/private` 系统树前缀内的临时目录。
fn is_safe_temp_path(path: &Path) -> bool {
    [
        "/private/tmp",
        "/private/var/folders",
        "/tmp",
        "/var/folders",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

/// 路径命中的系统树（纯路径，不访问文件系统）。
pub fn protected_system_tree(path: &Path) -> Option<&'static str> {
    SYSTEM_TREES.iter().copied().find(|sys| {
        let sys = Path::new(sys);
        (path == sys || path.starts_with(sys))
            && !(sys == Path::new("/private") && is_safe_temp_path(path))
    })
}

/// 拒绝把系统目录当作扫描根。扫描根由命令层 canonicalize 后传入。
pub fn validate_scan_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("扫描根必须是绝对路径".to_string());
    }
    if root == Path::new("/") {
        return Err("不能扫描文件系统根目录".to_string());
    }
    if let Some(sys) = protected_system_tree(root) {
        return Err(format!("不能扫描系统目录 {sys}"));
    }
    Ok(())
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            std::path::Component::RootDir => out.push(Path::new("/")),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return None,
            std::path::Component::Normal(part) => out.push(part),
        }
    }
    Some(out)
}

/// 删除守卫。挂载表在构造时读取一次；删除执行前会重新构造复查。
pub struct Guard {
    root: PathBuf,
    home: PathBuf,
    mounts: Vec<PathBuf>,
}

impl Guard {
    /// 读取真实挂载表构造守卫。
    pub fn new(root: &Path, home: &Path) -> Self {
        Self::with_mounts(root, home, read_mount_points())
    }

    /// 以给定挂载表构造守卫（测试用）。
    pub fn with_mounts(root: &Path, home: &Path, mounts: Vec<PathBuf>) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            home: home.canonicalize().unwrap_or_else(|_| home.to_path_buf()),
            mounts,
        }
    }

    /// 纯路径逻辑守卫：不访问文件系统，全部可测试。
    pub fn check_path(&self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() {
            return Err("非绝对路径".to_string());
        }
        let path = normalize_absolute(path).ok_or_else(|| "路径包含 ..".to_string())?;
        if path == Path::new("/") {
            return Err("文件系统根目录".to_string());
        }
        if path == self.root {
            return Err("扫描根目录".to_string());
        }
        if path == self.home {
            return Err("家目录".to_string());
        }
        if self.home.starts_with(&path) {
            return Err("包含家目录的目录".to_string());
        }
        if let Some(sys) = protected_system_tree(&path) {
            return Err(format!("系统目录 {sys}"));
        }
        if !path.starts_with(&self.root) {
            return Err("扫描根之外".to_string());
        }
        for mount in &self.mounts {
            if path == mount.as_path() {
                return Err(format!("挂载点 {}", mount.display()));
            }
            if mount.starts_with(&path) {
                return Err(format!("内部包含挂载点 {}", mount.display()));
            }
            // 目标位于一个已知挂载点内部（扫描根本身可以是挂载点）。
            if path.starts_with(mount) && !self.root.starts_with(mount) {
                return Err(format!("挂载点内部 {}", mount.display()));
            }
        }
        Ok(())
    }
}

/// 读取当前挂载点列表（macOS `getmntinfo`；Linux `/proc/self/mounts`）。
pub fn read_mount_points() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        unsafe {
            let mut mntbuf: *mut libc::statfs = std::ptr::null_mut();
            let count = libc::getmntinfo(&mut mntbuf, libc::MNT_WAIT);
            if count <= 0 || mntbuf.is_null() {
                return Vec::new();
            }
            (0..count as usize)
                .filter_map(|index| {
                    let entry = *mntbuf.add(index);
                    let bytes = std::slice::from_raw_parts(
                        entry.f_mntonname.as_ptr().cast::<u8>(),
                        entry.f_mntonname.len(),
                    );
                    let path = std::ffi::CStr::from_bytes_until_nul(bytes).ok()?;
                    let text = path.to_string_lossy().to_string();
                    if text.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(text))
                    }
                })
                .collect()
        }
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(content) = fs::read_to_string("/proc/self/mounts") else {
            return Vec::new();
        };
        content
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .map(|raw| PathBuf::from(unescape_mounts_field(raw)))
            .filter(|path| !path.as_os_str().is_empty())
            .collect()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn unescape_mounts_field(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if let Some(text) = bytes.get(index + 1..index + 4) {
                if let Ok(value) = u8::from_str_radix(&String::from_utf8_lossy(text), 8) {
                    out.push(value);
                    index += 4;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 根据用户标记的路径构建删除计划（守卫 + 存在性 + 吸收关系）。
///
/// `requested` 为 `(真实绝对路径, 字节, 是否目录)`，必须非空（空选择不删除）。
pub fn plan(root: &Path, home: &Path, requested: &[(PathBuf, u64, bool)]) -> Plan {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut plan = Plan {
        root: canonical_root.clone(),
        targets: Vec::new(),
        covered: Vec::new(),
        blocked: Vec::new(),
        reclaim: 0,
    };
    let guard = Guard::new(&canonical_root, home);
    for (raw_path, bytes, is_dir) in requested {
        // 先做词法归一化；含 `..` 的路径直接拒绝，后续再 canonicalize 父目录
        // 消除祖先符号链接，保证真正删除的路径仍在扫描根之内。
        let Some(path) = normalize_absolute(raw_path) else {
            plan.blocked.push(Blocked {
                path: raw_path.clone(),
                reason: "路径包含 ..".to_string(),
            });
            continue;
        };
        let Some(parent) = path.parent() else {
            plan.blocked.push(Blocked {
                path: raw_path.clone(),
                reason: "没有父目录".to_string(),
            });
            continue;
        };
        let Some(name) = path.file_name() else {
            plan.blocked.push(Blocked {
                path: raw_path.clone(),
                reason: "没有文件名".to_string(),
            });
            continue;
        };
        let parent = match parent.canonicalize() {
            Ok(parent) => parent,
            Err(err) => {
                plan.blocked.push(Blocked {
                    path: raw_path.clone(),
                    reason: format!("无法解析父目录: {err}"),
                });
                continue;
            }
        };
        if !parent.starts_with(&canonical_root) {
            plan.blocked.push(Blocked {
                path: raw_path.clone(),
                reason: "解析后位于扫描根之外".to_string(),
            });
            continue;
        }
        let path = parent.join(name);
        if let Err(reason) = guard.check_path(&path) {
            plan.blocked.push(Blocked {
                path: raw_path.clone(),
                reason,
            });
            continue;
        }
        match fs::symlink_metadata(&path) {
            Err(err) => plan.blocked.push(Blocked {
                path,
                reason: format!("无法读取: {err}"),
            }),
            Ok(meta) if meta.file_type().is_symlink() => plan.blocked.push(Blocked {
                path,
                reason: "符号链接".to_string(),
            }),
            Ok(meta) if meta.is_dir() != *is_dir => plan.blocked.push(Blocked {
                path,
                reason: "类型已变化".to_string(),
            }),
            Ok(meta) => {
                let (dev, ino) = identity_of(&meta);
                plan.targets.push(Target {
                    path,
                    bytes: *bytes,
                    is_dir: *is_dir,
                    dev,
                    ino,
                });
            }
        }
    }

    // 目录目标吸收内部标记：浅的在前，后面路径若在某个已接受目录内则计入 covered
    plan.targets
        .sort_by_key(|target| target.path.components().count());
    let mut accepted: Vec<Target> = Vec::new();
    for target in plan.targets.drain(..) {
        if accepted
            .iter()
            .any(|accepted| accepted.is_dir && target.path.starts_with(&accepted.path))
        {
            plan.covered.push(Covered {
                path: target.path,
                bytes: target.bytes,
            });
        } else {
            accepted.push(target);
        }
    }
    plan.targets = accepted;
    plan.reclaim = plan.targets.iter().map(|target| target.bytes).sum();
    plan
}

/// 删除执行结果。`freed_bytes` 来自 statvfs 前后实测，可能与估算不等
/// （APFS 克隆/快照、其他进程写入、硬链接在扫描根外）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutcome {
    pub removed: u64,
    pub failed: u64,
    pub estimated_bytes: u64,
    pub free_before: u64,
    pub free_after: u64,
    pub failures: Vec<Failure>,
}

/// 单项失败明细。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub path: PathBuf,
    pub error: String,
}

/// 执行删除计划。
///
/// `on_start`/`on_done` 供上层逐项转发 SSE 进度；被取消时提前返回，
/// 剩余目标保持原样。
pub fn execute(
    plan: &Plan,
    mode: RemovalMode,
    home: &Path,
    cancel: &AtomicBool,
    on_start: &mut dyn FnMut(&Target),
    on_done: &mut dyn FnMut(&Target, bool, u64, Option<&str>),
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome {
        removed: 0,
        failed: 0,
        estimated_bytes: 0,
        free_before: space_info(&plan.root).map_or(0, |info| info.available),
        free_after: 0,
        failures: Vec::new(),
    };
    if plan.targets.is_empty() {
        outcome.free_after = outcome.free_before;
        return outcome;
    }

    // 删前复查：重新读挂载表，重新过一遍守卫
    let guard = Guard::new(&plan.root, home);
    let mut valid_targets: Vec<&Target> = Vec::new();
    for target in &plan.targets {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Err(reason) = guard.check_path(&target.path) {
            outcome.failed += 1;
            let error = format!("删前复查被拒绝: {reason}");
            on_done(target, false, 0, Some(error.as_str()));
            outcome.failures.push(Failure {
                path: target.path.clone(),
                error,
            });
            continue;
        }
        // 父目录复查：计划阶段已 canonicalize；若中途祖先被替换成符号链接，
        // 回收站路径会指向别处，直接拒绝（永久删除还有 fd 级保护）。
        if let Some(parent) = target.path.parent() {
            match parent.canonicalize() {
                Ok(canonical) if canonical == parent => {}
                _ => {
                    outcome.failed += 1;
                    let error = "父目录已变化".to_string();
                    on_done(target, false, 0, Some(error.as_str()));
                    outcome.failures.push(Failure {
                        path: target.path.clone(),
                        error,
                    });
                    continue;
                }
            }
        }
        // 身份复查：消失视为已完成，变链接/变设备/变 inode 拒绝
        let meta = match fs::symlink_metadata(&target.path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                outcome.removed += 1;
                on_done(target, true, 0, None);
                continue;
            }
            Err(err) => {
                outcome.failed += 1;
                let error = format!("无法读取: {err}");
                on_done(target, false, 0, Some(error.as_str()));
                outcome.failures.push(Failure {
                    path: target.path.clone(),
                    error,
                });
                continue;
            }
            Ok(meta) => meta,
        };
        if meta.file_type().is_symlink() {
            outcome.failed += 1;
            let error = "删除前发现符号链接".to_string();
            on_done(target, false, 0, Some(error.as_str()));
            outcome.failures.push(Failure {
                path: target.path.clone(),
                error,
            });
            continue;
        }
        if meta.is_dir() != target.is_dir {
            outcome.failed += 1;
            let error = "类型已变化".to_string();
            on_done(target, false, 0, Some(error.as_str()));
            outcome.failures.push(Failure {
                path: target.path.clone(),
                error,
            });
            continue;
        }
        let (dev, ino) = identity_of(&meta);
        if dev != target.dev || ino != target.ino {
            outcome.failed += 1;
            let error = "目标身份已变化".to_string();
            on_done(target, false, 0, Some(error.as_str()));
            outcome.failures.push(Failure {
                path: target.path.clone(),
                error,
            });
            continue;
        }

        valid_targets.push(target);
    }

    // 取消可能发生在预检过程中。已经通过预检的目标也不能再写盘：
    // 用户点“停止”后，剩余目标（包括刚通过检查的）都应保持原样。
    if cancel.load(Ordering::Relaxed) {
        valid_targets.clear();
    }

    // 回收站整批提交 / 永久删除逐项执行

    match mode {
        RemovalMode::Trash => {
            // 删除开始前才发 Started/Deleting，避免预检阶段取消后前端留下
            // “正在删除”的假状态。这里的回调不应阻塞。
            for target in &valid_targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                on_start(target);
            }
            if cancel.load(Ordering::Relaxed) {
                valid_targets.clear();
            }
            // 整批提交：系统音效只播一次（逐项调用会连续响 N 声）
            let paths: Vec<&Path> = valid_targets
                .iter()
                .map(|target| target.path.as_path())
                .collect();
            match trash_batch(&paths) {
                Ok(()) => {
                    for target in valid_targets {
                        outcome.removed += 1;
                        outcome.estimated_bytes =
                            outcome.estimated_bytes.saturating_add(target.bytes);
                        on_done(target, true, target.bytes, None);
                    }
                }
                Err(TrashBatchError::Failed(batch_error)) => {
                    // 整批失败（可能已部分成功）：退回逐项回收，已不存在的项
                    // 视为已成功，避免把部分成功误报成失败（回退路径会多响几声）。
                    // 整批错误仅作诊断线索：逐项回退会给出更精确的失败原因。
                    eprintln!("trash batch failed, falling back per-item: {batch_error}");
                    for target in &valid_targets {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        if matches!(
                            fs::symlink_metadata(&target.path),
                            Err(err) if err.kind() == io::ErrorKind::NotFound
                        ) {
                            outcome.removed += 1;
                            outcome.estimated_bytes =
                                outcome.estimated_bytes.saturating_add(target.bytes);
                            on_done(target, true, target.bytes, None);
                            continue;
                        }
                        match delete_to_trash(&target.path) {
                            Ok(()) => {
                                outcome.removed += 1;
                                outcome.estimated_bytes =
                                    outcome.estimated_bytes.saturating_add(target.bytes);
                                on_done(target, true, target.bytes, None);
                            }
                            Err(error) => {
                                outcome.failed += 1;
                                on_done(target, false, 0, Some(error.as_str()));
                                outcome.failures.push(Failure {
                                    path: target.path.clone(),
                                    error,
                                });
                            }
                        }
                    }
                }
                Err(TrashBatchError::Timeout) => {
                    // 超时：系统可能仍在后台执行或已部分完成。此时逐项回退会把
                    // 已移入的目标再次入篓（重复占空间），因此全部按失败上报，
                    // 提示用户稍后在废纸篓确认实际结果。
                    let error = "移入回收站超时，请稍后在废纸篓确认结果".to_string();
                    for target in valid_targets {
                        outcome.failed += 1;
                        on_done(target, false, 0, Some(error.as_str()));
                        outcome.failures.push(Failure {
                            path: target.path.clone(),
                            error: error.clone(),
                        });
                    }
                }
            }
        }
        RemovalMode::Permanent => {
            for target in valid_targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                on_start(target);
                // 回调可能触发取消（例如测试/上层状态切换）；取消后不得落盘。
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let result = delete_permanent(target, &plan.root, cancel);
                match result {
                    Ok(()) => {
                        outcome.removed += 1;
                        outcome.estimated_bytes =
                            outcome.estimated_bytes.saturating_add(target.bytes);
                        on_done(target, true, target.bytes, None);
                    }
                    Err(error) => {
                        outcome.failed += 1;
                        let error = error.to_string();
                        on_done(target, false, 0, Some(error.as_str()));
                        outcome.failures.push(Failure {
                            path: target.path.clone(),
                            error,
                        });
                    }
                }
            }
        }
    }

    outcome.free_after = space_info(&plan.root).map_or(outcome.free_before, |info| info.available);
    outcome
}

/// 回收站删除（逐项）：macOS 走系统 Trash（NSFileManager），可在 Finder 恢复。
/// 注意：每次调用系统都会播放一次废纸篓音效，仅作批量失败时的回退路径。
fn delete_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|err| format!("移入回收站失败: {err}"))
}

/// 回收站整批提交的错误。超时与其他失败分开：超时意味着系统可能仍在
/// 执行或已部分完成，不能进入逐项回退（会把已入篓的目标再次移入）。
enum TrashBatchError {
    Timeout,
    Failed(String),
}

/// 回收站批量删除（Finder 同款）：`NSWorkspace.recycleURLs` 一次提交全部目标，
/// 系统音效只播一次。完成前阻塞调用线程（任务运行在 spawn_blocking 工作线程，
/// 完成回调由主队列触发）。
#[cfg(target_os = "macos")]
fn trash_batch(paths: &[&Path]) -> Result<(), TrashBatchError> {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSString, NSURL};
    use std::ptr::NonNull;
    use std::sync::mpsc;

    if paths.is_empty() {
        return Ok(());
    }
    let urls: Vec<Retained<NSURL>> = paths
        .iter()
        .map(|path| {
            NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&path.to_string_lossy()),
                path.is_dir(),
            )
        })
        .collect();
    let array = NSArray::from_retained_slice(&urls);
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let handler = RcBlock::new(
        move |_new_urls: NonNull<NSDictionary<NSURL, NSURL>>, error: *mut NSError| {
            // new_urls 是"原 URL → 回收站内新 URL"的映射，这里只关心成败
            if error.is_null() {
                let _ = tx.send(Ok(()));
            } else {
                let description = unsafe {
                    error
                        .as_ref()
                        .map(|err| err.localizedDescription().to_string())
                        .unwrap_or_else(|| "unknown error".to_string())
                };
                let _ = tx.send(Err(description));
            }
        },
    );
    NSWorkspace::sharedWorkspace().recycleURLs_completionHandler(&array, Some(&handler));
    match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(result) => result.map_err(TrashBatchError::Failed),
        Err(_) => Err(TrashBatchError::Timeout),
    }
}

#[cfg(not(target_os = "macos"))]
fn trash_batch(paths: &[&Path]) -> Result<(), TrashBatchError> {
    for path in paths {
        delete_to_trash(path).map_err(TrashBatchError::Failed)?;
    }
    Ok(())
}

/// 永久删除。Unix 走 openat(O_NOFOLLOW)/unlinkat 的单文件系统遍历；
/// 其他平台先回退 `remove_dir_all`/`remove_file`（配合删前复查）。
fn delete_permanent(target: &Target, root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    #[cfg(unix)]
    {
        delete_permanent_unix(target, root, cancel).map_err(|err| err.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (target, root, cancel);
        if target.is_dir {
            fs::remove_dir_all(&target.path)
        } else {
            fs::remove_file(&target.path)
        }
        .map_err(|err| err.to_string())
    }
}

/// 从扫描根开始逐级 `openat(O_NOFOLLOW|O_DIRECTORY)`，再经父目录 fd
/// `unlinkat`。这样即使删除过程中祖先目录被换成符号链接，也不会逃出扫描根。
#[cfg(unix)]
fn delete_permanent_unix(
    target: &Target,
    root: &Path,
    cancel: &AtomicBool,
) -> Result<(), io::Error> {
    use std::os::unix::ffi::OsStrExt;

    let relative = target
        .path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("目标不在扫描根内"))?;
    let Some(name) = relative.file_name() else {
        return Err(io::Error::other("目标没有文件名"));
    };
    let name = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::other("目标名称包含 NUL"))?;
    let name = name.as_c_str();
    let parent_rel = relative.parent().unwrap_or_else(|| Path::new(""));
    let mut dir_fd = rustix::fs::openat(
        rustix::fs::CWD,
        root,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| io::Error::other(format!("open root {:?}: {err}", root)))?;
    for component in parent_rel.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(io::Error::other("目标路径包含非普通组件"));
        };
        dir_fd = rustix::fs::openat(
            &dir_fd,
            part,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|err| io::Error::other(format!("open parent {:?}: {err}", part)))?;
    }
    let stat = rustix::fs::statat(&dir_fd, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|err| io::Error::other(format!("stat target {:?}: {err}", name)))?;
    const S_IFMT: u32 = 0o170_000;
    const S_IFLNK: u32 = 0o120_000;
    const S_IFDIR: u32 = 0o040_000;
    let mode = u32::from(stat.st_mode);
    if mode & S_IFMT == S_IFLNK {
        return Err(io::Error::other("目标是符号链接"));
    }
    if (mode & S_IFMT == S_IFDIR) != target.is_dir {
        return Err(io::Error::other("目标类型已变化"));
    }
    if stat.st_dev as u64 != target.dev || stat.st_ino as u64 != target.ino {
        return Err(io::Error::other("目标身份已变化"));
    }

    if target.is_dir {
        remove_dir_contents(&dir_fd, name, target.dev, 0, cancel)?;
        rustix::fs::unlinkat(&dir_fd, name, rustix::fs::AtFlags::REMOVEDIR)
            .map_err(|err| io::Error::other(format!("rmdir {:?}: {err}", name)))?;
    } else {
        rustix::fs::unlinkat(&dir_fd, name, rustix::fs::AtFlags::empty())
            .map_err(|err| io::Error::other(format!("unlink {:?}: {err}", name)))?;
    }
    Ok(())
}

/// 删除目录内容（不跨挂载点、不跟随符号链接）。
///
/// 目录以 `O_NOFOLLOW` 打开，所有 stat/unlink 都相对目录 fd 完成；
/// 中途被换成符号链接或跨设备都会失败，不会跟随。
#[cfg(unix)]
fn remove_dir_contents(
    parent_fd: &rustix::fd::OwnedFd,
    dir_name: &std::ffi::CStr,
    expected_dev: u64,
    depth: usize,
    cancel: &AtomicBool,
) -> Result<(), io::Error> {
    if depth > MAX_TRAVERSE_DEPTH {
        return Err(io::Error::other("目录层级过深"));
    }
    let dir_fd = rustix::fs::openat(
        parent_fd,
        dir_name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|err| io::Error::other(format!("openat {:?}: {err}", dir_name)))?;
    let stat = rustix::fs::fstat(&dir_fd)?;
    if stat.st_dev as u64 != expected_dev {
        return Err(io::Error::other("跨挂载点"));
    }
    let mut dir = rustix::fs::Dir::read_from(&dir_fd)
        .map_err(|err| io::Error::other(format!("readdir {:?}: {err}", dir_name)))?;
    while let Some(entry) = dir.read() {
        let entry = entry.map_err(|err| io::Error::other(format!("dirent: {err}")))?;
        // macOS 的 readdir 会返回 "." 与 ".."，不过滤会无限递归。
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("已取消"));
        }
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            rustix::fs::unlinkat(&dir_fd, name, rustix::fs::AtFlags::empty())
                .map_err(|err| io::Error::other(format!("unlink {:?}: {err}", name)))?;
        } else if file_type.is_dir() {
            let child_stat =
                rustix::fs::statat(&dir_fd, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(|err| io::Error::other(format!("statat {:?}: {err}", name)))?;
            if child_stat.st_dev as u64 != expected_dev {
                return Err(io::Error::other("目录内包含挂载点"));
            }
            remove_dir_contents(&dir_fd, name, expected_dev, depth + 1, cancel)?;
            rustix::fs::unlinkat(&dir_fd, name, rustix::fs::AtFlags::REMOVEDIR)
                .map_err(|err| io::Error::other(format!("rmdir {:?}: {err}", name)))?;
        } else {
            rustix::fs::unlinkat(&dir_fd, name, rustix::fs::AtFlags::empty())
                .map_err(|err| io::Error::other(format!("unlink {:?}: {err}", name)))?;
        }
    }
    Ok(())
}

fn identity_of(meta: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("proj/target/debug")).unwrap();
        fs::write(root.join("proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("proj/target/debug/app"), vec![0u8; 512]).unwrap();
        fs::write(root.join("proj/target/keep.txt"), "keep").unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/app.log"), "hello").unwrap();
        fs::create_dir_all(root.join("neighbor")).unwrap();
        fs::write(root.join("neighbor/value.txt"), "value").unwrap();
        tmp
    }

    fn requested(root: &Path, rel: &[&str]) -> Vec<(PathBuf, u64, bool)> {
        rel.iter()
            .map(|item| {
                let path = root.join(item);
                let meta = fs::symlink_metadata(&path).unwrap();
                (path, meta.len(), meta.is_dir())
            })
            .collect()
    }

    #[test]
    fn guard_refuses_roots_home_and_system_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("deep/nested/home");
        fs::create_dir_all(&home).unwrap();
        let guard = Guard::with_mounts(
            &root,
            &home,
            vec![root.join("mnt"), root.join("contains-mnt/usb")],
        );

        assert_eq!(
            guard.check_path(Path::new("/")),
            Err("文件系统根目录".to_string())
        );
        assert_eq!(guard.check_path(&root), Err("扫描根目录".to_string()));
        assert_eq!(
            guard.check_path(Path::new("relative")),
            Err("非绝对路径".to_string())
        );
        assert_eq!(guard.check_path(&home), Err("家目录".to_string()));
        // 家目录的祖先目录（包含家目录）拒绝
        assert_eq!(
            guard.check_path(&root.join("deep")),
            Err("包含家目录的目录".to_string())
        );
        assert_eq!(
            guard.check_path(Path::new("/System/Library")),
            Err("系统目录 /System".to_string())
        );
        assert_eq!(
            guard.check_path(Path::new("/usr/bin")),
            Err("系统目录 /usr".to_string())
        );
        assert_eq!(
            guard.check_path(&root.join("mnt")),
            Err(format!("挂载点 {}", root.join("mnt").display()))
        );
        assert_eq!(
            guard.check_path(&root.join("contains-mnt")),
            Err(format!(
                "内部包含挂载点 {}",
                root.join("contains-mnt/usb").display()
            ))
        );
        // 扫描根内的普通目录允许
        assert!(guard.check_path(&root.join("neighbor")).is_ok());
        // 家目录下的子目录允许（家目录本身不允许）
        assert!(guard.check_path(&home.join("sub")).is_ok());
    }

    #[test]
    fn safe_temp_roots_are_allowed_but_system_trees_are_not() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let guard = Guard::with_mounts(&root, &root.join("home"), Vec::new());
        assert!(validate_scan_root(&root).is_ok(), "临时目录应可扫描");
        assert!(guard.check_path(&root.join("x")).is_ok());
        assert!(validate_scan_root(Path::new("/")).is_err());
        assert!(validate_scan_root(Path::new("/System")).is_err());
        assert!(validate_scan_root(Path::new("/private/etc")).is_err());
        assert!(guard.check_path(Path::new("/usr/bin")).is_err());
        assert!(
            guard
                .check_path(&root.join("neighbor/../neighbor"))
                .is_err()
        );
    }

    #[test]
    fn guard_blocks_targets_inside_mounts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let guard = Guard::with_mounts(&root, &root.join("home"), vec![root.join("mnt")]);
        assert_eq!(
            guard.check_path(&root.join("mnt/sub")),
            Err(format!("挂载点内部 {}", root.join("mnt").display()))
        );
    }

    #[test]
    fn plan_rejects_parent_dir_and_ancestor_symlink_escape() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("victim.txt"), "victim").unwrap();

        let traversal = root.join("neighbor/../../victim.txt");
        let out_plan = plan(&root, &home, &[(traversal, 1, false)]);
        assert_eq!(out_plan.blocked.len(), 1);
        assert_eq!(out_plan.blocked[0].reason, "路径包含 ..");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.join("outside-link")).unwrap();
            let victim = root.join("outside-link/victim.txt");
            let out_plan = plan(&root, &home, &[(victim, 1, false)]);
            assert_eq!(out_plan.blocked.len(), 1);
            assert_eq!(out_plan.blocked[0].reason, "解析后位于扫描根之外");
            assert!(outside.path().join("victim.txt").exists());
        }
    }

    #[test]
    fn plan_blocks_and_covers() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");

        // 根本身 + 扫描根之外的路径被拒
        let out_plan = plan(&root, &home, &[(root.clone(), 0, true)]);
        assert_eq!(out_plan.blocked.len(), 1);
        assert!(out_plan.targets.is_empty());

        // 父目录吸收内部标记
        let out_plan = plan(
            &root,
            &home,
            &requested(&root, &["proj/target", "proj/target/debug"]),
        );
        assert_eq!(out_plan.targets.len(), 1);
        assert!(out_plan.targets[0].path.ends_with("proj/target"));
        assert_eq!(out_plan.covered.len(), 1);
        assert!(out_plan.covered[0].path.ends_with("proj/target/debug"));
        assert_eq!(out_plan.reclaim, out_plan.targets[0].bytes);

        // 符号链接拒绝
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root.join("logs"), &root.join("logs-link")).unwrap();
            let out_plan = plan(&root, &home, &[(root.join("logs-link"), 0, true)]);
            assert_eq!(out_plan.blocked.len(), 1);
            assert_eq!(out_plan.blocked[0].reason, "符号链接");
        }
    }

    #[test]
    fn permanent_delete_removes_only_targets() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let out_plan = plan(
            &root,
            &root.join("home"),
            &requested(&root, &["proj/target", "logs/app.log"]),
        );
        assert_eq!(out_plan.targets.len(), 2);

        let outcome = execute(
            &out_plan,
            RemovalMode::Permanent,
            &root.join("home"),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_, _, _, _| {},
        );
        assert_eq!(outcome.removed, 2, "failures={:?}", outcome.failures);
        assert_eq!(outcome.failed, 0);
        // free_after/free_before 来自 statvfs 实测：测量间隙其他进程可能写盘，
        // 不保证 free_after >= free_before（该差值仅作展示，不做断言）

        assert!(!root.join("proj/target").exists());
        assert!(!root.join("logs/app.log").exists());
        // 未标记的邻居全部保留
        assert!(root.join("proj/Cargo.toml").exists());
        assert!(root.join("neighbor/value.txt").exists());
        assert!(root.join("logs").exists());
    }

    #[test]
    fn permanent_delete_refuses_swapped_symlink() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let target = root.join("neighbor");
        let out_plan = plan(&root, &root.join("home"), &[(target.clone(), 10, true)]);
        assert_eq!(out_plan.targets.len(), 1);

        // 扫描后目标被替换成符号链接 → 删前复查拒绝
        fs::remove_dir_all(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root.join("proj"), &target).unwrap();

        let outcome = execute(
            &out_plan,
            RemovalMode::Permanent,
            &root.join("home"),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_, _, _, _| {},
        );
        assert_eq!(outcome.failed, 1);
        assert!(outcome.failures[0].error.contains("符号链接"));
        assert!(root.join("proj/Cargo.toml").exists(), "链接目标不应被触碰");
    }

    #[test]
    fn cancellation_after_preflight_does_not_enter_trash() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let plan = plan(
            &root,
            &root.join("home"),
            &requested(&root, &["logs/app.log"]),
        );

        let cancel = AtomicBool::new(false);
        let outcome = execute(
            &plan,
            RemovalMode::Trash,
            &root.join("home"),
            &cancel,
            &mut |_| cancel.store(true, Ordering::Relaxed),
            &mut |_, _, _, _| {},
        );

        assert_eq!(outcome.removed, 0);
        assert!(
            tmp.path().join("logs/app.log").exists(),
            "用户取消后不应进入回收站"
        );
    }

    #[test]
    fn empty_plan_is_noop() {
        let tmp = fixture();
        let root = tmp.path().canonicalize().unwrap();
        let out_plan = plan(&root, &root.join("home"), &[]);
        let outcome = execute(
            &out_plan,
            RemovalMode::Permanent,
            &root.join("home"),
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |_, _, _, _| {},
        );
        assert_eq!(outcome.removed, 0);
        assert!(root.join("proj/target").exists());
    }

    #[test]
    fn mount_points_can_be_decoded_from_proc() {
        // Linux 挂载表解码：\040 是空格
        #[cfg(target_os = "linux")]
        assert_eq!(
            unescape_mounts_field("/mnt/with\\040space"),
            "/mnt/with space"
        );
    }
}
