//! 分析窗口核心链路集成测试：树扫描 → 标记解析 → 计划 → 删除。
//! 对应面板上的"点色块 → Space 标记 → 复核清理 → 确认删除"操作。

use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use deskpet_disk::{
    removal::{self, RemovalMode},
    tree::{TreeLimits, locate, scan_tree},
};

fn no_progress() -> impl FnMut(deskpet_disk::tree::TreeProgress) {
    |_| {}
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = home.join("workspace/proj");
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    fs::write(root.join("target/debug/app"), vec![0u8; 4096]).unwrap();
    fs::create_dir_all(home.join("Library/Caches")).unwrap();
    fs::write(home.join("Library/Caches/data.bin"), vec![0u8; 1024]).unwrap();
    (tmp, home)
}

fn scan(home_root: &Path) -> Arc<deskpet_disk::tree::ScanTree> {
    Arc::new(
        scan_tree(
            home_root,
            home_root,
            Some(home_root),
            TreeLimits::default(),
            &AtomicBool::new(false),
            &mut no_progress(),
        )
        .expect("树扫描应完成"),
    )
}

#[test]
fn mark_flow_locates_and_reports_sizes() {
    let (tmp, home) = fixture();
    let _ = tmp;
    let scan = scan(&home);

    // 用户在 treemap 里选中 target（tile 路径 = 视图根拼接），标记后应能解析出大小
    let target_path = home.join("workspace/proj/target");
    let marks = vec![target_path.to_string_lossy().to_string()];
    for path in &marks {
        let node =
            locate(&scan.tree, &scan.root, Path::new(path)).expect("标记路径必须能在树中定位");
        assert!(node.bytes > 0);
    }
    // 与 disk_analyze_marks 同语义
    let info_exists = marks
        .iter()
        .all(|path| locate(&scan.tree, &scan.root, Path::new(path)).is_some());
    assert!(info_exists, "标记项应全部 exists=true");
}

#[test]
fn plan_and_execute_remove_marked_target() {
    let (_tmp, home) = fixture();
    let scan = scan(&home);

    let target_path = home.join("workspace/proj/target");
    let requested = vec![(target_path.clone(), 4096, true)];
    let plan = removal::plan(&scan.root, &home, &requested);
    assert!(
        !plan.targets.is_empty(),
        "计划不应为空: blocked={:?}",
        plan.blocked
    );
    assert!(plan.blocked.is_empty(), "正常标记不应被拒绝");

    // 永久删除路径（回收站模式涉及用户真实废纸篓，不做自动测试）
    let outcome = removal::execute(
        &plan,
        RemovalMode::Permanent,
        &home,
        &AtomicBool::new(false),
        &mut |_| {},
        &mut |_, _, _, _| {},
    );
    assert_eq!(outcome.removed, 1, "永久删除应成功: {outcome:?}");
    assert!(!target_path.exists(), "标记的 target 应被删除");
    assert!(home.join("workspace/proj/Cargo.toml").exists(), "邻居保留");
}

#[test]
fn danger_stats_counts_git_repos_in_marked_subtree() {
    // 危险度：含 .git 的目录标记后，永久删除会被命令层拦截（见 analyze_cmds），
    // 这里验证统计本身：子树内 .git 目录数与非可回收字节
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("workspace/old-project");
    fs::create_dir_all(proj.join(".git")).unwrap();
    fs::create_dir_all(proj.join("src")).unwrap();
    fs::write(proj.join("main.rs"), "fn main() {}").unwrap();

    let scan = scan(&home);
    let node = locate(&scan.tree, &scan.root, &proj).unwrap();
    let (git, non_reclaim) = deskpet_disk::tree::danger_stats(node);
    assert_eq!(git, 1, ".git 仓库应计入危险度");
    assert!(non_reclaim > 0, "源码字节应计入非可回收内容");
}

#[test]
fn cancel_flag_stops_tree_scan() {
    let (_tmp, home) = fixture();
    let cancel = AtomicBool::new(true);
    cancel.store(true, Ordering::Relaxed);
    let result = scan_tree(
        &home,
        &home,
        Some(&home),
        TreeLimits::default(),
        &cancel,
        &mut no_progress(),
    );
    assert!(result.is_err(), "取消的扫描应返回 Err");
}
