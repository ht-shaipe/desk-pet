//! 分类与可回收原因：回答"这个目录是什么、为什么可以删、风险多高"。
//!
//! 规则以目录名为核心，必要时看同级清单文件（`target` 只有旁边有 `Cargo.toml`
//! 才算构建产物）。第一条命中的规则生效，规则表顺序即优先级。
//! 新增规则必须同时补正例/反例测试。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 内容类别（用于结果分组与配色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Code,
    AgentScratch,
    Toolchain,
    Synced,
    Git,
    Media,
    Documents,
    Cache,
    Other,
}

/// 可回收原因（为什么可以删）。`None` 表示不可回收（仅作信息展示或纯剪枝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reclaim {
    Regenerable,
    SyncHistory,
    PackageStore,
    BuildOutput,
    Reinstallable,
    SandboxLayers,
    Snapshots,
    Trash,
    Temporary,
}

/// 风险分级：Safe 可默认勾选；Caution 需用户手动选；Risky 不进推荐、默认不选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Safe,
    Caution,
    Risky,
}

/// 候选种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Dir,
    File,
}

/// 清理规则。
#[derive(Debug)]
pub struct Rule {
    pub id: &'static str,
    pub category: Category,
    pub reclaim: Option<Reclaim>,
    pub risk: Risk,
    /// 扫描完成后是否默认勾选（Risky 恒为 false）
    pub default_selected: bool,
    /// 命中后是否作为候选展示；false = 仅剪枝（不下钻也不展示，如应用数据/云盘）
    pub emit: bool,
    pub matcher: Matcher,
}

/// 匹配方式。`HomeSuffix` 相对家目录按组件匹配：单组件要求顶层精确相等
/// （`Pictures` 只匹配 ~/Pictures），多组件允许作为路径尾部
/// （`Library/Caches` 匹配任意层级下的 Library/Caches）。
#[derive(Debug)]
pub enum Matcher {
    Name(&'static str),
    /// 精确文件名（大小写敏感），用于 Docker.raw 这类固定文件名。
    FileName(&'static str),
    NameWithSibling {
        name: &'static str,
        sibling: &'static str,
    },
    HomeSuffix(&'static str),
    /// 家目录下 `suffix` 目录的直接子项按修改时间筛选（旧下载等）
    HomeChildrenWithAge {
        suffix: &'static str,
        days: u32,
    },
    /// 文件扩展名（大小写不敏感）+ 修改时间
    ExtensionWithAge {
        ext: &'static str,
        days: u32,
    },
}

/// `*.log` 的默认时间阈值；扫描选项可覆盖（0 = 不限时间）。
pub const LOG_DEFAULT_DAYS: u32 = 7;

/// 首批清理规则表（见 docs/disktree-integration-plan.md 附录 B）。
pub static RULES: &[Rule] = &[
    // —— 构建产物 / 依赖：只信同级清单文件，不靠目录名猜 ——
    Rule {
        id: "rust-target",
        matcher: Matcher::NameWithSibling {
            name: "target",
            sibling: "Cargo.toml",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "node-modules",
        matcher: Matcher::NameWithSibling {
            name: "node_modules",
            sibling: "package.json",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::Reinstallable),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "js-dist",
        matcher: Matcher::NameWithSibling {
            name: "dist",
            sibling: "package.json",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "js-build",
        matcher: Matcher::NameWithSibling {
            name: "build",
            sibling: "package.json",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "js-next",
        matcher: Matcher::NameWithSibling {
            name: ".next",
            sibling: "package.json",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "js-turbo",
        matcher: Matcher::NameWithSibling {
            name: ".turbo",
            sibling: "package.json",
        },
        category: Category::Code,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    // —— 工具链 / 包管理器存储 ——
    Rule {
        id: "xcode-derived-data",
        matcher: Matcher::HomeSuffix("Library/Developer/Xcode/DerivedData"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::BuildOutput),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "xcode-device-support",
        matcher: Matcher::HomeSuffix("Library/Developer/Xcode/iOS DeviceSupport"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::Regenerable),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "simulator-devices",
        matcher: Matcher::HomeSuffix("Library/Developer/CoreSimulator/Devices"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::Snapshots),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "npm-cacache",
        matcher: Matcher::Name("_cacache"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "pnpm-store",
        matcher: Matcher::Name(".pnpm-store"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "pnpm-store-home",
        matcher: Matcher::HomeSuffix("Library/pnpm/store"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "yarn-cache",
        matcher: Matcher::HomeSuffix(".yarn/cache"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "cargo-cache",
        matcher: Matcher::HomeSuffix(".cargo/registry/cache"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "go-mod-cache",
        matcher: Matcher::HomeSuffix("go/pkg/mod/cache/download"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "gradle-caches",
        matcher: Matcher::HomeSuffix(".gradle/caches"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::Regenerable),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "m2-repository",
        matcher: Matcher::HomeSuffix(".m2/repository"),
        category: Category::Toolchain,
        reclaim: Some(Reclaim::PackageStore),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    // —— 通用缓存 / 日志 ——
    Rule {
        id: "dot-cache",
        matcher: Matcher::Name(".cache"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Regenerable),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "caches",
        matcher: Matcher::Name("Caches"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Regenerable),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "sccache",
        matcher: Matcher::Name(".sccache"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Regenerable),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    Rule {
        id: "logs-upper",
        matcher: Matcher::Name("Logs"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Temporary),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "logs-lower",
        matcher: Matcher::Name("logs"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Temporary),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    // —— 回收站 / 旧下载 ——
    Rule {
        id: "trash",
        matcher: Matcher::HomeSuffix(".Trash"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Trash),
        risk: Risk::Safe,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "recycle-bin",
        matcher: Matcher::Name("$Recycle.Bin"),
        category: Category::Cache,
        reclaim: Some(Reclaim::Trash),
        risk: Risk::Safe,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "downloads-old",
        matcher: Matcher::HomeChildrenWithAge {
            suffix: "Downloads",
            days: 90,
        },
        category: Category::Documents,
        reclaim: Some(Reclaim::Temporary),
        risk: Risk::Caution,
        default_selected: false,
        emit: true,
    },
    // —— 文件规则 ——
    Rule {
        id: "log-files",
        matcher: Matcher::ExtensionWithAge {
            ext: "log",
            days: LOG_DEFAULT_DAYS,
        },
        category: Category::Cache,
        reclaim: Some(Reclaim::Temporary),
        risk: Risk::Safe,
        default_selected: true,
        emit: true,
    },
    // —— 信息位：可展示但风险高（.git 删除会丢仓库历史） ——
    Rule {
        id: "git-dir",
        matcher: Matcher::Name(".git"),
        category: Category::Git,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "ollama-models",
        matcher: Matcher::HomeSuffix(".ollama/models"),
        category: Category::Media,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: true,
    },
    Rule {
        id: "docker-disk",
        matcher: Matcher::FileName("Docker.raw"),
        category: Category::Toolchain,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: true,
    },
    // —— 纯剪枝：不展示不下钻（应用数据/云盘/媒体库，避免误删与触发云下载） ——
    Rule {
        id: "app-support",
        matcher: Matcher::HomeSuffix("Library/Application Support"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "mobile-documents",
        matcher: Matcher::HomeSuffix("Library/Mobile Documents"),
        category: Category::Synced,
        // 云盘同步目录不是“历史缓存”：绝不提供可回收原因。
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "cloud-docs",
        matcher: Matcher::HomeSuffix("Library/CloudDocs"),
        category: Category::Synced,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "mail",
        matcher: Matcher::HomeSuffix("Library/Mail"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "messages",
        matcher: Matcher::HomeSuffix("Library/Messages"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "containers",
        matcher: Matcher::HomeSuffix("Library/Containers"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "group-containers",
        matcher: Matcher::HomeSuffix("Library/Group Containers"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "photos",
        matcher: Matcher::HomeSuffix("Pictures"),
        category: Category::Media,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "movies",
        matcher: Matcher::HomeSuffix("Movies"),
        category: Category::Media,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "music",
        matcher: Matcher::HomeSuffix("Music"),
        category: Category::Media,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    // Windows 系统目录：纯剪枝
    Rule {
        id: "win-system",
        matcher: Matcher::Name("Windows"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "win-system32",
        matcher: Matcher::Name("System32"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "win-program-files",
        matcher: Matcher::Name("Program Files"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "win-program-files-x86",
        matcher: Matcher::Name("Program Files (x86)"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
    Rule {
        id: "win-volume-info",
        matcher: Matcher::Name("System Volume Information"),
        category: Category::Other,
        reclaim: None,
        risk: Risk::Risky,
        default_selected: false,
        emit: false,
    },
];

/// 目录规则匹配结果：返回命中的规则（第一条优先）。
///
/// `rel_home` 为相对家目录的路径（不在家目录下时为 None，Home 系规则全部跳过）。
pub fn match_dir<'a>(
    name: &str,
    rel_home: Option<&Path>,
    siblings: &dyn Fn(&str) -> bool,
) -> Option<&'a Rule> {
    RULES.iter().find(|rule| {
        let matched = match &rule.matcher {
            Matcher::Name(want) => name == *want,
            Matcher::FileName(_) => false,
            Matcher::NameWithSibling {
                name: want,
                sibling,
            } => name == *want && siblings(sibling),
            Matcher::HomeSuffix(suffix) => {
                rel_home.is_some_and(|rel| home_suffix_matches(rel, suffix))
            }
            Matcher::HomeChildrenWithAge { .. } | Matcher::ExtensionWithAge { .. } => false,
        };
        matched
    })
}

/// 文件规则匹配。
///
/// `log_days_override` 覆盖 `log-files` 规则的时间阈值（0 = 不限时间）。
pub fn match_file(
    name: &str,
    rel_home: Option<&Path>,
    modified_age_days: Option<u64>,
    log_days_override: Option<u32>,
) -> Option<&'static Rule> {
    RULES.iter().find(|rule| match &rule.matcher {
        Matcher::FileName(want) => name == *want,
        Matcher::ExtensionWithAge { ext, days } => {
            let actual_ext = Path::new(name)
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(ext));
            if !actual_ext {
                return false;
            }
            let threshold = log_days_override.unwrap_or(*days);
            match modified_age_days {
                Some(age) => threshold == 0 || age >= u64::from(threshold),
                // 取不到修改时间时保守跳过
                None => false,
            }
        }
        Matcher::HomeChildrenWithAge { suffix, days } => {
            let Some(rel) = rel_home else {
                return false;
            };
            // 该规则的候选是家目录顶层 Downloads 的**直接子项**：
            // rel 是子项自身路径，按其父目录匹配（Downloads/x.zip → Downloads）
            rel.parent() == Some(Path::new(suffix))
                && modified_age_days.is_some_and(|age| age >= u64::from(*days))
        }
        _ => false,
    })
}

/// 家目录相对路径是否匹配后缀：单组件要求整体相等，多组件允许作为尾部。
fn home_suffix_matches(rel: &Path, suffix: &str) -> bool {
    let suffix_path = Path::new(suffix);
    let suffix_len = suffix_path.components().count();
    if suffix_len == 1 {
        rel == suffix_path
    } else {
        ends_with_components(rel, suffix_path)
    }
}

/// 组件级的"路径以 suffix 结尾"（`ends_with` 已是组件语义，直接用）。
fn ends_with_components(path: &Path, suffix: &Path) -> bool {
    path.ends_with(suffix)
}

/// 目录的父目录（Downloads 等）是否命中 HomeChildrenWithAge 的父条件。
pub fn is_aged_children_parent(rel_home: Option<&Path>) -> bool {
    rel_home.is_some_and(|rel| {
        RULES.iter().any(|rule| match &rule.matcher {
            Matcher::HomeChildrenWithAge { suffix, .. } => rel.parent() == Some(Path::new(suffix)),
            _ => false,
        })
    })
}

/// HomeChildrenWithAge 规则的时间阈值（天）。
pub fn aged_children_days(rel_home: Option<&Path>) -> Option<u32> {
    rel_home.and_then(|rel| {
        RULES.iter().find_map(|rule| match &rule.matcher {
            Matcher::HomeChildrenWithAge { suffix, days }
                if rel.parent() == Some(Path::new(suffix)) =>
            {
                Some(*days)
            }
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_rule(name: &str, siblings: &[&str], rel_home: Option<&str>) -> Option<&'static Rule> {
        let siblings_set: Vec<String> = siblings.iter().map(|item| item.to_string()).collect();
        match_dir(name, rel_home.map(Path::new), &|want| {
            siblings_set.iter().any(|item| item == want)
        })
    }

    #[test]
    fn target_requires_cargo_toml_sibling() {
        let rule = dir_rule("target", &["Cargo.toml"], None).unwrap();
        assert_eq!(rule.id, "rust-target");
        assert!(dir_rule("target", &["README.md"], None).is_none());
    }

    #[test]
    fn node_modules_requires_package_json_sibling() {
        assert_eq!(
            dir_rule("node_modules", &["package.json"], None)
                .unwrap()
                .id,
            "node-modules"
        );
        assert!(dir_rule("node_modules", &[], None).is_none());
    }

    #[test]
    fn caches_matches_by_name() {
        assert_eq!(dir_rule("Caches", &[], None).unwrap().id, "caches");
        assert_eq!(dir_rule(".cache", &[], None).unwrap().id, "dot-cache");
    }

    #[test]
    fn home_suffix_requires_component_boundary() {
        // ~/Library/Caches 命中（尾部多组件）
        assert_eq!(
            dir_rule("Caches", &[], Some("Library/Caches")).map(|rule| rule.id),
            Some("caches")
        );
        // 单组件后缀只匹配顶层精确路径
        let rel = Path::new("Pictures");
        assert!(home_suffix_matches(rel, "Pictures"));
        assert!(!home_suffix_matches(Path::new("proj/Pictures"), "Pictures"));
        assert!(home_suffix_matches(
            Path::new("Library/Caches"),
            "Library/Caches"
        ));
        assert!(home_suffix_matches(
            Path::new("a/b/Library/Caches"),
            "Library/Caches"
        ));
        assert!(!home_suffix_matches(Path::new("Library"), "Library/Caches"));
    }

    #[test]
    fn git_dir_is_risky_informative() {
        let rule = dir_rule(".git", &[], None).unwrap();
        assert_eq!(rule.risk, Risk::Risky);
        assert!(rule.emit);
        assert!(!rule.default_selected);
    }

    #[test]
    fn app_support_is_pruned_not_emitted() {
        let rule = dir_rule(
            "Application Support",
            &[],
            Some("Library/Application Support"),
        )
        .unwrap();
        assert_eq!(rule.id, "app-support");
        assert!(!rule.emit);
    }

    #[test]
    fn log_file_age_gate() {
        // 8 天前的 .log：默认 7 天阈值命中
        assert!(match_file("app.log", None, Some(8), None).is_some());
        // 3 天前的 .log：默认阈值不命中，显式 0（不限）命中
        assert!(match_file("app.log", None, Some(3), None).is_none());
        assert!(match_file("app.log", None, Some(3), Some(0)).is_some());
        assert!(match_file("APP.LOG", None, Some(30), None).is_some());
        // 非 .log、以及取不到 mtime 的文件不命中
        assert!(match_file("notes.txt", None, Some(30), None).is_none());
        assert!(match_file("app.log", None, None, None).is_none());
    }

    #[test]
    fn downloads_children_use_age() {
        // Downloads 的直接子项按 90 天过滤（rel 传子项自身路径）
        assert!(
            match_file(
                "old.zip",
                Some(Path::new("Downloads/old.zip")),
                Some(91),
                None
            )
            .is_some()
        );
        assert!(
            match_file(
                "new.zip",
                Some(Path::new("Downloads/new.zip")),
                Some(10),
                None
            )
            .is_none()
        );
        // 非 Downloads 下的同名文件不走该规则
        assert!(
            match_file(
                "old.zip",
                Some(Path::new("proj/Downloads/old.zip")),
                Some(91),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn aged_children_parent_detection() {
        // 子项路径按父目录匹配
        assert!(is_aged_children_parent(Some(Path::new(
            "Downloads/OldProj"
        ))));
        assert!(is_aged_children_parent(Some(Path::new(
            "Downloads/old.zip"
        ))));
        // Downloads 本身不是自己的子项
        assert!(!is_aged_children_parent(Some(Path::new("Downloads"))));
        assert!(!is_aged_children_parent(Some(Path::new(
            "proj/Downloads/x"
        ))));
        assert_eq!(
            aged_children_days(Some(Path::new("Downloads/x.zip"))),
            Some(90)
        );
    }
}
