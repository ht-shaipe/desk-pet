//! Squarified treemap 布局（Bruls/Huizing/van Wijk）。
//!
//! 布局在基础像素坐标系完成并一次性返回 tile 数组；缩放/平移由前端
//! 只改视图变换，不重新布局。布局是纯几何计算，可独立单测：
//! 面积守恒、宽高比有界、header 不重叠、超出预算合并为 Others。

use std::path::Path;

use serde::Serialize;

use crate::{
    classify::{Category, Reclaim, Risk},
    tree::TreeNode,
};

/// 布局度量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Bytes,
    Files,
}

/// tile 种类：目录 / 所在目录的直接文件聚合 / 超限合并项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TileKind {
    Dir,
    Files,
    Others,
}

/// 一个可绘制的矩形区域。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    /// 绝对路径（Files/Others 指向所在目录）
    pub path: String,
    pub name: String,
    pub kind: TileKind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub depth: usize,
    /// 目录 tile 的标题条（x,y,w,h）；小 tile 无 header
    pub header: Option<[f64; 4]>,
    pub bytes: u64,
    pub files: u64,
    pub category: Category,
    pub reclaim: Option<Reclaim>,
    pub risk: Risk,
    /// Others tile 合并的数量
    pub count: Option<usize>,
    /// 是否可下钻
    pub has_children: bool,
}

/// 布局选项（对齐 disktree 默认值）。
#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions {
    pub max_depth: usize,
    pub max_children: usize,
    pub min_tile: f64,
    pub header_outer: f64,
    pub header_inner: f64,
    pub max_tiles: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_children: 96,
            min_tile: 5.0,
            header_outer: 20.0,
            header_inner: 15.0,
            max_tiles: 2000,
        }
    }
}

/// 布局结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutResult {
    pub root: String,
    pub tiles: Vec<Tile>,
    pub total_bytes: u64,
    pub total_files: u64,
    pub truncated: bool,
}

/// 对 `node`（位于 `root_path`）在给定画布内布局。
///
/// `filter` 为名称子串过滤（大小写不敏感）：命中的节点按整棵子树计量，
/// 未命中但有命中后代的节点作为容器保留。
pub fn layout(
    node: &TreeNode,
    root_path: &Path,
    width: f64,
    height: f64,
    metric: Metric,
    filter: Option<&str>,
    opts: &LayoutOptions,
) -> LayoutResult {
    let mut ctx = Ctx {
        tiles: Vec::new(),
        truncated: false,
        opts: *opts,
        metric,
        filter: filter.map(str::to_lowercase),
    };
    if width >= opts.min_tile && height >= opts.min_tile {
        layout_children(
            node,
            &root_path.to_string_lossy(),
            0.0,
            0.0,
            width,
            height,
            0,
            &mut ctx,
        );
    }
    LayoutResult {
        root: root_path.to_string_lossy().to_string(),
        tiles: ctx.tiles,
        total_bytes: node.bytes,
        total_files: node.files,
        truncated: ctx.truncated,
    }
}

struct Ctx {
    tiles: Vec<Tile>,
    truncated: bool,
    opts: LayoutOptions,
    metric: Metric,
    filter: Option<String>,
}

/// 节点在当前度量/过滤下的权重。
fn weight_of(node: &TreeNode, metric: Metric, filter: Option<&str>) -> f64 {
    let raw = match metric {
        Metric::Bytes => node.bytes as f64,
        Metric::Files => node.files as f64,
    };
    match filter {
        // 过滤时权重在子项收集处单独计算
        Some(_) => raw,
        None => raw,
    }
}

/// 过滤语义下的子树权重：命中 → 整棵子树；未命中 → 命中后代之和。
/// 命中时不能再加子项，否则祖先与后代同时命中会重复计量。
fn match_weight(node: &TreeNode, metric: Metric, filter: &str) -> f64 {
    if node.name.to_lowercase().contains(filter) {
        return match metric {
            Metric::Bytes => node.bytes as f64,
            Metric::Files => node.files as f64,
        };
    }
    node.children
        .iter()
        .map(|child| match_weight(child, metric, filter))
        .sum()
}

/// 直接子文件数（不含子目录）。`files` 是子树聚合值。
fn own_file_count(node: &TreeNode) -> u64 {
    node.files
        .saturating_sub(node.children.iter().map(|child| child.files).sum())
}

/// 收集某目录的子项并布局；`rect` 是该目录的内容区。
fn layout_children(
    node: &TreeNode,
    node_path: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    depth: usize,
    ctx: &mut Ctx,
) {
    if depth >= ctx.opts.max_depth {
        return;
    }
    if ctx.tiles.len() >= ctx.opts.max_tiles {
        ctx.truncated = true;
        return;
    }
    let header_h = if depth == 0 {
        ctx.opts.header_outer
    } else {
        ctx.opts.header_inner
    };
    let content_y = y + header_h;
    let content_h = h - header_h;
    if w < ctx.opts.min_tile || content_h < ctx.opts.min_tile {
        return;
    }

    // 收集子项：目录 + （无过滤时的）直接文件聚合
    struct Item<'a> {
        weight: f64,
        kind: ItemKind<'a>,
    }
    enum ItemKind<'a> {
        Dir(&'a TreeNode),
        Files,
    }

    let mut items: Vec<Item> = Vec::new();
    match &ctx.filter {
        None => {
            for child in &node.children {
                let weight = weight_of(child, ctx.metric, None);
                if weight > 0.0 {
                    items.push(Item {
                        weight,
                        kind: ItemKind::Dir(child),
                    });
                }
            }
            let own_files = own_file_count(node);
            let has_files = match ctx.metric {
                Metric::Bytes => node.own_bytes > 0,
                Metric::Files => own_files > 0,
            };
            if has_files {
                let weight = match ctx.metric {
                    Metric::Bytes => node.own_bytes as f64,
                    Metric::Files => own_files as f64,
                };
                if weight > 0.0 {
                    items.push(Item {
                        weight,
                        kind: ItemKind::Files,
                    });
                }
            }
        }
        Some(filter) => {
            for child in &node.children {
                let weight = match_weight(child, ctx.metric, filter);
                if weight > 0.0 {
                    items.push(Item {
                        weight,
                        kind: ItemKind::Dir(child),
                    });
                }
            }
            // 过滤时文件无名称可匹配，不产出文件 tile
        }
    }
    let total: f64 = items.iter().map(|item| item.weight).sum();
    if total <= 0.0 {
        return;
    }

    // 小于 min_tile² 等效面积的项并入 Others
    let content_area = w * content_h;
    let min_weight = ctx.opts.min_tile * ctx.opts.min_tile / content_area * total;
    let mut big: Vec<Item> = Vec::new();
    let mut others_weight = 0.0f64;
    let mut others_count = 0usize;
    for item in items {
        if item.weight >= min_weight {
            big.push(item);
        } else {
            others_weight += item.weight;
            others_count += 1;
        }
    }
    big.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    if big.len() > ctx.opts.max_children {
        let tail = big.split_off(ctx.opts.max_children - 1);
        others_weight += tail.iter().map(|item| item.weight).sum::<f64>();
        others_count += tail.len();
    }

    // 构建 squarify 输入（保持 big 顺序 + Others 殿后）
    let mut inputs: Vec<(usize, f64)> = Vec::with_capacity(big.len() + 1);
    for (order, item) in big.iter().enumerate() {
        inputs.push((order, item.weight));
    }
    let mut others_index: Option<usize> = None;
    if others_weight > 0.0 {
        others_index = Some(inputs.len());
        inputs.push((inputs.len(), others_weight));
    }
    if inputs.is_empty() {
        return;
    }

    let placed = squarify(&inputs, x, content_y, w, content_h);
    for tile in placed {
        let (tx, ty, tw, th) = (tile.x, tile.y, tile.w, tile.h);
        let header = if th >= header_h + ctx.opts.min_tile {
            Some([tx, ty, tw, header_h])
        } else {
            None
        };
        if ctx.tiles.len() >= ctx.opts.max_tiles {
            ctx.truncated = true;
            return;
        }
        if Some(tile.index) == others_index {
            ctx.tiles.push(Tile {
                path: node_path.to_string(),
                name: String::new(),
                kind: TileKind::Others,
                x: tx,
                y: ty,
                w: tw,
                h: th,
                depth,
                header: None,
                bytes: 0,
                files: 0,
                category: node.category,
                reclaim: None,
                risk: node.risk,
                count: Some(others_count),
                has_children: false,
            });
            continue;
        }
        // tile.index 即 big 的下标（Others 殿后不受影响）
        let item = &big[tile.index];
        if matches!(item.kind, ItemKind::Files) {
            ctx.tiles.push(Tile {
                path: node_path.to_string(),
                name: String::new(),
                kind: TileKind::Files,
                x: tx,
                y: ty,
                w: tw,
                h: th,
                depth,
                header: None,
                bytes: node.own_bytes,
                files: own_file_count(node),
                category: node.category,
                reclaim: None,
                risk: node.risk,
                count: None,
                has_children: false,
            });
            continue;
        }
        if let ItemKind::Dir(child) = item.kind {
            let child_path = join_path(node_path, &child.name);
            let self_matched = ctx
                .filter
                .as_ref()
                .is_some_and(|filter| child.name.to_lowercase().contains(filter));
            ctx.tiles.push(Tile {
                path: child_path.clone(),
                name: child.name.clone(),
                kind: TileKind::Dir,
                x: tx,
                y: ty,
                w: tw,
                h: th,
                depth,
                header,
                bytes: child.bytes,
                files: child.files,
                category: child.category,
                reclaim: child.reclaim,
                risk: child.risk,
                count: None,
                has_children: !child.children.is_empty()
                    || child.own_bytes > 0
                    || own_file_count(child) > 0,
            });
            // 命中过滤的目录按整棵子树呈现（不再下钻）；未命中的容器继续展开
            if !self_matched {
                layout_children(child, &child_path, tx, ty, tw, th, depth + 1, ctx);
            }
        }
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// 单个摆放结果。
struct Placed {
    index: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Squarified 布局：`items` 需按权重降序；面积按矩形面积归一化。
fn squarify(items: &[(usize, f64)], x: f64, y: f64, w: f64, h: f64) -> Vec<Placed> {
    let total: f64 = items.iter().map(|(_, weight)| weight).sum();
    if total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let scale = (w * h) / total;
    let areas: Vec<(usize, f64)> = items
        .iter()
        .map(|(index, weight)| (*index, weight * scale))
        .collect();

    let mut out: Vec<Placed> = Vec::with_capacity(areas.len());
    let mut rx = x;
    let mut ry = y;
    let mut rw = w;
    let mut rh = h;
    let mut index = 0usize;
    while index < areas.len() {
        let side = rw.min(rh);
        // 贪心建行：逐个尝试并入当前行，宽高比不再改善即截止
        let mut row: Vec<(usize, f64)> = vec![areas[index]];
        let mut row_sum = areas[index].1;
        let mut best = worst_ratio(&row, row_sum, side);
        let mut next = index + 1;
        while next < areas.len() {
            let cand_sum = row_sum + areas[next].1;
            let mut cand = row.clone();
            cand.push(areas[next]);
            let ratio = worst_ratio(&cand, cand_sum, side);
            if ratio <= best {
                row = cand;
                row_sum = cand_sum;
                best = ratio;
                next += 1;
            } else {
                break;
            }
        }
        let thickness = row_sum / side;
        if rw >= rh {
            // 左侧竖条：自上而下堆叠
            let mut cy = ry;
            for (item_index, area) in &row {
                let hh = area / thickness;
                out.push(Placed {
                    index: *item_index,
                    x: rx,
                    y: cy,
                    w: thickness,
                    h: hh,
                });
                cy += hh;
            }
            rx += thickness;
            rw -= thickness;
        } else {
            // 顶部横条：自左向右排列
            let mut cx = rx;
            for (item_index, area) in &row {
                let ww = area / thickness;
                out.push(Placed {
                    index: *item_index,
                    x: cx,
                    y: ry,
                    w: ww,
                    h: thickness,
                });
                cx += ww;
            }
            ry += thickness;
            rh -= thickness;
        }
        index = next;
    }
    out
}

/// 行内最大宽高比。
fn worst_ratio(row: &[(usize, f64)], row_sum: f64, side: f64) -> f64 {
    let thickness = row_sum / side;
    let mut worst = 0.0f64;
    for (_, area) in row {
        let length = area / thickness;
        if length <= 0.0 {
            continue;
        }
        let ratio = (thickness / length).max(length / thickness);
        worst = worst.max(ratio);
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{Category, Risk};

    fn node(name: &str, own: u64, children: Vec<TreeNode>) -> TreeNode {
        let files = children.iter().map(|c| c.files).sum::<u64>() + 1;
        let bytes = children.iter().map(|c| c.bytes).sum::<u64>() + own;
        TreeNode {
            name: name.to_string(),
            bytes,
            own_bytes: own,
            files,
            dirs: children.len() as u32,
            modified: 0,
            category: Category::Other,
            reclaim: None,
            risk: Risk::Risky,
            read_error: false,
            pruned: false,
            children,
        }
    }

    #[test]
    fn squarify_conserves_area() {
        let items: Vec<(usize, f64)> = (0..10).map(|i| (i, 100.0 - i as f64)).collect();
        let placed = squarify(&items, 0.0, 0.0, 400.0, 300.0);
        let area: f64 = placed.iter().map(|p| p.w * p.h).sum();
        assert!((area - 400.0 * 300.0).abs() < 1e-6, "面积守恒: {area}");
        assert_eq!(placed.len(), 10);
    }

    #[test]
    fn squarify_keeps_aspect_ratios_reasonable() {
        let items: Vec<(usize, f64)> = (0..20).map(|i| (i, 100.0 - i as f64)).collect();
        let placed = squarify(&items, 0.0, 0.0, 500.0, 400.0);
        let worst = placed
            .iter()
            .map(|p| (p.w / p.h).max(p.h / p.w))
            .fold(0.0f64, |a, b| a.max(b));
        assert!(worst < 6.0, "squarified 宽高比应有界: {worst}");
    }

    #[test]
    fn layout_emits_dir_and_files_tiles() {
        let tree = node(
            "root",
            1000,
            vec![node("a", 300, vec![]), node("b", 500, vec![])],
        );
        let result = layout(
            &tree,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Bytes,
            None,
            &LayoutOptions::default(),
        );
        assert_eq!(
            result
                .tiles
                .iter()
                .filter(|t| t.kind == TileKind::Dir)
                .count(),
            2
        );
        assert!(result.tiles.iter().any(|t| t.kind == TileKind::Files));
        // 面积守恒（一层）
        let area: f64 = result
            .tiles
            .iter()
            .filter(|t| t.depth == 0)
            .map(|t| t.w * t.h)
            .sum();
        assert!(
            (area - 800.0 * (600.0 - 20.0)).abs() < 1e-4,
            "一级 tile 铺满根 header 之下的内容区"
        );
    }

    #[test]
    fn layout_merges_overflow_into_others() {
        let children: Vec<TreeNode> = (0..30)
            .map(|i| node(&format!("d{i}"), 100 - i, vec![]))
            .collect();
        let tree = node("root", 0, children);
        let result = layout(
            &tree,
            Path::new("/data"),
            600.0,
            400.0,
            Metric::Bytes,
            None,
            &LayoutOptions {
                max_children: 8,
                ..LayoutOptions::default()
            },
        );
        let others: Vec<&Tile> = result
            .tiles
            .iter()
            .filter(|tile| tile.kind == TileKind::Others)
            .collect();
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].count, Some(23));
    }

    #[test]
    fn filter_keeps_matching_and_ancestors() {
        let tree = node(
            "root",
            0,
            vec![
                node("proj-a", 100, vec![node("target", 900, vec![])]),
                node("plain", 500, vec![]),
            ],
        );
        let result = layout(
            &tree,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Bytes,
            Some("target"),
            &LayoutOptions::default(),
        );
        // 只有 proj-a（容器）与其中的 target 出现；plain 不出现
        let names: Vec<&str> = result
            .tiles
            .iter()
            .filter(|tile| tile.kind == TileKind::Dir)
            .map(|tile| tile.name.as_str())
            .collect();
        assert!(names.contains(&"target"));
        assert!(names.contains(&"proj-a"));
        assert!(!names.contains(&"plain"));
        // 过滤权重不能重复计算祖先与后代
        let tree = node("proj-a", 0, vec![node("target", 900, vec![])]);
        assert_eq!(match_weight(&tree, Metric::Bytes, "target"), 900.0);
        // 过滤时不产出文件 tile
        assert!(!result.tiles.iter().any(|tile| tile.kind == TileKind::Files));
    }

    #[test]
    fn files_tile_uses_direct_file_count() {
        let mut root = node("root", 10, vec![node("child", 10, vec![])]);
        root.files = 2; // 1 个直接文件 + 1 个子文件
        root.own_bytes = 10;
        let result = layout(
            &root,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Files,
            None,
            &LayoutOptions::default(),
        );
        let files = result
            .tiles
            .iter()
            .find(|tile| tile.kind == TileKind::Files)
            .unwrap();
        assert_eq!(files.files, 1);
    }

    #[test]
    fn metric_files_uses_file_counts() {
        let a = {
            let mut n = node("a", 10, vec![]);
            n.files = 900;
            n.bytes = 10;
            n
        };
        let mut b = node("b", 5000, vec![]);
        b.files = 3;
        b.bytes = 5000;
        let tree = node("root", 0, vec![a, b]);
        let result = layout(
            &tree,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Files,
            None,
            &LayoutOptions::default(),
        );
        // files 度量下 a(900) 应占得比 b(3) 大
        let tile_a = result.tiles.iter().find(|t| t.name == "a").unwrap();
        let tile_b = result.tiles.iter().find(|t| t.name == "b").unwrap();
        assert!(tile_a.w * tile_a.h > tile_b.w * tile_b.h);
    }

    #[test]
    fn header_laid_out_inside_tile() {
        let tree = node("root", 0, vec![node("a", 100, vec![])]);
        let result = layout(
            &tree,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Bytes,
            None,
            &LayoutOptions::default(),
        );
        let tile = result.tiles.iter().find(|t| t.name == "a").unwrap();
        let header = tile.header.expect("足够大的 tile 应有 header");
        assert_eq!(header[1], 20.0, "子 tile 从根 header(20px) 之下开始");
        assert_eq!(header[3], 20.0, "顶层 header 高度");
    }

    #[test]
    fn paths_are_joined() {
        let tree = node("root", 0, vec![node("a", 100, vec![node("b", 50, vec![])])]);
        let result = layout(
            &tree,
            Path::new("/data"),
            800.0,
            600.0,
            Metric::Bytes,
            None,
            &LayoutOptions::default(),
        );
        let tile_a = result.tiles.iter().find(|t| t.name == "a").unwrap();
        assert_eq!(tile_a.path, "/data/a");
        let tile_b = result.tiles.iter().find(|t| t.name == "b").unwrap();
        assert_eq!(tile_b.path, "/data/a/b");
    }
}
