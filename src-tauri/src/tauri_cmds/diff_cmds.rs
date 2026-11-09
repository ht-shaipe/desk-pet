//! 文本对比：行级 diff（similar）+ 词级变化定位 + 语法感知高亮（tree-sitter）。
//! 思路参照语法感知 diff 工具（omnidiff 等）的能力点自研实现，未使用其代码。
//!
//! 输出按行展开的统一 diff 结构，前端可据此渲染统一/并排两种视图：
//! - kind: equal / delete / insert
//! - segments: 行内片段，带语法 token 类型与词级 changed 标记

use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// 高亮查询里可识别的 capture 名（tree-sitter-highlight 按名字匹配，未列出的忽略）。
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constant.character.escape",
    "constructor",
    "function",
    "function.builtin",
    "function.call",
    "function.macro",
    "include",
    "keyword",
    "keyword.function",
    "keyword.operator",
    "label",
    "method",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "type.definition",
    "variable",
    "variable.builtin",
    "variable.parameter",
    "variable.other.member",
    "namespace",
];

/// capture 名归并到前端配色的小集合（None = 无色）。
fn token_kind(name: &str) -> Option<&'static str> {
    let kind = match name {
        "comment" => "comment",
        "string" | "string.special" => "string",
        "number" | "constant.character.escape" => "number",
        "constant" | "constant.builtin" => "constant",
        "constructor" => "function",
        "keyword.operator" | "operator" => "operator",
        n if n.starts_with("keyword") || n == "include" || n == "namespace" => "keyword",
        n if n.starts_with("function") || n.starts_with("method") => "function",
        n if n.starts_with("type") => "type",
        n
        if n.starts_with("property")
            || n == "variable.other.member"
            || n == "attribute"
            || n == "tag"
            || n == "label" =>
        {
            "property"
        }
        n if n.starts_with("variable") => "variable",
        n if n.starts_with("punctuation") => "punctuation",
        _ => return None,
    };
    Some(kind)
}

/// 按语言 ID 构建 tree-sitter 高亮配置；未知语言/查询编译失败返回 None（降级纯文本）。
fn make_language_config(lang_id: &str) -> Option<HighlightConfiguration> {
    use tree_sitter::Language;
    let (language, name, query): (Language, &str, String) = match lang_id {
        "rust" => (
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(),
        ),
        "javascript" => (
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            // 常规 + JSX 高亮合并，粘贴 JSX 也能正确着色
            format!(
                "{}{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ),
        ),
        "typescript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
            tree_sitter_typescript::HIGHLIGHTS_QUERY.to_string(),
        ),
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            "tsx",
            tree_sitter_typescript::HIGHLIGHTS_QUERY.to_string(),
        ),
        "python" => (
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY.to_string(),
        ),
        "json" => (
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY.to_string(),
        ),
        "css" => (
            tree_sitter_css::LANGUAGE.into(),
            "css",
            tree_sitter_css::HIGHLIGHTS_QUERY.to_string(),
        ),
        "html" => (
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY.to_string(),
        ),
        "go" => (
            tree_sitter_go::LANGUAGE.into(),
            "go",
            tree_sitter_go::HIGHLIGHTS_QUERY.to_string(),
        ),
        "java" => (
            tree_sitter_java::LANGUAGE.into(),
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY.to_string(),
        ),
        "c" => (
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY.to_string(),
        ),
        "cpp" => (
            tree_sitter_cpp::LANGUAGE.into(),
            "cpp",
            tree_sitter_cpp::HIGHLIGHT_QUERY.to_string(),
        ),
        "yaml" => (
            tree_sitter_yaml::LANGUAGE.into(),
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY.to_string(),
        ),
        _ => return None,
    };
    let mut config = HighlightConfiguration::new(language, name, &query, "", "").ok()?;
    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}

/// 一行内的片段。
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffSegmentDto {
    pub text: String,
    /// 语法 token 类型（keyword/string/comment/…），None = 默认色
    pub token: Option<&'static str>,
    /// 词级差异标记（加深背景）
    pub changed: bool,
}

/// 一行 diff。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffRowDto {
    /// equal / delete / insert
    pub kind: &'static str,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub segments: Vec<DiffSegmentDto>,
}

/// 对比结果。
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextDiffResultDto {
    /// 实际启用高亮的语言（纯文本/未知语言为 None）
    pub language: Option<String>,
    pub rows: Vec<DiffRowDto>,
    pub added: u32,
    pub removed: u32,
}

/// 按 \n 切行并去掉行尾 \r（与 tree-sitter 字节偏移解耦的规范化行）。
/// 空文本返回零行（避免对比 "" vs "x" 时出现幽灵空行的 delete）。
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// 计算全文中每个高亮片段的字节区间 (start, end, token)；失败/无语言返回空。
fn highlight_spans(
    config: &HighlightConfiguration,
    text: &str,
) -> Vec<(usize, usize, Option<&'static str>)> {
    let mut spans = Vec::new();
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(config, text.as_bytes(), None, |_| None) else {
        return spans;
    };
    let mut stack: Vec<Option<&'static str>> = Vec::new();
    for event in events.flatten() {
        match event {
            HighlightEvent::HighlightStart(h) => {
                stack.push(HIGHLIGHT_NAMES.get(h.0).and_then(|n| token_kind(n)));
            }
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                // 嵌套时取最内层 capture
                spans.push((start, end, stack.last().copied().flatten()));
            }
        }
    }
    spans
}

/// 把全文字节区间切片到逐行（行内字节偏移），跨行片段按行边界切开。
/// line_starts 由原始文本（未去 \r）计算，保证与 tree-sitter 偏移一致。
fn spans_to_lines(
    text: &str,
    line_count: usize,
    spans: Vec<(usize, usize, Option<&'static str>)>,
) -> Vec<Vec<(usize, usize, Option<&'static str>)>> {
    let mut line_starts: Vec<usize> = Vec::with_capacity(line_count);
    let mut offset = 0usize;
    for piece in text.split('\n') {
        line_starts.push(offset);
        offset += piece.len() + 1;
    }
    // 规范化行的长度上限（去 \r 后），用于裁掉覆盖到 \r 的片段尾巴
    let stripped_lens: Vec<usize> = split_lines(text).iter().map(|l| l.len()).collect();

    let mut per_line = vec![Vec::<(usize, usize, Option<&'static str>)>::new(); line_count];
    for (start, end, kind) in spans {
        if end <= start || per_line.is_empty() {
            continue;
        }
        // 二分定位起始行
        let mut li = line_starts.partition_point(|&ls| ls <= start) - 1;
        let (mut s, e) = (start, end);
        while li < line_starts.len() && s < e {
            let line_start = line_starts[li];
            // 行尾（不含 \n）：下一行起点减 1；末行取全文末尾。
            // 直接用预计算的 line_starts，避免每个 span 重新 find('\n') 扫描整行。
            let raw_end = if li + 1 < line_starts.len() {
                line_starts[li + 1] - 1
            } else {
                text.len()
            };
            let seg_end = e.min(raw_end);
            if seg_end > s {
                let local_end = (seg_end - line_start).min(stripped_lens[li]);
                let local_start = s - line_start;
                if local_end > local_start {
                    per_line[li].push((local_start, local_end, kind));
                }
            }
            s = raw_end + 1;
            li += 1;
        }
    }
    per_line
}

/// 词级 diff：返回两行各自「发生变化」的字节区间（用于行内加深标记）。
fn changed_ranges(old_line: &str, new_line: &str) -> (ByteRanges, ByteRanges) {
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    let diff = TextDiff::from_unicode_words(old_line, new_line);
    let mut op = 0usize;
    let mut np = 0usize;
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            ChangeTag::Equal => {
                op += len;
                np += len;
            }
            ChangeTag::Delete => {
                old_ranges.push((op, op + len));
                op += len;
            }
            ChangeTag::Insert => {
                new_ranges.push((np, np + len));
                np += len;
            }
        }
    }
    (old_ranges, new_ranges)
}

/// 一行内的字节区间集合（起止均含）。
type ByteRanges = Vec<(usize, usize)>;

/// 词级 diff 的单行长度上限：超长行（如压缩产物）词级比对会退化为
/// 近平方复杂度，直接整行标记变化以保证响应。
const MAX_WORD_DIFF_LINE: usize = 32 * 1024;

/// 带长度保护的词级 diff。
fn line_changed_ranges(old_line: &str, new_line: &str) -> (ByteRanges, ByteRanges) {
    if old_line.len() > MAX_WORD_DIFF_LINE || new_line.len() > MAX_WORD_DIFF_LINE {
        return (
            vec![(0, old_line.len())],
            vec![(0, new_line.len())],
        );
    }
    changed_ranges(old_line, new_line)
}

/// 合并「语法 token 切分」与「词级变化切分」为最终片段序列。
fn build_segments(
    line: &str,
    tokens: &[(usize, usize, Option<&'static str>)],
    changed: &[(usize, usize)],
) -> Vec<DiffSegmentDto> {
    if line.is_empty() {
        return vec![DiffSegmentDto {
            text: String::new(),
            token: None,
            changed: false,
        }];
    }
    // 边界集合：token 边界 ∪ 变化边界 ∪ 行首尾
    let mut bounds: Vec<usize> = vec![0, line.len()];
    for (s, e, _) in tokens {
        bounds.push(*s.min(&line.len()));
        bounds.push(*e.min(&line.len()));
    }
    for (s, e) in changed {
        bounds.push(*s.min(&line.len()));
        bounds.push(*e.min(&line.len()));
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut segments = Vec::new();
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b <= a {
            continue;
        }
        let token = tokens
            .iter()
            .find(|(s, e, _)| *s <= a && b <= *e)
            .and_then(|(_, _, k)| *k);
        let is_changed = changed.iter().any(|(s, e)| *s < b && a < *e);
        segments.push(DiffSegmentDto {
            text: line[a..b].to_string(),
            token,
            changed: is_changed,
        });
    }
    segments
}

/// 对比核心（与 Tauri 解耦，便于单测）。
pub fn compute_text_diff(left: &str, right: &str, language: Option<&str>) -> TextDiffResultDto {
    let lang_id = language
        .filter(|l| !l.is_empty() && *l != "plain")
        .map(str::to_string);
    let config = lang_id.as_deref().and_then(make_language_config);

    let old_lines = split_lines(left);
    let new_lines = split_lines(right);

    // 两份文本各自的高亮片段（无语言时空）
    let (tokens_old, tokens_new) = match &config {
        Some(cfg) => {
            let spans_old = highlight_spans(cfg, left);
            let spans_new = highlight_spans(cfg, right);
            (
                spans_to_lines(left, old_lines.len(), spans_old),
                spans_to_lines(right, new_lines.len(), spans_new),
            )
        }
        None => (
            vec![Vec::new(); old_lines.len()],
            vec![Vec::new(); new_lines.len()],
        ),
    };

    let old_refs: Vec<&str> = old_lines.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new_lines.iter().map(String::as_str).collect();
    let diff = TextDiff::from_slices(&old_refs, &new_refs);

    let mut rows: Vec<DiffRowDto> = Vec::new();
    let mut added = 0u32;
    let mut removed = 0u32;

    // 相邻 delete 组与 insert 组逐行配对做词级 diff（GitHub 式行内高亮）
    let changes: Vec<(ChangeTag, Option<usize>, Option<usize>, &str)> = diff
        .iter_all_changes()
        .map(|c| (c.tag(), c.old_index(), c.new_index(), c.value()))
        .collect();
    let mut i = 0usize;
    while i < changes.len() {
        let (tag, oi, ni, value) = changes[i];
        match tag {
            ChangeTag::Equal => {
                rows.push(DiffRowDto {
                    kind: "equal",
                    old_lineno: oi.map(|v| v as u32 + 1),
                    new_lineno: ni.map(|v| v as u32 + 1),
                    segments: build_segments(value, &tokens_old[oi.unwrap_or(0)], &[]),
                });
                i += 1;
            }
            ChangeTag::Delete | ChangeTag::Insert => {
                let dels: Vec<usize> = (i..changes.len())
                    .take_while(|&k| changes[k].0 == ChangeTag::Delete)
                    .collect();
                let ins_start = i + dels.len();
                let ins: Vec<usize> = (ins_start..changes.len())
                    .take_while(|&k| changes[k].0 == ChangeTag::Insert)
                    .collect();

                for k in 0..dels.len().max(ins.len()) {
                    if let Some(&di) = dels.get(k) {
                        let (_, oi, _, value) = changes[di];
                        let old_changed = match ins.get(k) {
                            Some(&ii) => line_changed_ranges(value, changes[ii].3).0,
                            // 无配对：整行标记变化
                            None => vec![(0, value.len())],
                        };
                        rows.push(DiffRowDto {
                            kind: "delete",
                            old_lineno: oi.map(|v| v as u32 + 1),
                            new_lineno: None,
                            segments: build_segments(
                                value,
                                &tokens_old[oi.unwrap_or(0)],
                                &old_changed,
                            ),
                        });
                        removed += 1;
                    }
                    if let Some(&ii) = ins.get(k) {
                        let (_, _, ni, value) = changes[ii];
                        let new_changed = match dels.get(k) {
                            Some(&di) => line_changed_ranges(changes[di].3, value).1,
                            None => vec![(0, value.len())],
                        };
                        rows.push(DiffRowDto {
                            kind: "insert",
                            old_lineno: None,
                            new_lineno: ni.map(|v| v as u32 + 1),
                            segments: build_segments(
                                value,
                                &tokens_new[ni.unwrap_or(0)],
                                &new_changed,
                            ),
                        });
                        added += 1;
                    }
                }
                i += dels.len() + ins.len();
            }
        }
    }

    TextDiffResultDto {
        language: if config.is_some() { lang_id } else { None },
        rows,
        added,
        removed,
    }
}

/// 单侧输入上限（字节）：防止异常粘贴导致词级 diff/解析耗时失控。
const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;

/// 文本对比命令：language 为 null/"plain"/未知值时按纯文本处理。
/// 计算是 CPU 密集型，放入 spawn_blocking 避免占用 async runtime。
#[tauri::command]
pub async fn text_diff(
    left: String,
    right: String,
    language: Option<String>,
) -> Result<TextDiffResultDto, String> {
    if left.len() > MAX_INPUT_BYTES || right.len() > MAX_INPUT_BYTES {
        return Err("单侧文本不能超过 2MB".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        compute_text_diff(&left, &right, language.as_deref())
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_line_diff() {
        let result = compute_text_diff("a\nb\nc", "a\nx\nc", None);
        assert_eq!(result.rows.len(), 4);
        assert_eq!(result.rows[0].kind, "equal");
        assert_eq!(result.rows[1].kind, "delete");
        assert_eq!(result.rows[2].kind, "insert");
        assert_eq!(result.rows[1].old_lineno, Some(2));
        assert_eq!(result.rows[2].new_lineno, Some(2));
        assert_eq!(result.added, 1);
        assert_eq!(result.removed, 1);
    }

    #[test]
    fn identical_inputs() {
        let result = compute_text_diff("one\ntwo", "one\ntwo", None);
        assert!(result.rows.iter().all(|r| r.kind == "equal"));
        assert_eq!(result.added, 0);
        assert_eq!(result.removed, 0);
    }

    #[test]
    fn word_level_changed_marks_only_difference() {
        let result = compute_text_diff("count = 1", "count = 2", None);
        assert_eq!(result.rows.len(), 2);
        let del = &result.rows[0];
        let ins = &result.rows[1];
        // “count = ” 部分不应标记 changed，仅数字部分标记
        let unchanged: String = del
            .segments
            .iter()
            .filter(|s| !s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(unchanged, "count = ");
        let changed: String = del
            .segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed, "1");
        let changed_ins: String = ins
            .segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed_ins, "2");
    }

    #[test]
    fn unpaired_line_marks_whole_line() {
        let result = compute_text_diff("a\nb", "a", None);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[1].kind, "delete");
        assert!(result.rows[1].segments.iter().all(|s| s.changed));
    }

    #[test]
    fn rust_keyword_highlighted() {
        let result = compute_text_diff("fn main() {}", "fn main() {}", Some("rust"));
        assert_eq!(result.language.as_deref(), Some("rust"));
        let row = &result.rows[0];
        let fn_seg = row
            .segments
            .iter()
            .find(|s| s.text == "fn")
            .expect("fn segment");
        assert_eq!(fn_seg.token, Some("keyword"));
        let main_seg = row
            .segments
            .iter()
            .find(|s| s.text == "main")
            .expect("main segment");
        assert_eq!(main_seg.token, Some("function"));
    }

    #[test]
    fn string_and_comment_tokens() {
        let result = compute_text_diff(
            "// note\nlet s = \"hi\";",
            "// note\nlet s = \"ho\";",
            Some("rust"),
        );
        let comment_row = &result.rows[0];
        assert!(comment_row
            .segments
            .iter()
            .any(|s| s.token == Some("comment")));
        // 第二行是 delete/insert 对，字符串字面量应有 string token
        let del_row = &result.rows[1];
        assert!(del_row.segments.iter().any(|s| s.token == Some("string")));
        // 词级：仅引号内内容标记 changed
        let changed: String = del_row
            .segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed, "hi");
    }

    #[test]
    fn unknown_language_falls_back_to_plain() {
        let result = compute_text_diff("x", "y", Some("nonsense"));
        assert_eq!(result.language, None);
        assert!(result
            .rows
            .iter()
            .all(|r| r.segments.iter().all(|s| s.token.is_none())));
    }

    #[test]
    fn empty_vs_content() {
        let result = compute_text_diff("", "hello", None);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].kind, "insert");
        assert_eq!(result.added, 1);
        let text: String = result.rows[0]
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(text, "hello");
    }

    #[test]
    fn multiline_string_token_spans_lines() {
        let result = compute_text_diff(
            "let s = \"a\nb\";",
            "let s = \"a\nb\";",
            Some("rust"),
        );
        // 跨行字符串：两行都应带 string token
        assert!(result.rows[0].segments.iter().any(|s| s.token == Some("string")));
        assert!(result.rows[1].segments.iter().any(|s| s.token == Some("string")));
    }

    #[test]
    fn crlf_lines_normalized() {
        let result = compute_text_diff("a\r\nb", "a\r\nb", None);
        assert!(result.rows.iter().all(|r| r.kind == "equal"));
        let text: String = result.rows[1]
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(text, "b");
    }

    #[test]
    fn oversize_line_skips_word_diff_and_marks_whole_line() {
        // 超过 MAX_WORD_DIFF_LINE 的行不做词级 diff，整行标记变化（保证响应）
        let old_line = "x".repeat(MAX_WORD_DIFF_LINE + 1);
        let new_line = format!("{}suffix", "x".repeat(MAX_WORD_DIFF_LINE + 1));
        let result = compute_text_diff(&old_line, &new_line, None);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].kind, "delete");
        assert_eq!(result.rows[1].kind, "insert");
        // 降级后整行所有片段都应标记 changed
        assert!(result.rows[0].segments.iter().all(|s| s.changed));
        let changed_text: String = result.rows[1]
            .segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed_text, new_line);
    }
}
