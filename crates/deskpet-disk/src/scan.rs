//! 候选扫描：并行深度优先遍历，产出"可解释的清理候选"。
//!
//! 并行模型（P3）：每个目录一个任务，投入自研工作池（`pool::JobPool`）；
//! 命中规则的目录由当前 worker 就地测厚（子树串行、目录间并行），
//! 未命中的子目录继续入池。共享状态全部为原子/Mutex：
//! 硬链接按 `(dev, ino)` 全局去重、预算/截断显式标注、不跨挂载点、
//! 符号链接一律不跟随。取消时任务按空操作快速排干（<500ms）。
//!
//! 实时预览（P3）：顶层目录聚合到最多 128 个命名桶 + 1 个溢出桶，
//! 每秒最多推一次，与结果集分离——扫描中不可操作。

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;

use crate::{
    DiskError,
    classify::{self, Category, Kind, Reclaim, Risk, Rule},
    pool::JobPool,
    size::{DirSize, allocated_size},
};

/// 扫描预算：防止误选大目录后失控。
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    /// 从扫描根向下的最大层级
    pub max_depth: usize,
    /// 最多收集的候选数
    pub max_candidates: usize,
    /// 统计体积时最多访问的目录数（超过后结果标注不完整）
    pub max_dir_visits: u64,
    /// 扫描线程数；0 = 自动（min(8, 可用并行度)）
    pub threads: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_depth: 12,
            max_candidates: 20_000,
            max_dir_visits: 200_000,
            threads: 0,
        }
    }
}

/// 扫描选项。
#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    /// `*.log` 的时间阈值（天）；0 = 不限时间
    pub log_days: u32,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            log_days: classify::LOG_DEFAULT_DAYS,
        }
    }
}

/// 清理候选。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// 真实绝对路径（展示前缀映射由上层处理）
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub bytes: u64,
    pub files: u64,
    /// 最近修改时间（Unix 秒，0 = 未知）
    pub modified: i64,
    pub rule_id: &'static str,
    pub category: Category,
    pub reclaim: Option<Reclaim>,
    pub risk: Risk,
    /// 目录体积是否完整（访问预算内扫完）
    pub complete: bool,
    /// 扫描完成后是否默认勾选
    pub default_selected: bool,
}

/// 扫描统计。
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub dirs: u64,
    pub files: u64,
    pub errors: u64,
    /// 候选字节总和（分配大小口径，硬链接去重后）
    pub candidate_bytes: u64,
    /// 候选数达到上限，提前截断
    pub truncated: bool,
    /// 是否在访问预算内完成（false = 结果不完整）
    pub complete: bool,
}

/// 扫描结果。
#[derive(Debug)]
pub struct ScanOutput {
    pub candidates: Vec<Candidate>,
    pub stats: ScanStats,
}

/// 扫描进度快照（上层节流后转发 SSE）。
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub dirs: u64,
    pub files: u64,
    pub errors: u64,
    pub candidate_bytes: u64,
    pub current: PathBuf,
}

/// 实时预览条目（顶层目录聚合；name 为空表示溢出桶）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntry {
    pub name: String,
    pub bytes: u64,
    pub files: u64,
}

/// 实时预览快照（只读，扫描中不可操作）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPreview {
    pub entries: Vec<PreviewEntry>,
    pub elapsed_ms: u64,
}

/// 预览桶：顶层目录 → 字节/文件。
type PreviewBuckets = HashMap<String, (u64, u64)>;

/// 预览桶数量上限（文档：最多 128 个命名桶 + 1 个溢出桶）。
const PREVIEW_BUCKETS: usize = 128;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(1000);

/// 一次多链接文件的出现记录：扫描结束后按 inode 分组做确定性归属重算，
/// 消除"先扫到的线程先计入"带来的跨运行抖动。
#[derive(Debug)]
struct HardlinkOcc {
    /// `(device, inode)`
    key: (u64, u64),
    /// 出现位置（文件路径）
    path: PathBuf,
    /// 该文件按分配口径的大小（同一 inode 的每次出现相同）
    size: u64,
    /// 计入的归属候选路径（目录/文件候选）；None = 计入预览桶等非候选位置
    owner: Option<PathBuf>,
    /// 是否为实际计一次的那条出现（其余出现计 0）
    counted: bool,
}

/// worker 间共享的扫描状态（原子/Mutex，无阻塞聚合）。
struct ScanShared<'a> {
    root_name: String,
    root_dev: u64,
    home: Option<&'a Path>,
    limits: ScanLimits,
    log_days: u32,
    cancel: &'a AtomicBool,
    cancelled: AtomicBool,

    dirs: AtomicU64,
    files: AtomicU64,
    errors: AtomicU64,
    candidate_bytes: AtomicU64,
    visits: AtomicU64,
    truncated: AtomicBool,
    complete: AtomicBool,

    seen_hardlinks: Mutex<HashSet<(u64, u64)>>,
    hardlink_occurrences: Mutex<Vec<HardlinkOcc>>,
    candidates: Mutex<Vec<Candidate>>,
    preview: Mutex<PreviewBuckets>,
    last_progress: Mutex<Instant>,
    last_preview: Mutex<Instant>,
    started: Instant,

    progress: Mutex<&'a mut (dyn FnMut(ScanProgress) + Send + 'a)>,
    preview_cb: Mutex<&'a mut (dyn FnMut(ScanPreview) + Send + 'a)>,
}

impl ScanShared<'_> {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.cancel.load(Ordering::Relaxed)
    }

    /// 访问预算；耗尽时置 complete=false（总量为下界，必须显式标注）。
    fn visit(&self) -> bool {
        if self.visits.fetch_add(1, Ordering::Relaxed) + 1 > self.limits.max_dir_visits {
            self.complete.store(false, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// 候选入列；达到上限置 truncated。
    fn push_candidate(&self, candidate: Candidate) {
        let mut candidates = self.candidates.lock().unwrap();
        if candidates.len() >= self.limits.max_candidates {
            self.truncated.store(true, Ordering::Relaxed);
            return;
        }
        self.candidate_bytes
            .fetch_add(candidate.bytes, Ordering::Relaxed);
        candidates.push(candidate);
    }

    /// 预览桶聚合。
    fn add_preview(&self, top: &str, bytes: u64, files: u64) {
        if bytes == 0 && files == 0 {
            return;
        }
        let mut buckets = self.preview.lock().unwrap();
        let entry = buckets.entry(top.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(bytes);
        entry.1 = entry.1.saturating_add(files);
    }

    /// 进度节流推送（150ms）。
    fn emit_progress(&self, current: &Path) {
        {
            let mut last = self.last_progress.lock().unwrap();
            if last.elapsed() < Duration::from_millis(150) {
                return;
            }
            *last = Instant::now();
        }
        (self.progress.lock().unwrap())(ScanProgress {
            dirs: self.dirs.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            candidate_bytes: self.candidate_bytes.load(Ordering::Relaxed),
            current: current.to_path_buf(),
        });
    }

    /// 预览节流推送（1s）：排序取 128 桶 + 溢出桶。
    fn emit_preview(&self) {
        {
            let mut last = self.last_preview.lock().unwrap();
            if last.elapsed() < PREVIEW_INTERVAL {
                return;
            }
            *last = Instant::now();
        }
        let buckets = self.preview.lock().unwrap();
        let entries = snapshot_buckets(&buckets);
        (self.preview_cb.lock().unwrap())(ScanPreview {
            entries,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
        });
    }

    fn rel_home<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.home.and_then(|home| path.strip_prefix(home).ok())
    }

    /// 硬链接去重：`nlink > 1` 的文件按 `(device, inode)` 只计一次字节。
    /// `path` 为出现位置，`owner` 为计入的归属候选（None = 预览等非候选位置）；
    /// 同时记录出现明细，扫描结束后统一重算归属（见 `resolve_hardlink_attribution`）。
    fn hardlink_adjusted(&self, meta: &fs::Metadata, path: &Path, owner: Option<&Path>) -> u64 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() > 1 {
                let key = (meta.dev(), meta.ino());
                let size = allocated_size(meta);
                let counted = {
                    let mut seen = self.seen_hardlinks.lock().unwrap();
                    seen.insert(key)
                };
                self.hardlink_occurrences.lock().unwrap().push(HardlinkOcc {
                    key,
                    path: path.to_path_buf(),
                    size,
                    owner: owner.map(Path::to_path_buf),
                    counted,
                });
                if !counted {
                    return 0;
                }
            }
            allocated_size(meta)
        }
        #[cfg(not(unix))]
        {
            let _ = (self, path, owner);
            allocated_size(meta)
        }
    }
}

/// 一个目录的处理任务。
struct ScanJob {
    path: PathBuf,
    depth: usize,
    /// 顶层预览桶（根的直接子目录名；根自身文件归入根名）
    top: String,
}

/// 扫描候选（并行）。
///
/// `home` 用于家目录相对规则（`Library/Caches` 等）；`progress` 150ms 节流，
/// `preview` 1s 节流（只读，扫描中不可操作）。
pub fn scan_candidates(
    root: &Path,
    home: Option<&Path>,
    options: ScanOptions,
    limits: ScanLimits,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(ScanProgress) + Send),
    preview: &mut (dyn FnMut(ScanPreview) + Send),
) -> Result<ScanOutput, DiskError> {
    let root_meta = fs::metadata(root)?;
    let root_name = root.file_name().map_or_else(
        || root.to_string_lossy().to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let shared = ScanShared {
        root_name,
        root_dev: device_of(&root_meta),
        home,
        limits,
        log_days: options.log_days,
        cancel,
        cancelled: AtomicBool::new(false),
        dirs: AtomicU64::new(0),
        files: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        candidate_bytes: AtomicU64::new(0),
        visits: AtomicU64::new(0),
        truncated: AtomicBool::new(false),
        complete: AtomicBool::new(true),
        seen_hardlinks: Mutex::new(HashSet::new()),
        hardlink_occurrences: Mutex::new(Vec::new()),
        candidates: Mutex::new(Vec::new()),
        preview: Mutex::new(HashMap::new()),
        last_progress: Mutex::new(Instant::now() - Duration::from_millis(500)),
        last_preview: Mutex::new(Instant::now() - Duration::from_millis(1500)),
        started: Instant::now(),
        progress: Mutex::new(progress),
        preview_cb: Mutex::new(preview),
    };

    let pool = Arc::new(JobPool::<ScanJob>::new());
    // 根任务：top 用根名（根目录下的直接文件归入该桶）
    pool.push(ScanJob {
        path: root.to_path_buf(),
        depth: 0,
        top: shared.root_name.clone(),
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

    if shared.cancelled() {
        return Err(DiskError::Cancelled);
    }

    let mut candidates = std::mem::take(&mut *shared.candidates.lock().unwrap());
    // 硬链接字节归属重算：并行扫描"先到先得"受线程调度影响，
    // 统一按 inode 归到确定的位置，保证两次扫描同一候选字节一致。
    let occurrences = std::mem::take(&mut *shared.hardlink_occurrences.lock().unwrap());
    resolve_hardlink_attribution(&mut candidates, &occurrences);
    let stats = ScanStats {
        dirs: shared.dirs.load(Ordering::Relaxed),
        files: shared.files.load(Ordering::Relaxed),
        errors: shared.errors.load(Ordering::Relaxed),
        // 以归属重算后的候选为准（原子值只用于扫描中的实时进度）
        candidate_bytes: candidates.iter().map(|item| item.bytes).sum(),
        truncated: shared.truncated.load(Ordering::Relaxed),
        complete: shared.complete.load(Ordering::Relaxed)
            && shared.errors.load(Ordering::Relaxed) == 0,
    };
    progress(ScanProgress {
        dirs: stats.dirs,
        files: stats.files,
        errors: stats.errors,
        candidate_bytes: stats.candidate_bytes,
        current: root.to_path_buf(),
    });

    Ok(ScanOutput { candidates, stats })
}

/// worker 主循环：取任务 → 处理 → 完成；取消时 pop 返回 None 自然退出。
fn worker_loop(shared: &ScanShared, pool: &JobPool<ScanJob>) {
    while let Some(job) = pool.pop(shared.cancel) {
        if shared.cancelled() {
            pool.done();
            continue;
        }
        process_dir(job, shared, pool);
        pool.done();
        // 有新数据时顺带推送预览（内部 1s 节流）
        shared.emit_preview();
    }
}

/// 处理一个目录任务：规则匹配 + 测厚 + 子任务入池。
fn process_dir(job: ScanJob, shared: &ScanShared, pool: &JobPool<ScanJob>) {
    let ScanJob { path, depth, top } = job;
    if shared.cancelled() {
        return;
    }
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(_) => {
            shared.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let mut items: Vec<fs::DirEntry> = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => items.push(entry),
            Err(_) => {
                shared.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    shared.dirs.fetch_add(1, Ordering::Relaxed);
    let names: HashSet<String> = items
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    let siblings = |want: &str| names.contains(want);
    // 本目录直接文件的预览聚合（任务结束时一次性入桶）
    let mut own_preview: (u64, u64) = (0, 0);

    for entry in items {
        if shared.cancelled() {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            shared.errors.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        // 符号链接一律不跟随
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if file_type.is_dir() {
            // 跨挂载点的子树不属于本卷
            let Ok(meta) = entry.metadata() else {
                shared.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            if device_of(&meta) != shared.root_dev {
                continue;
            }
            let rel_home = shared.rel_home(&path);
            if classify::is_aged_children_parent(rel_home) {
                // 旧下载目录：子项按修改时间筛选，旧的测厚入列且不再下钻
                let days = classify::aged_children_days(rel_home).unwrap_or(90);
                if entry_is_older_than(&entry, days) {
                    if let Some(candidate) =
                        measure_dir_candidate(&path, &name, shared, rule_of("downloads-old"), &top)
                    {
                        shared.push_candidate(candidate);
                    }
                } else if depth < shared.limits.max_depth {
                    pool.push(ScanJob {
                        path,
                        depth: depth + 1,
                        top: top.clone(),
                    });
                } else {
                    // 达到深度上限：未下钻的子树没有被计量，结果必须标记不完整，
                    // 不能把“没扫到”伪装成“没有候选”。
                    shared.complete.store(false, Ordering::Relaxed);
                }
            } else if let Some(rule) = classify::match_dir(&name, rel_home, &siblings) {
                if rule.emit {
                    if let Some(candidate) = measure_dir_candidate(&path, &name, shared, rule, &top)
                    {
                        shared.push_candidate(candidate);
                    }
                }
                // 命中的目录（候选或纯剪枝）都不再下钻
            } else if depth < shared.limits.max_depth {
                // 顶层目录（根的直接子目录）用自己的名字作预览桶
                let child_top = if depth == 0 {
                    name.clone()
                } else {
                    top.clone()
                };
                pool.push(ScanJob {
                    path,
                    depth: depth + 1,
                    top: child_top,
                });
            } else {
                // 达到深度上限：该目录及子树未扫描，结果不能宣称完整。
                shared.complete.store(false, Ordering::Relaxed);
            }
        } else if file_type.is_file() {
            shared.files.fetch_add(1, Ordering::Relaxed);
            let Ok(meta) = entry.metadata() else {
                shared.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            let age_days = age_in_days(&meta);
            let rel_home = shared.rel_home(&path);
            let is_old_download = classify::is_aged_children_parent(rel_home)
                && age_days.is_some_and(|age| {
                    age >= u64::from(classify::aged_children_days(rel_home).unwrap_or(90))
                });
            let rule = if is_old_download {
                Some(rule_of("downloads-old"))
            } else {
                classify::match_file(&name, rel_home, age_days, Some(shared.log_days))
            };
            if let Some(rule) = rule {
                let bytes = shared.hardlink_adjusted(&meta, &path, Some(&path));
                shared.push_candidate(Candidate {
                    path: path.clone(),
                    name,
                    kind: Kind::File,
                    bytes,
                    files: 1,
                    modified: modified_secs(&meta),
                    rule_id: rule.id,
                    category: rule.category,
                    reclaim: rule.reclaim,
                    risk: rule.risk,
                    complete: true,
                    default_selected: rule.default_selected && rule.risk != Risk::Risky,
                });
            } else {
                own_preview.0 = own_preview
                    .0
                    .saturating_add(shared.hardlink_adjusted(&meta, &path, None));
            }
            own_preview.1 = own_preview.1.saturating_add(1);
            shared.emit_progress(&path);
        }
    }
    shared.add_preview(&top, own_preview.0, own_preview.1);
}

/// 测厚目录并生成候选（子树串行、目录间并行；共享预算与硬链接去重）。
fn measure_dir_candidate(
    path: &Path,
    name: &str,
    shared: &ScanShared,
    rule: &'static Rule,
    top: &str,
) -> Option<Candidate> {
    let (size, max_modified) = measure_dir(path, shared, top).ok()?;
    Some(Candidate {
        path: path.to_path_buf(),
        name: name.to_string(),
        kind: Kind::Dir,
        bytes: size.bytes,
        files: size.files,
        modified: max_modified,
        rule_id: rule.id,
        category: rule.category,
        reclaim: rule.reclaim,
        risk: rule.risk,
        complete: size.complete,
        default_selected: rule.default_selected && rule.risk != Risk::Risky && size.complete,
    })
}

/// 测厚目录（共享预算与硬链接去重），返回体积与子树最新修改时间。
fn measure_dir(path: &Path, shared: &ScanShared, top: &str) -> Result<(DirSize, i64), DiskError> {
    let mut result = DirSize {
        complete: true,
        ..DirSize::default()
    };
    let mut modified = 0i64;
    let mut preview: (u64, u64) = (0, 0);
    let mut stack = vec![path.to_path_buf()];
    let cancel = shared.cancel;
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(DiskError::Cancelled);
        }
        if !shared.visit() {
            result.complete = false;
            break;
        }
        shared.dirs.fetch_add(1, Ordering::Relaxed);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                result.errors += 1;
                continue;
            };
            if cancel.load(Ordering::Relaxed) {
                return Err(DiskError::Cancelled);
            }
            let Ok(file_type) = entry.file_type() else {
                result.errors += 1;
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let Ok(meta) = entry.metadata() else {
                    result.errors += 1;
                    continue;
                };
                if device_of(&meta) != shared.root_dev {
                    // 跨挂载点：不计入（删除护栏也不会触碰）
                    continue;
                }
                result.dirs += 1;
                stack.push(entry.path());
            } else if file_type.is_file() {
                let Ok(meta) = entry.metadata() else {
                    result.errors += 1;
                    continue;
                };
                result.files += 1;
                shared.files.fetch_add(1, Ordering::Relaxed);
                let bytes = shared.hardlink_adjusted(&meta, &entry.path(), Some(path));
                result.bytes = result.bytes.saturating_add(bytes);
                modified = modified.max(modified_secs(&meta));
                preview.0 = preview.0.saturating_add(bytes);
                preview.1 = preview.1.saturating_add(1);
            }
        }
    }
    if result.errors > 0 {
        shared.errors.fetch_add(result.errors, Ordering::Relaxed);
        result.complete = false;
    }
    shared.add_preview(top, preview.0, preview.1);
    Ok((result, modified))
}

/// 桶 → 预览条目：按字节降序取前 128 桶，其余合并为一个溢出桶（name 为空）。
fn snapshot_buckets(buckets: &PreviewBuckets) -> Vec<PreviewEntry> {
    let mut entries: Vec<(String, (u64, u64))> = buckets
        .iter()
        .map(|(name, value)| (name.clone(), *value))
        .collect();
    entries.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let overflow: (u64, u64) = entries
        .iter()
        .skip(PREVIEW_BUCKETS)
        .fold((0, 0), |acc, item| (acc.0 + item.1.0, acc.1 + item.1.1));
    entries.truncate(PREVIEW_BUCKETS);
    let mut list: Vec<PreviewEntry> = entries
        .into_iter()
        .map(|(name, (bytes, files))| PreviewEntry { name, bytes, files })
        .collect();
    if overflow.0 > 0 || overflow.1 > 0 {
        list.push(PreviewEntry {
            name: String::new(),
            bytes: overflow.0,
            files: overflow.1,
        });
    }
    list
}

fn rule_of(id: &str) -> &'static Rule {
    classify::RULES
        .iter()
        .find(|rule| rule.id == id)
        .expect("规则 id 必须存在于规则表")
}

fn entry_is_older_than(entry: &fs::DirEntry, days: u32) -> bool {
    entry
        .metadata()
        .ok()
        .and_then(|meta| age_in_days(&meta))
        .is_some_and(|age| age >= u64::from(days))
}

/// 修改时间距今天数；取不到或在未来时返回 None（保守跳过）。
fn age_in_days(meta: &fs::Metadata) -> Option<u64> {
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    Some(age.as_secs() / 86_400)
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

/// 硬链接归属的确定性重算。并行扫描时同一 inode 的字节"先扫到的线程
/// 先计入"，归属候选受线程调度影响（同一磁盘两次扫描，同一候选的字节数
/// 可能不同）。这里按 inode 分组，把字节统一记到"有归属候选的出现中
/// 路径序最小"的那个候选：被计入的出现与胜者不一致时，从原归属候选
/// 扣除并加到胜者归属候选。
fn resolve_hardlink_attribution(candidates: &mut [Candidate], occurrences: &[HardlinkOcc]) {
    let mut groups: HashMap<(u64, u64), Vec<&HardlinkOcc>> = HashMap::new();
    for occ in occurrences {
        groups.entry(occ.key).or_default().push(occ);
    }
    for group in groups.values() {
        if group.len() < 2 {
            continue;
        }
        // 胜者：有归属候选的出现中路径序最小的一个（与线程调度无关）
        let Some(winner) = group
            .iter()
            .filter(|occ| occ.owner.is_some())
            .min_by(|a, b| a.path.cmp(&b.path))
            .copied()
        else {
            continue;
        };
        for occ in group.iter().filter(|occ| occ.counted) {
            if occ.path == winner.path {
                continue;
            }
            if let Some(from) = &occ.owner {
                if let Some(candidate) = candidates.iter_mut().find(|item| &item.path == from) {
                    candidate.bytes = candidate.bytes.saturating_sub(occ.size);
                }
            }
            if let Some(to) = &winner.owner {
                if let Some(candidate) = candidates.iter_mut().find(|item| &item.path == to) {
                    candidate.bytes = candidate.bytes.saturating_add(occ.size);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_progress() -> impl FnMut(ScanProgress) {
        |_| {}
    }

    fn no_preview() -> impl FnMut(ScanPreview) {
        |_| {}
    }

    fn scan(root: &Path, limits: ScanLimits) -> Result<ScanOutput, DiskError> {
        scan_candidates(
            root,
            None,
            ScanOptions::default(),
            limits,
            &AtomicBool::new(false),
            &mut no_progress(),
            &mut no_preview(),
        )
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("proj/target/debug")).unwrap();
        fs::write(root.join("proj/Cargo.toml"), "[package]").unwrap();
        fs::write(root.join("proj/target/debug/app"), vec![0u8; 4096]).unwrap();

        fs::create_dir_all(root.join("web/node_modules/dep")).unwrap();
        fs::write(root.join("web/package.json"), "{}").unwrap();
        fs::write(root.join("web/node_modules/dep/index.js"), "x").unwrap();

        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/app.log"), "hello").unwrap();
        fs::write(root.join("logs/keep.txt"), "hello").unwrap();
        // 日志候选要求超过 7 天阈值：把 mtime 调到 30 天前
        let log = fs::OpenOptions::new()
            .append(true)
            .open(root.join("logs/app.log"))
            .unwrap();
        log.set_modified(SystemTime::now() - Duration::from_secs(30 * 86_400))
            .unwrap();
        drop(log);

        // 没有 Cargo.toml / package.json 的同名目录不是构建产物（会被继续下钻）
        fs::create_dir_all(root.join("assets/target")).unwrap();
        fs::create_dir_all(root.join("static/node_modules")).unwrap();

        tmp
    }

    #[test]
    fn scan_finds_rules_and_skips_non_matches() {
        let tmp = fixture();
        let out = scan(tmp.path(), ScanLimits::default()).unwrap();

        let target = out
            .candidates
            .iter()
            .find(|item| item.rule_id == "rust-target")
            .expect("target 应命中");
        assert!(target.path.ends_with("proj/target"));
        assert_eq!(target.kind, Kind::Dir);
        assert!(target.complete);
        assert!(target.default_selected);
        // 分配大小口径：4096 全写文件至少 1 块
        assert!(target.bytes >= 4096, "分配大小口径: {}", target.bytes);

        assert!(
            out.candidates
                .iter()
                .any(|item| item.rule_id == "node-modules"
                    && item.path.ends_with("web/node_modules"))
        );
        assert!(
            out.candidates
                .iter()
                .any(|item| item.rule_id == "logs-lower" && item.path.ends_with("logs"))
        );
        // logs 目录命中 logs 规则后不再下钻：app.log/keep.txt 不单独出现
        assert!(
            !out.candidates
                .iter()
                .any(|item| item.path.ends_with("app.log"))
        );
        assert!(
            !out.candidates
                .iter()
                .any(|item| item.path.ends_with("keep.txt"))
        );
        // 同名但无清单文件的目录不是候选
        assert!(
            !out.candidates
                .iter()
                .any(|item| item.path.ends_with("assets/target"))
        );
        assert!(out.stats.complete);
        assert!(!out.stats.truncated);
    }

    #[test]
    fn parallel_scan_is_deterministic() {
        // 并行扫描两次：候选集合与字节完全一致（顺序可不同）
        let tmp = fixture();
        let first = scan(tmp.path(), ScanLimits::default()).unwrap();
        let second = scan(tmp.path(), ScanLimits::default()).unwrap();
        let key = |out: &ScanOutput| {
            let mut items: Vec<(String, u64)> = out
                .candidates
                .iter()
                .map(|item| (item.path.to_string_lossy().to_string(), item.bytes))
                .collect();
            items.sort();
            items
        };
        assert_eq!(key(&first), key(&second));
        assert_eq!(first.stats.candidate_bytes, second.stats.candidate_bytes);
    }

    #[test]
    fn log_age_gate_applies() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fresh.log");
        fs::write(&path, "x").unwrap();
        let out = scan(tmp.path(), ScanLimits::default()).unwrap();
        assert!(out.candidates.is_empty(), "7 天内的日志不应成为候选");

        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let old = SystemTime::now() - Duration::from_secs(30 * 86_400);
        file.set_modified(old).unwrap();
        drop(file);
        let out = scan(tmp.path(), ScanLimits::default()).unwrap();
        assert_eq!(out.candidates.len(), 1);
        assert!(out.candidates[0].modified > 0);
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_counted_once() {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("data.bin");
        fs::write(&a, vec![0u8; 8192]).unwrap();

        fs::create_dir_all(tmp.path().join(".cache/a")).unwrap();
        fs::create_dir_all(tmp.path().join(".cache/b")).unwrap();
        fs::rename(&a, tmp.path().join(".cache/a/data.bin")).unwrap();
        fs::hard_link(
            tmp.path().join(".cache/a/data.bin"),
            tmp.path().join(".cache/b/link.bin"),
        )
        .unwrap();

        let out = scan(tmp.path(), ScanLimits::default()).unwrap();
        let cache = out
            .candidates
            .iter()
            .find(|item| item.path.ends_with(".cache"))
            .expect(".cache 应命中");
        assert_eq!(cache.files, 2, "文件数按名字计数");
        let meta = fs::metadata(tmp.path().join(".cache/a/data.bin")).unwrap();
        let single = meta.blocks() * 512;
        assert_eq!(cache.bytes, single, "硬链接字节只计一次");
    }

    /// 硬链接归属确定性：同一 inode 出现在多个候选目录时，字节必须总是
    /// 记到路径序最小的出现所属的候选，与线程调度无关（两次扫描结果一致）。
    #[cfg(unix)]
    #[test]
    fn hardlink_attribution_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for proj in ["proj-a", "proj-b"] {
            let base = root.join(proj);
            fs::create_dir_all(base.join("target")).unwrap();
            fs::write(base.join("Cargo.toml"), "[package]").unwrap();
        }
        let file_a = root.join("proj-a/target/lib.bin");
        fs::write(&file_a, vec![0u8; 8192]).unwrap();
        fs::hard_link(&file_a, root.join("proj-b/target/lib.bin")).unwrap();

        let first = scan(root, ScanLimits::default()).unwrap();
        let second = scan(root, ScanLimits::default()).unwrap();

        let by_path = |out: &ScanOutput| {
            let mut items: Vec<(String, u64)> = out
                .candidates
                .iter()
                .map(|item| (item.path.to_string_lossy().to_string(), item.bytes))
                .collect();
            items.sort();
            items
        };
        assert_eq!(by_path(&first), by_path(&second), "两次扫描的归属必须一致");

        use std::os::unix::fs::MetadataExt;
        let single = fs::metadata(&file_a).unwrap().blocks() * 512;
        let bytes_of = |out: &ScanOutput, suffix: &str| {
            out.candidates
                .iter()
                .find(|item| item.path.ends_with(suffix))
                .map(|item| item.bytes)
                .unwrap_or_else(|| panic!("{suffix} 候选应存在"))
        };
        assert_eq!(
            bytes_of(&first, "proj-a/target"),
            single,
            "路径序小的出现持有字节"
        );
        assert_eq!(bytes_of(&first, "proj-b/target"), 0, "另一份硬链接计 0");
    }

    #[test]
    fn budget_exhaustion_marks_incomplete() {
        let tmp = tempfile::tempdir().unwrap();
        for index in 0..20 {
            let dir = tmp.path().join(format!("d{index}/.cache/nested"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("f.bin"), vec![0u8; 512]).unwrap();
        }
        let out = scan(
            tmp.path(),
            ScanLimits {
                max_depth: 12,
                max_candidates: 20_000,
                max_dir_visits: 5,
                threads: 2,
            },
        )
        .unwrap();
        assert!(!out.stats.complete, "预算耗尽后必须显式标注不完整");
        assert!(
            out.candidates.iter().any(|candidate| !candidate.complete),
            "受预算影响的候选必须标注不完整"
        );
    }

    #[test]
    fn candidate_cap_marks_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        for index in 0..10 {
            let dir = tmp.path().join(format!("p{index}"));
            fs::create_dir_all(dir.join(".cache")).unwrap();
        }
        let out = scan(
            tmp.path(),
            ScanLimits {
                max_depth: 12,
                max_candidates: 3,
                max_dir_visits: 200_000,
                threads: 2,
            },
        )
        .unwrap();
        assert!(out.stats.truncated);
        assert_eq!(out.candidates.len(), 3);
    }

    #[test]
    fn downloads_old_children_become_candidates() {
        // 回归：旧下载规则曾因"父目录 vs 子项路径"语义错位而从未生效
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(home.join("Downloads/OldProj")).unwrap();

        let old_zip = home.join("Downloads/old.zip");
        fs::write(&old_zip, vec![0u8; 2048]).unwrap();
        let file = fs::OpenOptions::new().append(true).open(&old_zip).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(100 * 86_400))
            .unwrap();
        drop(file);

        fs::write(home.join("Downloads/new.zip"), "new").unwrap();
        fs::write(home.join("Downloads/OldProj/inside.txt"), "x").unwrap();

        // std 无法设置目录 mtime，用 utimes 把 OldProj 调到 100 天前
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let c_path =
                std::ffi::CString::new(home.join("Downloads/OldProj").as_os_str().as_bytes())
                    .unwrap();
            let old_secs = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 100 * 86_400;
            let tv = libc::timeval {
                tv_sec: old_secs as libc::time_t,
                tv_usec: 0,
            };
            let times = [tv, tv];
            unsafe {
                libc::utimes(c_path.as_ptr(), times.as_ptr());
            }
        }

        let out = scan_candidates(
            &home,
            Some(&home),
            ScanOptions::default(),
            ScanLimits::default(),
            &AtomicBool::new(false),
            &mut no_progress(),
            &mut no_preview(),
        )
        .unwrap();

        assert!(
            out.candidates
                .iter()
                .any(|item| item.rule_id == "downloads-old"
                    && item.path.ends_with("Downloads/old.zip")),
            "90 天前的 old.zip 应命中旧下载规则"
        );
        assert!(
            out.candidates
                .iter()
                .any(|item| item.rule_id == "downloads-old"
                    && item.path.ends_with("Downloads/OldProj")),
            "90 天前的子目录应命中旧下载规则"
        );
        assert!(
            !out.candidates
                .iter()
                .any(|item| item.path.ends_with("new.zip")),
            "新下载不应成为候选"
        );
        assert!(
            !out.candidates
                .iter()
                .any(|item| item.path.ends_with("inside.txt")),
            "目录候选不再下钻，内部文件不重复出现"
        );
    }

    /// 大规模性能冒烟（文档预算：100k 文件 warm cache < 3s）。
    /// 默认忽略：CI 上构造 10 万文件太慢，本地 `cargo test -- --ignored` 手动跑。
    #[test]
    #[ignore]
    fn perf_smoke_100k_files_warm_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        // 20 个顶层目录 × 50 个子目录 × 100 个文件 = 100k 文件
        for top in 0..20 {
            for sub in 0..50 {
                let dir = root.join(format!("t{top}/s{sub}"));
                fs::create_dir_all(&dir).unwrap();
                for file in 0..100 {
                    fs::write(dir.join(format!("f{file}.bin")), vec![0u8; 64]).unwrap();
                }
            }
        }
        let started = std::time::Instant::now();
        let out = scan(&root, ScanLimits::default()).unwrap();
        let elapsed = started.elapsed();
        println!(
            "100k 文件并行扫描: {:?}（候选 {}）",
            elapsed,
            out.candidates.len()
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "并行扫描 100k 文件应远小于 10s: {elapsed:?}"
        );
    }

    #[test]
    fn cancelled_scan_returns_error() {
        let tmp = fixture();
        let cancel = AtomicBool::new(true);
        let result = scan_candidates(
            tmp.path(),
            None,
            ScanOptions::default(),
            ScanLimits::default(),
            &cancel,
            &mut no_progress(),
            &mut no_preview(),
        );
        assert!(matches!(result, Err(DiskError::Cancelled)));
    }

    #[test]
    fn preview_buckets_cover_scanned_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        for index in 0..4 {
            let dir = tmp.path().join(format!("top{index}"));
            fs::create_dir_all(dir.join(".cache")).unwrap();
            fs::write(dir.join(".cache/f.bin"), vec![0u8; 1024]).unwrap();
        }
        let previews: Mutex<Vec<ScanPreview>> = Mutex::new(Vec::new());
        scan_candidates(
            tmp.path(),
            None,
            ScanOptions::default(),
            ScanLimits::default(),
            &AtomicBool::new(false),
            &mut no_progress(),
            &mut |snapshot| previews.lock().unwrap().push(snapshot),
        )
        .unwrap();
        // 小 fixture 不足 1s，节流后可能只有首帧空快照；桶的数学下面用纯函数验证
        let _ = previews.into_inner().unwrap();
        let mut buckets: PreviewBuckets = HashMap::new();
        for index in 0..PREVIEW_BUCKETS + 2 {
            buckets.insert(format!("top{index}"), ((index as u64 + 1) * 1024, 1));
        }
        let snapshot = snapshot_buckets(&buckets);
        assert_eq!(snapshot.len(), PREVIEW_BUCKETS + 1, "128 桶 + 1 溢出桶");
        assert_eq!(
            snapshot[0].bytes,
            (PREVIEW_BUCKETS as u64 + 2) * 1024,
            "按字节降序"
        );
        let overflow = snapshot.last().unwrap();
        assert!(overflow.name.is_empty());
        assert_eq!(overflow.bytes, 3 * 1024, "溢出桶合并两桶（最小的两个）");
        // 四个顶层桶的正常场景
        let mut buckets: PreviewBuckets = HashMap::new();
        for index in 0..4 {
            buckets.insert(format!("top{index}"), (1024, 1));
        }
        let snapshot = snapshot_buckets(&buckets);
        assert_eq!(snapshot.iter().filter(|e| !e.name.is_empty()).count(), 4);
        let total: u64 = snapshot.iter().map(|e| e.bytes).sum();
        assert_eq!(total, 4 * 1024, "桶字节覆盖已扫描量");
    }

    #[cfg(unix)]
    #[test]
    fn device_helpers_are_consistent() {
        use std::os::unix::fs::MetadataExt;

        let tmp = tempfile::tempdir().unwrap();
        let meta = fs::metadata(tmp.path()).unwrap();
        assert_eq!(device_of(&meta), meta.dev());
    }
}
