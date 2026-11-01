//! 目录树扫描：为分析窗口的 treemap 提供完整层级。
//!
//! 设计要点（对齐文档 6.5 预算）：
//! - 只保留**目录节点**，文件聚合进所在目录的 `own_bytes`（treemap 中呈现为
//!   "文件" tile），节点数压到目录数量级；单文件级明细由面板候选扫描承担；
//! - 总量自底向上聚合（totals derived, never tracked），硬链接按 `(dev, ino)`
//!   全局去重；默认不跨挂载点、不跟随符号链接；
//! - 节点预算耗尽或达到深度上限时，剩余子树**继续计量但不保留结构**，
//!   字节折入父目录并在 `pruned` 标注，总量保持准确；
//! - 目录读取失败标 `read_error`，不静默当 0。

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;

use crate::{
    DiskError,
    classify::{self, Category, Reclaim, Risk},
    pool::JobPool,
    size::allocated_size,
};

/// 树节点：字节/文件数/目录数均为子树聚合值，`own_bytes` 是直接子文件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    /// 子树分配大小
    pub bytes: u64,
    /// 直接子文件（不含子目录）
    pub own_bytes: u64,
    /// 子树文件数
    pub files: u64,
    /// 子树目录数（不含自身）
    pub dirs: u32,
    /// 子树最新修改时间（Unix 秒，0 = 未知）
    pub modified: i64,
    pub category: Category,
    pub reclaim: Option<Reclaim>,
    pub risk: Risk,
    /// 读取失败（权限等），子树计量不完整
    pub read_error: bool,
    /// 预算/深度剪枝：字节折入 own_bytes，子树结构未保留
    pub pruned: bool,
    pub children: Vec<TreeNode>,
}

/// 树扫描预算。
#[derive(Debug, Clone, Copy)]
pub struct TreeLimits {
    /// 最大递归深度
    pub max_depth: usize,
    /// 最多保留的目录节点数（超出部分计量但不保留结构）
    pub max_nodes: usize,
    /// 扫描线程数；0 = 自动（min(8, 可用并行度)）
    pub threads: usize,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_nodes: 1_500_000,
            threads: 0,
        }
    }
}

/// 树扫描统计。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeStats {
    pub dirs: u64,
    pub files: u64,
    pub errors: u64,
    pub total_bytes: u64,
    pub truncated: bool,
    /// 扫描是否完整：未截断且没有读取/元数据错误。
    pub complete: bool,
    pub duration_ms: u64,
}

/// 完整扫描树（一次分析窗口会话的缓存单元）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanTree {
    /// 规范后的真实根路径
    pub root: PathBuf,
    pub display_root: PathBuf,
    pub tree: TreeNode,
    pub stats: TreeStats,
}

/// 扫描进度快照。
#[derive(Debug, Clone)]
pub struct TreeProgress {
    pub dirs: u64,
    pub files: u64,
    pub errors: u64,
    pub bytes_seen: u64,
    pub current: PathBuf,
}

/// 目录的并行任务状态：节点（部分聚合）+ 完成计数。
struct DirState {
    /// 部分聚合的节点；自身条目阶段由所属 worker 独占修改，
    /// 子目录完成时按锁合并，无竞争
    node: Mutex<TreeNode>,
    /// 未完成计数：自身条目阶段(1) + 已派发的子目录数
    active: AtomicUsize,
    parent: Option<Arc<DirState>>,
}

/// 一个目录的处理任务。
struct TreeJob {
    state: Arc<DirState>,
    path: PathBuf,
    depth: usize,
}

/// worker 间共享状态。
struct TreeShared<'a> {
    root_dev: u64,
    home: Option<&'a Path>,
    limits: TreeLimits,
    nodes: AtomicUsize,
    seen_hardlinks: Mutex<HashSet<(u64, u64)>>,
    dirs: AtomicU64,
    files: AtomicU64,
    errors: AtomicU64,
    bytes_seen: AtomicU64,
    truncated: AtomicBool,
    cancel: &'a AtomicBool,
    final_root: Mutex<Option<TreeNode>>,
    last_progress: Mutex<Instant>,
    progress: Mutex<&'a mut (dyn FnMut(TreeProgress) + Send + 'a)>,
}

impl TreeShared<'_> {
    fn emit_progress(&self, current: &Path) {
        {
            let mut last = self.last_progress.lock().unwrap();
            if last.elapsed() < Duration::from_millis(150) {
                return;
            }
            *last = Instant::now();
        }
        (self.progress.lock().unwrap())(TreeProgress {
            dirs: self.dirs.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            bytes_seen: self.bytes_seen.load(Ordering::Relaxed),
            current: current.to_path_buf(),
        });
    }

    fn rel_home<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.home.and_then(|home| path.strip_prefix(home).ok())
    }

    /// 硬链接去重：`nlink > 1` 的文件按 `(device, inode)` 只计一次字节。
    fn hardlink_adjusted(&self, seen: &mut HashSet<(u64, u64)>, meta: &fs::Metadata) -> u64 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() > 1 {
                let key = (meta.dev(), meta.ino());
                if !seen.insert(key) {
                    return 0;
                }
            }
            allocated_size(meta)
        }
        #[cfg(not(unix))]
        {
            let _ = seen;
            allocated_size(meta)
        }
    }
}

impl TreeNode {
    fn empty() -> Self {
        Self {
            name: String::new(),
            bytes: 0,
            own_bytes: 0,
            files: 0,
            dirs: 0,
            modified: 0,
            category: Category::Other,
            reclaim: None,
            risk: Risk::Risky,
            read_error: false,
            pruned: false,
            children: Vec::new(),
        }
    }
}

/// 扫描完整目录树（并行）。
pub fn scan_tree(
    root: &Path,
    display_root: &Path,
    home: Option<&Path>,
    limits: TreeLimits,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(TreeProgress) + Send),
) -> Result<ScanTree, DiskError> {
    let started = Instant::now();
    let root_meta = fs::metadata(root)?;
    let root_name = root.file_name().map_or_else(
        || root.to_string_lossy().to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let shared = TreeShared {
        root_dev: device_of(&root_meta),
        home,
        limits,
        nodes: AtomicUsize::new(1),
        seen_hardlinks: Mutex::new(HashSet::new()),
        dirs: AtomicU64::new(1),
        files: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        bytes_seen: AtomicU64::new(0),
        truncated: AtomicBool::new(false),
        cancel,
        final_root: Mutex::new(None),
        last_progress: Mutex::new(Instant::now() - Duration::from_millis(500)),
        progress: Mutex::new(progress),
    };

    let root_state = Arc::new(DirState {
        node: Mutex::new(TreeNode {
            name: root_name.clone(),
            ..TreeNode::empty()
        }),
        active: AtomicUsize::new(1),
        parent: None,
    });

    let pool = Arc::new(JobPool::<TreeJob>::new());
    pool.push(TreeJob {
        state: root_state.clone(),
        path: root.to_path_buf(),
        depth: 0,
    });

    let shared_ref = &shared;
    let pool_ref = &pool;
    std::thread::scope(|scope| {
        let workers = crate::pool::scan_thread_count(limits.threads);
        for _ in 0..workers.saturating_sub(1) {
            scope.spawn(move || worker_loop(shared_ref, pool_ref));
        }
        worker_loop(shared_ref, pool_ref);
    });

    if shared.cancel.load(Ordering::Relaxed) {
        return Err(DiskError::Cancelled);
    }

    let root_node = shared
        .final_root
        .lock()
        .unwrap()
        .take()
        .expect("根节点必然完成");
    let stats = TreeStats {
        dirs: shared.dirs.load(Ordering::Relaxed),
        files: shared.files.load(Ordering::Relaxed),
        errors: shared.errors.load(Ordering::Relaxed),
        total_bytes: root_node.bytes,
        truncated: shared.truncated.load(Ordering::Relaxed),
        complete: !shared.truncated.load(Ordering::Relaxed)
            && shared.errors.load(Ordering::Relaxed) == 0,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    progress(TreeProgress {
        dirs: stats.dirs,
        files: stats.files,
        errors: stats.errors,
        bytes_seen: stats.total_bytes,
        current: root.to_path_buf(),
    });

    Ok(ScanTree {
        root: root.to_path_buf(),
        display_root: display_root.to_path_buf(),
        tree: root_node,
        stats,
    })
}

/// worker 主循环。
fn worker_loop(shared: &TreeShared, pool: &JobPool<TreeJob>) {
    while let Some(job) = pool.pop(shared.cancel) {
        process_dir(job, shared, pool);
        pool.done();
    }
}

/// 处理一个目录任务：条目聚合 + 子任务入池；自身条目完成后尝试完结。
fn process_dir(job: TreeJob, shared: &TreeShared, pool: &JobPool<TreeJob>) {
    let TreeJob {
        state, path, depth, ..
    } = job;
    if shared.cancel.load(Ordering::Relaxed) {
        // 完成计数仍要归还，父目录的级联照常进行（取消时树会被丢弃，仅为计数一致）
        return;
    }
    let (entries, read_failed) = read_entries(&path, shared);
    if read_failed {
        state.node.lock().unwrap().read_error = true;
    }

    let names: HashSet<String> = entries
        .iter()
        .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
        .collect();
    // 自身直接文件的聚合（本 worker 独占，结束后并入节点）
    let mut own_bytes = 0u64;
    let mut own_files = 0u64;
    let mut own_modified = 0i64;

    for entry in entries {
        if shared.cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            shared.errors.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let entry_path = entry.path();
        // 路径String是删除/定位的标识；非 UTF-8 名称无法无损表示，跳过并标记不完整，
        // 避免 lossy 转换后与另一个名字碰撞。
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            shared.errors.fetch_add(1, Ordering::Relaxed);
            state.node.lock().unwrap().read_error = true;
            continue;
        };

        if file_type.is_dir() {
            let Ok(meta) = entry.metadata() else {
                shared.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            if device_of(&meta) != shared.root_dev {
                // 跨挂载点子树不属于本卷
                continue;
            }
            shared.dirs.fetch_add(1, Ordering::Relaxed);
            shared.emit_progress(&entry_path);
            let rel_home = shared.rel_home(&entry_path).map(Path::to_path_buf);
            // 分类：规则优先，未命中继承父类。规则 emit=false 表示只计量、
            // 不保留结构（云盘/应用数据/媒体库），绝不能把云盘子树折叠成
            // 可回收节点——否则分析窗口的“安全标记”会直接选中它们。
            let rule =
                classify::match_dir(&name, rel_home.as_deref(), &|want| names.contains(want));
            if rule.is_some_and(|rule| !rule.emit) {
                let mut seen = shared.seen_hardlinks.lock().unwrap();
                let measured = measure_only(&entry_path, shared.root_dev, &mut seen, shared.cancel)
                    .unwrap_or_default();
                drop(seen);
                own_bytes = own_bytes.saturating_add(measured.bytes);
                own_files = own_files.saturating_add(measured.files);
                own_modified = own_modified.max(measured.modified);
                shared.files.fetch_add(measured.files, Ordering::Relaxed);
                shared.dirs.fetch_add(measured.dirs, Ordering::Relaxed);
                shared.errors.fetch_add(measured.errors, Ordering::Relaxed);
                shared
                    .bytes_seen
                    .fetch_add(measured.bytes, Ordering::Relaxed);
                state.node.lock().unwrap().pruned = true;
                continue;
            }
            // 预算/深度剪枝：继续计量但不再保留结构，字节折入本目录
            if shared.nodes.load(Ordering::Relaxed) >= shared.limits.max_nodes
                || depth + 1 >= shared.limits.max_depth
            {
                shared.truncated.store(true, Ordering::Relaxed);
                let mut seen = shared.seen_hardlinks.lock().unwrap();
                // 取消时 measure_only 返回 Err——这里按 0 处理即可，
                // 外层循环马上会因 cancel 退出，树最终被丢弃
                let measured = measure_only(&entry_path, shared.root_dev, &mut seen, shared.cancel)
                    .unwrap_or_default();
                drop(seen);
                own_bytes = own_bytes.saturating_add(measured.bytes);
                own_files = own_files.saturating_add(measured.files);
                own_modified = own_modified.max(measured.modified);
                shared.files.fetch_add(measured.files, Ordering::Relaxed);
                shared.dirs.fetch_add(measured.dirs, Ordering::Relaxed);
                shared.errors.fetch_add(measured.errors, Ordering::Relaxed);
                shared
                    .bytes_seen
                    .fetch_add(measured.bytes, Ordering::Relaxed);
                state.node.lock().unwrap().pruned = true;
                continue;
            }
            let (category, reclaim, risk) = match rule {
                Some(rule) => (rule.category, rule.reclaim, rule.risk),
                None => {
                    let node = state.node.lock().unwrap();
                    (node.category, node.reclaim, node.risk)
                }
            };
            let child_state = Arc::new(DirState {
                node: Mutex::new(TreeNode {
                    name,
                    bytes: 0,
                    own_bytes: 0,
                    files: 0,
                    dirs: 0,
                    modified: 0,
                    category,
                    reclaim,
                    risk,
                    // read_error 由子任务自身的 worker 读取时置位
                    read_error: false,
                    pruned: false,
                    children: Vec::new(),
                }),
                active: AtomicUsize::new(1),
                parent: Some(state.clone()),
            });
            // 先计数再入队：保证子任务完成前父的 active 不会提前归零
            state.active.fetch_add(1, Ordering::Relaxed);
            shared.nodes.fetch_add(1, Ordering::Relaxed);
            pool.push(TreeJob {
                state: child_state,
                path: entry_path,
                depth: depth + 1,
            });
        } else if file_type.is_file() {
            shared.files.fetch_add(1, Ordering::Relaxed);
            own_files += 1;
            let Ok(meta) = entry.metadata() else {
                shared.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            let bytes = shared.hardlink_adjusted(&mut shared.seen_hardlinks.lock().unwrap(), &meta);
            own_bytes = own_bytes.saturating_add(bytes);
            own_modified = own_modified.max(modified_secs(&meta));
            shared.bytes_seen.fetch_add(bytes, Ordering::Relaxed);
            shared.emit_progress(&entry_path);
        }
    }

    // 自身条目阶段完成：own 并入聚合值
    {
        let mut node = state.node.lock().unwrap();
        node.own_bytes = node.own_bytes.saturating_add(own_bytes);
        node.files = node.files.saturating_add(own_files);
        node.modified = node.modified.max(own_modified);
    }
    if state.active.fetch_sub(1, Ordering::Relaxed) == 1 {
        complete_dir(state, shared);
    }
}

/// 目录完成：own 并入聚合值， attaching 到父目录并级联
/// （最后完成者负责聚合父目录，worker 永不阻塞等待）。
fn complete_dir(state: Arc<DirState>, shared: &TreeShared) {
    let mut current = Some(state);
    while let Some(st) = current {
        let node = {
            let mut guard = st.node.lock().unwrap();
            guard.bytes = guard.bytes.saturating_add(guard.own_bytes);
            std::mem::replace(&mut *guard, TreeNode::empty())
        };
        match &st.parent {
            Some(parent) => {
                {
                    let mut pguard = parent.node.lock().unwrap();
                    pguard.bytes = pguard.bytes.saturating_add(node.bytes);
                    pguard.files = pguard.files.saturating_add(node.files);
                    pguard.dirs = pguard.dirs.saturating_add(node.dirs + 1);
                    pguard.modified = pguard.modified.max(node.modified);
                    pguard.children.push(node);
                }
                if parent.active.fetch_sub(1, Ordering::Relaxed) == 1 {
                    current = Some(parent.clone());
                } else {
                    break;
                }
            }
            None => {
                *shared.final_root.lock().unwrap() = Some(node);
                break;
            }
        }
    }
}

/// 读取目录条目；第二个返回值表示读取失败（节点应标注 read_error）。
fn read_entries(dir: &Path, shared: &TreeShared) -> (Vec<std::fs::DirEntry>, bool) {
    match fs::read_dir(dir) {
        Ok(entries) => {
            let mut items = Vec::new();
            let mut failed = false;
            for entry in entries {
                match entry {
                    Ok(entry) => items.push(entry),
                    Err(_) => {
                        shared.errors.fetch_add(1, Ordering::Relaxed);
                        failed = true;
                    }
                }
            }
            (items, failed)
        }
        Err(_) => {
            shared.errors.fetch_add(1, Ordering::Relaxed);
            (Vec::new(), true)
        }
    }
}

/// 目录的危险度：删除该子树会一并丢失的内容。
///
/// - `git_repos`：子树内 `.git` 目录数（git 仓库，可能含未提交的工作与历史）；
/// - `non_reclaimable_bytes`：子树内**不属于任何可回收目录**的字节
///   （源码、文档等用户数据——可回收目录的整体字节视为可再生）。
pub fn danger_stats(node: &TreeNode) -> (u64, u64) {
    fn count_git(node: &TreeNode) -> u64 {
        node.children
            .iter()
            .map(|child| {
                if child.name == ".git" {
                    1
                } else {
                    count_git(child)
                }
            })
            .sum()
    }
    fn walk(node: &TreeNode) -> (u64, u64) {
        // 只有 Safe 可回收目录才把整棵子树视为可再生；Caution/Risky 即使
        // 带 reclaim 原因也必须计入非可回收字节，否则“谨慎项”会被整体放行。
        // 即便规则是 Safe，子树里出现 .git 仍然要拦下——规则只说明目录名
        // 可再生，不能保证用户没往里放仓库。
        //
        // 读取失败或预算/深度剪枝时结构里看不到的内容不能当作“可再生”：
        // 被折叠进 own_bytes 的未知内容必须按非可回收处理，否则一个 Safe
        // 缓存目录可能把隐藏的源码/仓库一起整目录删掉。
        if node.reclaim.is_some() && node.risk == Risk::Safe {
            let hidden = if node.pruned || node.read_error {
                node.own_bytes.max(1)
            } else {
                0
            };
            return (count_git(node), hidden);
        }
        let mut git = 0u64;
        let mut non_reclaim = node.own_bytes;
        if node.read_error && non_reclaim == 0 {
            non_reclaim = 1;
        }
        for child in &node.children {
            if child.name == ".git" {
                git += 1;
                continue;
            }
            let (child_git, child_non_reclaim) = walk(child);
            git += child_git;
            non_reclaim += child_non_reclaim;
        }
        (git, non_reclaim)
    }
    walk(node)
}

/// 按路径在树中定位节点（组件级匹配）；target 为扫描根本身时返回根节点。
pub fn locate<'a>(tree: &'a TreeNode, root: &Path, target: &Path) -> Option<&'a TreeNode> {
    let rel = target.strip_prefix(root).ok()?;
    let mut node = tree;
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy();
        node = node.children.iter().find(|child| child.name == name)?;
    }
    Some(node)
}

/// 计量子树（不保留结构）：预算耗尽或 emit=false 时保持总量准确的兜底遍历。
#[derive(Debug, Default)]
struct Measurement {
    bytes: u64,
    files: u64,
    dirs: u64,
    modified: i64,
    errors: u64,
}

fn measure_only(
    path: &Path,
    root_dev: u64,
    seen: &mut HashSet<(u64, u64)>,
    cancel: &AtomicBool,
) -> Result<Measurement, DiskError> {
    let mut measured = Measurement::default();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(DiskError::Cancelled);
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                measured.errors += 1;
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return Err(DiskError::Cancelled);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    measured.errors += 1;
                    continue;
                }
            };
            let Ok(file_type) = entry.file_type() else {
                measured.errors += 1;
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let Ok(meta) = entry.metadata() else {
                    measured.errors += 1;
                    continue;
                };
                if device_of(&meta) != root_dev {
                    continue;
                }
                measured.dirs += 1;
                stack.push(entry.path());
            } else if file_type.is_file() {
                let Ok(meta) = entry.metadata() else {
                    measured.errors += 1;
                    continue;
                };
                measured.files += 1;
                measured.bytes = measured
                    .bytes
                    .saturating_add(hardlink_adjusted(seen, &meta));
                measured.modified = measured.modified.max(modified_secs(&meta));
            }
        }
    }
    Ok(measured)
}

/// 硬链接去重：`nlink > 1` 的文件按 `(device, inode)` 只计一次字节。
fn hardlink_adjusted(seen: &mut HashSet<(u64, u64)>, meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            let key = (meta.dev(), meta.ino());
            if !seen.insert(key) {
                return 0;
            }
        }
        allocated_size(meta)
    }
    #[cfg(not(unix))]
    {
        let _ = seen;
        allocated_size(meta)
    }
}

fn modified_secs(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(unix)]
fn device_of(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn device_of(_meta: &fs::Metadata) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn no_progress() -> impl FnMut(TreeProgress) {
        |_| {}
    }

    fn scan(root: &Path) -> ScanTree {
        scan_tree(
            &root,
            &root,
            None,
            TreeLimits::default(),
            &AtomicBool::new(false),
            &mut no_progress(),
        )
        .expect("树扫描应完成")
    }

    #[test]
    fn tree_aggregates_bottom_up() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("proj/target/debug")).unwrap();
        fs::write(root.join("proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("proj/target/debug/app"), vec![0u8; 4096]).unwrap();
        fs::write(root.join("top.bin"), vec![0u8; 1024]).unwrap();

        let tree = scan(&root);
        assert_eq!(tree.tree.files, 3);
        assert_eq!(tree.tree.dirs, 3, "proj + target + debug");
        assert!(
            tree.tree.bytes >= 3 * 1024,
            "总量=分配大小聚合: {}",
            tree.tree.bytes
        );
        // proj 未命中规则 → 继承根 Other；target 命中 Code/BuildOutput
        let proj = tree
            .tree
            .children
            .iter()
            .find(|n| n.name == "proj")
            .unwrap();
        assert_eq!(proj.category, Category::Other);
        let target = proj.children.iter().find(|n| n.name == "target").unwrap();
        assert_eq!(target.category, Category::Code);
        assert_eq!(target.reclaim, Some(Reclaim::BuildOutput));
    }

    #[test]
    fn classify_rules_apply_and_inherit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("web/node_modules/dep")).unwrap();
        fs::write(root.join("web/package.json"), "{}").unwrap();
        fs::write(root.join("web/node_modules/dep/index.js"), "x").unwrap();
        fs::create_dir_all(root.join("plain/sub")).unwrap();

        let tree = scan(&root);
        let web = tree.tree.children.iter().find(|n| n.name == "web").unwrap();
        // web 未命中规则 → 继承根 Other；node_modules 命中 Code/Reinstallable
        assert_eq!(web.category, Category::Other);
        let modules = web
            .children
            .iter()
            .find(|n| n.name == "node_modules")
            .unwrap();
        assert_eq!(modules.category, Category::Code);
        assert_eq!(modules.reclaim, Some(Reclaim::Reinstallable));
        let plain = tree
            .tree
            .children
            .iter()
            .find(|n| n.name == "plain")
            .unwrap();
        assert_eq!(plain.category, Category::Other);
    }

    #[test]
    fn budget_prune_keeps_totals() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for index in 0..5 {
            let dir = root.join(format!("d{index}/inner/deep"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("f.bin"), vec![0u8; 1024]).unwrap();
        }
        let tree = scan_tree(
            &root,
            &root,
            None,
            TreeLimits {
                max_depth: 64,
                max_nodes: 3,
                threads: 2,
            },
            &AtomicBool::new(false),
            &mut no_progress(),
        )
        .unwrap();
        assert!(tree.stats.truncated);
        // 总量仍然准确：被剪枝子树折入父级
        assert_eq!(tree.tree.files, 5);
        assert!(tree.tree.bytes >= 5 * 1024);
        fn any_pruned(node: &TreeNode) -> bool {
            node.pruned || node.children.iter().any(any_pruned)
        }
        assert!(any_pruned(&tree.tree), "应有 pruned 标注的节点");
    }

    #[test]
    fn danger_stats_counts_git_and_non_reclaimable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // workspace/proj：源码 1MB + target(可再生 4MB) + .git 仓库
        fs::create_dir_all(root.join("workspace/proj/target/debug")).unwrap();
        fs::create_dir_all(root.join("workspace/proj/.git")).unwrap();
        fs::create_dir_all(root.join("workspace/proj/src")).unwrap();
        fs::write(root.join("workspace/proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("workspace/proj/src/main.rs"), "fn main() {}").unwrap();
        fs::write(
            root.join("workspace/proj/target/debug/app"),
            vec![0u8; 4096],
        )
        .unwrap();
        // 纯缓存目录（可再生）
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join(".cache/f.bin"), vec![0u8; 1024]).unwrap();

        let tree = scan(&root);
        let workspace = locate(&tree.tree, &root, &root.join("workspace")).unwrap();
        let (git, non_reclaim) = danger_stats(workspace);
        assert_eq!(git, 1, "应发现 1 个 git 仓库");
        // 非可回收 = 源码等（target 整体可再生）；1MB 级别（块对齐后）
        assert!(
            non_reclaim >= 1024 && non_reclaim < 4 * 1024 * 1024,
            "非可回收字节应只含源码: {non_reclaim}"
        );
        // 纯缓存目录：非可回收为 0
        let caches = locate(&tree.tree, &root, &root.join(".cache")).unwrap();
        assert_eq!(danger_stats(caches), (0, 0));
    }

    #[test]
    fn danger_stats_still_flags_git_inside_a_safe_reclaim_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("proj/target/.git")).unwrap();
        fs::write(root.join("proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("proj/target/app"), vec![0u8; 4096]).unwrap();

        let tree = scan(&root);
        let target = locate(&tree.tree, &root, &root.join("proj/target")).unwrap();
        assert_eq!(target.reclaim, Some(Reclaim::BuildOutput));
        assert_eq!(target.risk, Risk::Safe);
        let (git, non_reclaim) = danger_stats(target);
        assert_eq!(git, 1, "Safe 规则目录内的 .git 仍需拦截");
        assert_eq!(non_reclaim, 0, "Safe 目录本身的字节仍视为可再生");
    }

    #[test]
    fn emit_false_cloud_dirs_are_measured_but_not_reclaimable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("Library/Mobile Documents/CloudDocs")).unwrap();
        fs::write(
            root.join("Library/Mobile Documents/CloudDocs/secret.bin"),
            vec![0u8; 4096],
        )
        .unwrap();

        let tree = scan_tree(
            &root,
            &root,
            Some(&root),
            TreeLimits::default(),
            &AtomicBool::new(false),
            &mut no_progress(),
        )
        .unwrap();
        assert!(tree.tree.bytes >= 4096, "云盘字节仍应计入父总量");
        let library = tree
            .tree
            .children
            .iter()
            .find(|n| n.name == "Library")
            .unwrap();
        assert!(
            !library
                .children
                .iter()
                .any(|n| n.name == "Mobile Documents"),
            "emit=false 的云盘目录不应生成节点"
        );
        let insights = crate::insights::insights_from_tree(&tree.tree, &tree.root, 0, 8);
        assert!(
            insights
                .iter()
                .all(|item| !item.path.contains("Mobile Documents")),
            "云盘不得进入可回收洞察"
        );
    }

    #[test]
    fn danger_stats_does_not_trust_caution_reclaim_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/app.log"), vec![0u8; 4096]).unwrap();
        let tree = scan(&root);
        let logs = locate(&tree.tree, &root, &root.join("logs")).unwrap();
        assert_eq!(logs.reclaim, Some(Reclaim::Temporary));
        assert_eq!(logs.risk, Risk::Caution);
        let (git, non_reclaim) = danger_stats(logs);
        assert_eq!(git, 0);
        assert!(non_reclaim > 0, "Caution 项不能整目录放行");
    }

    #[test]
    fn locate_finds_nested_node() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        let tree = scan(&root);
        assert!(locate(&tree.tree, &root, &root.join("a/b/c")).is_some());
        assert!(locate(&tree.tree, &root, &root).is_some(), "根本身可定位");
        assert!(locate(&tree.tree, &root, &root.join("a/nope")).is_none());
    }

    #[test]
    fn modified_propagates_to_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("deep")).unwrap();
        let path = root.join("deep/old.log");
        fs::write(&path, "x").unwrap();
        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(90 * 86_400))
            .unwrap();
        drop(file);
        let tree = scan(&root);
        assert!(tree.tree.modified > 0);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(tree.tree.modified < now, "根的修改时间应来自 90 天前的文件");
    }
}
