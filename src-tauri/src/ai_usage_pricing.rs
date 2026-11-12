//! AI 模型价格表（USD / 每百万 token，成本估算用）。
//! 官方 API 定价快照，集中在一个常量表便于核对；按最长前缀匹配，
//! 支持日期后缀（claude-sonnet-4-5-20250929 命中 claude-sonnet-4-5）。
//! 查不到价格的模型一律返回 None——估算值绝不冒充真实成本。
//! 注意：这是 API 计费口径的估算，不是订阅费用。

/// 单个模型（家族）的单价：USD / 每百万 token。
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    /// 缓存命中（读）单价
    pub cache_read: f64,
    /// 缓存写入单价（OpenAI 系按输入计费，无独立写入价用 0）
    pub cache_write: f64,
}

const fn price(input: f64, output: f64, cache_read: f64, cache_write: f64) -> ModelPrice {
    ModelPrice { input, output, cache_read, cache_write }
}

/// 前缀 -> 价格。前缀越长优先级越高（gpt-5-mini 优先于 gpt-5）。
static PRICES: &[(&str, ModelPrice)] = &[
    // Anthropic Claude（写入为 5 分钟 TTL 缓存价）
    ("claude-opus-4", price(15.0, 75.0, 1.5, 18.75)),
    ("claude-sonnet-4", price(3.0, 15.0, 0.3, 3.75)),
    ("claude-haiku-4", price(1.0, 5.0, 0.1, 1.25)),
    ("claude-3-7-sonnet", price(3.0, 15.0, 0.3, 3.75)),
    ("claude-3-5-sonnet", price(3.0, 15.0, 0.3, 3.75)),
    ("claude-3-5-haiku", price(0.8, 4.0, 0.08, 1.0)),
    ("claude-3-opus", price(15.0, 75.0, 1.5, 18.75)),
    // OpenAI / Codex
    ("gpt-5-codex", price(1.25, 10.0, 0.125, 0.0)),
    ("gpt-5-mini", price(0.25, 2.0, 0.025, 0.0)),
    ("gpt-5-nano", price(0.05, 0.4, 0.005, 0.0)),
    ("gpt-5", price(1.25, 10.0, 0.125, 0.0)),
    ("gpt-4.1-mini", price(0.4, 1.6, 0.1, 0.0)),
    ("gpt-4.1-nano", price(0.1, 0.4, 0.025, 0.0)),
    ("gpt-4.1", price(2.0, 8.0, 0.5, 0.0)),
    ("gpt-4o-mini", price(0.15, 0.6, 0.075, 0.0)),
    ("gpt-4o", price(2.5, 10.0, 1.25, 0.0)),
    ("codex-mini", price(1.5, 6.0, 0.375, 0.0)),
    ("o4-mini", price(1.1, 4.4, 0.275, 0.0)),
    ("o3", price(2.0, 8.0, 0.5, 0.0)),
];

/// 模型名 -> 单价（最长前缀匹配）。未知模型返回 None。
pub fn price_for(model: &str) -> Option<&'static ModelPrice> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    PRICES
        .iter()
        .filter(|(prefix, _)| model.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, p)| p)
}

/// token 用量 -> 估算成本（USD，保留 6 位小数削浮点噪声）。未知模型返回 None。
pub fn cost_usd(
    model: &str,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
) -> Option<f64> {
    let p = price_for(model)?;
    let cost = (input as f64 * p.input
        + output as f64 * p.output
        + cache_read as f64 * p.cache_read
        + cache_write as f64 * p.cache_write)
        / 1_000_000.0;
    Some((cost * 1e6).round() / 1e6)
}

/// 真实存储成本 + 价格表估算混合（USD）：
/// - `est_*` 为无存储成本事件的 token 子集，按价格表估算
/// - 估算子集为零时直接返回存储成本（OpenCode 全量真实成本场景）
/// - 子集非零但模型无价时返回 None（无法算出完整成本，不冒充）
pub fn blended_cost(
    model: &str,
    stored_cost: f64,
    est_input: i64,
    est_output: i64,
    est_cache_read: i64,
    est_cache_write: i64,
) -> Option<f64> {
    let has_est_tokens =
        est_input > 0 || est_output > 0 || est_cache_read > 0 || est_cache_write > 0;
    if !has_est_tokens {
        return Some((stored_cost * 1e6).round() / 1e6);
    }
    let est = cost_usd(model, est_input, est_output, est_cache_read, est_cache_write)?;
    Some(((stored_cost + est) * 1e6).round() / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matches_with_date_suffix() {
        // 日期后缀命中家族前缀
        assert_eq!(price_for("claude-sonnet-4-5-20250929").unwrap().input, 3.0);
        assert_eq!(price_for("claude-opus-4-1-20250805").unwrap().output, 75.0);
        assert_eq!(price_for("gpt-5-codex").unwrap().input, 1.25);
    }

    #[test]
    fn longest_prefix_wins() {
        // gpt-5-mini 优先于 gpt-5
        assert_eq!(price_for("gpt-5-mini-2025").unwrap().output, 2.0);
        assert_eq!(price_for("gpt-5").unwrap().output, 10.0);
        assert_eq!(price_for("gpt-5-nano").unwrap().output, 0.4);
        // gpt-4o 与 gpt-4o-mini
        assert_eq!(price_for("gpt-4o-2024-11-20").unwrap().input, 2.5);
        assert_eq!(price_for("gpt-4o-mini-2024-07-18").unwrap().input, 0.15);
    }

    #[test]
    fn unknown_models_have_no_price() {
        assert!(price_for("").is_none());
        assert!(price_for("mystery-model-v9").is_none());
        assert!(cost_usd("mystery-model-v9", 1000, 1000, 0, 0).is_none());
    }

    #[test]
    fn cost_math() {
        // 1M input + 2M output 的 sonnet：3 + 30 = 33 美元
        let c = cost_usd("claude-sonnet-4-5", 1_000_000, 2_000_000, 0, 0).unwrap();
        assert!((c - 33.0).abs() < 1e-9);
        // 缓存读写：1M read + 1M write = 0.3 + 3.75
        let c = cost_usd("claude-sonnet-4-5", 0, 0, 1_000_000, 1_000_000).unwrap();
        assert!((c - 4.05).abs() < 1e-9);
        // 零用量
        assert_eq!(cost_usd("gpt-5", 0, 0, 0, 0).unwrap(), 0.0);
    }

    #[test]
    fn blended_cost_mixes_stored_and_estimate() {
        // 纯存储成本（OpenCode 全量真实成本）：est 子集为零，直接返回 stored
        assert_eq!(blended_cost("mimo-v2.5-free", 0.25, 0, 0, 0, 0), Some(0.25));
        assert_eq!(blended_cost("unknown-model", 1.5, 0, 0, 0, 0), Some(1.5));
        // 纯估算（无存储成本）：等价于 cost_usd
        assert_eq!(
            blended_cost("gpt-5", 0.0, 1_000_000, 0, 0, 0),
            cost_usd("gpt-5", 1_000_000, 0, 0, 0)
        );
        // 混合：stored + 已知模型估算
        let c = blended_cost("claude-sonnet-4-5", 0.5, 1_000_000, 1_000_000, 0, 0).unwrap();
        assert!((c - (0.5 + 3.0 + 15.0)).abs() < 1e-9);
        // 混合但未知模型且有估算 token：无法算完整成本
        assert_eq!(blended_cost("mimo-v2.5-free", 0.25, 100, 100, 0, 0), None);
    }
}
