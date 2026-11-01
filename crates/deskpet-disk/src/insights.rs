//! "值得看看"：从候选里挑出最值得用户注意的可回收大项。
//!
//! O(候选数) 的轻量后处理，比 treemap 更适合 388 宽的小面板：
//! 只取有可回收原因的目录、低于阈值的忽略、按字节降序取 Top N。
//! 候选扫描对目录型命中不下钻，天然无嵌套重复，无需再去重。

use serde::Serialize;

use crate::{
    classify::{Category, Risk},
    scan::Candidate,
};

/// 默认阈值：小于 64MB 不列出。
pub const MIN_INSIGHT_BYTES: u64 = 64 * 1024 * 1024;

/// 一条"值得看看"。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub files: u64,
    pub category: Category,
    pub reclaim: Option<crate::classify::Reclaim>,
    pub risk: Risk,
}

/// 从候选中取 Top N 可回收大项。
pub fn worth_a_look(candidates: &[Candidate], min_bytes: u64, top: usize) -> Vec<Insight> {
    let mut items: Vec<&Candidate> = candidates
        .iter()
        .filter(|item| item.reclaim.is_some() && item.bytes >= min_bytes)
        .collect();
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    items
        .into_iter()
        .take(top)
        .map(|item| Insight {
            path: item.path.to_string_lossy().to_string(),
            name: item.name.clone(),
            bytes: item.bytes,
            files: item.files,
            category: item.category,
            reclaim: item.reclaim,
            risk: item.risk,
        })
        .collect()
}

/// 从分析树中取"值得看看"：有可回收原因的目录，低于阈值忽略，
/// 命中的目录不再下钻（只取顶层发现，避免嵌套重复），按字节降序 Top N。
pub fn insights_from_tree(
    node: &crate::tree::TreeNode,
    root_path: &std::path::Path,
    min_bytes: u64,
    top: usize,
) -> Vec<Insight> {
    let mut found: Vec<Insight> = Vec::new();
    fn walk(
        node: &crate::tree::TreeNode,
        path: &std::path::Path,
        min_bytes: u64,
        found: &mut Vec<Insight>,
    ) {
        if node.reclaim.is_some() {
            if node.bytes >= min_bytes {
                found.push(Insight {
                    path: path.to_string_lossy().to_string(),
                    name: node.name.clone(),
                    bytes: node.bytes,
                    files: node.files,
                    category: node.category,
                    reclaim: node.reclaim,
                    risk: node.risk,
                });
            }
            // 命中即停：内部细节由该目录的候选/复核承担
            return;
        }
        for child in &node.children {
            walk(child, &path.join(&child.name), min_bytes, found);
        }
    }
    walk(node, root_path, min_bytes, &mut found);
    found.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    found.truncate(top);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        classify::{Reclaim, Risk},
        scan::Candidate,
    };
    use std::path::PathBuf;

    fn candidate(path: &str, bytes: u64, reclaim: Option<Reclaim>) -> Candidate {
        Candidate {
            path: PathBuf::from(path),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            kind: crate::classify::Kind::Dir,
            bytes,
            files: 1,
            modified: 0,
            rule_id: "test",
            category: Category::Cache,
            reclaim,
            risk: Risk::Safe,
            complete: true,
            default_selected: true,
        }
    }

    #[test]
    fn filters_small_and_non_reclaimable() {
        let candidates = vec![
            candidate("a/cache", 1024, Some(Reclaim::Regenerable)),
            candidate("b/cache", MIN_INSIGHT_BYTES + 1, Some(Reclaim::Regenerable)),
            candidate("c/git", MIN_INSIGHT_BYTES + 2, None),
        ];
        let insights = worth_a_look(&candidates, MIN_INSIGHT_BYTES, 8);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].name, "cache");
    }

    #[test]
    fn insights_from_tree_takes_top_reclaim_dirs_without_nesting() {
        use crate::{classify::Risk, tree::TreeNode};
        use std::path::Path;

        fn dir_node(
            name: &str,
            bytes: u64,
            reclaim: Option<Reclaim>,
            children: Vec<TreeNode>,
        ) -> TreeNode {
            TreeNode {
                name: name.to_string(),
                bytes,
                own_bytes: 0,
                files: 1,
                dirs: 0,
                modified: 0,
                category: Category::Cache,
                reclaim,
                risk: Risk::Safe,
                read_error: false,
                pruned: false,
                children,
            }
        }

        // root(Other) → Library(Other) → Caches(Reclaim, 1.2GB)；proj → target(Reclaim)
        let tree = dir_node(
            "home",
            0,
            None,
            vec![dir_node(
                "Library",
                0,
                None,
                vec![
                    dir_node("Caches", 1_200_000_000, Some(Reclaim::Regenerable), vec![]),
                    dir_node("Small", 1000, Some(Reclaim::Regenerable), vec![]),
                ],
            )],
        );
        let insights = insights_from_tree(&tree, Path::new("/home"), 64 * 1024 * 1024, 8);
        assert_eq!(insights.len(), 1, "小于阈值的忽略");
        assert_eq!(insights[0].name, "Caches");

        // 嵌套：home → proj(Other) → target(Reclaim) 命中；proj 不命中则下钻
        let tree2 = dir_node(
            "home",
            0,
            None,
            vec![dir_node(
                "proj",
                0,
                None,
                vec![dir_node(
                    "target",
                    800_000_000,
                    Some(Reclaim::BuildOutput),
                    vec![],
                )],
            )],
        );
        let insights = insights_from_tree(&tree2, Path::new("/home"), 64 * 1024 * 1024, 8);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].name, "target");
        assert!(insights[0].path.ends_with("proj/target"));
    }

    #[test]
    fn sorts_by_bytes_and_takes_top() {
        let candidates = vec![
            candidate("s", 300, Some(Reclaim::Trash)),
            candidate("l", 900, Some(Reclaim::Regenerable)),
            candidate("m", 600, Some(Reclaim::BuildOutput)),
        ];
        let insights = worth_a_look(&candidates, 1, 2);
        assert_eq!(insights.len(), 2);
        assert_eq!(insights[0].bytes, 900);
        assert_eq!(insights[1].bytes, 600);
    }
}
