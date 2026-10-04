// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 提取质量门（prose 硬门槛 + 词数门槛的度量与判定）
//!
//! 为 [`super::facade::ContentExtractionFacade`] 的降级重试阶梯提供确定性度量：
//! - [`ProseMetrics`]：对候选提取结果的 prose 质量度量（词数 / 句均词数 / 链接密度）
//! - [`evaluate`]：从提取文本 + 可选正文 HTML 计算度量
//! - [`is_hard_fail`]：defuddle 式 prose 硬门槛判定（句均词数过低 ‖ 链接密度过高）
//! - [`QualityGateConfig`]：门槛配置；[`QualityGateConfig::disabled`] 常量保持
//!   未接线调用点的既有行为零变化
//!
//! 度量口径说明：
//! - `word_count` 用空白切词——CJK 文本无空格会被低估，中文站建议调低
//!   `min_word_count`（见 README 提取能力节）
//! - 长度比较用字符数（`chars().count()`，对 CJK 公平），由 facade 裁决逻辑使用
//! - `link_density` 从候选的正文 HTML 测量；`html = None` 时记 0（视为通过）

/// 提取候选的 prose 质量度量
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProseMetrics {
    /// 空白切词计数（CJK 无空格会低估）
    pub word_count: usize,
    /// 句均词数（按句末标点 `。！？.!?` 切分后的均值；无句末标点时整段视为一句）
    pub sentence_avg_words: f64,
    /// 链接密度 = 链接文本词数 / 总词数（`html = None` 时为 0）
    pub link_density: f64,
}

/// 提取降级阶梯质量门配置
///
/// `enabled = false`（[`QualityGateConfig::disabled`]）时 facade 走既有语义：
/// 首个 `confidence >= 0.7` 即返、低置信取首个成功结果，所有门槛与裁决不生效。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityGateConfig {
    /// 质量门总开关（`false` = 既有行为零变化）
    pub enabled: bool,
    /// 短结果升级阈值：候选词数低于该值不满足快路径
    pub min_word_count: usize,
    /// 候选替换裁决：候选字符数 ≥ 当前 best 字符数 × 该比例才采纳
    pub replacement_length_ratio: f64,
    /// prose 硬门槛：句均词数下限
    pub prose_min_sentence_words: f64,
    /// prose 硬门槛：链接密度上限（超过视为导航/索引页噪声）
    pub prose_max_link_density: f64,
}

impl QualityGateConfig {
    /// 禁用质量门（既有行为零变化）
    ///
    /// `new()` 构造的 facade 与未接线调用点（scrape_worker / markdown_post_processor /
    /// agent_lib）使用此配置；bootstrap 显式接线 settings 后才启用新语义。
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            min_word_count: 0,
            replacement_length_ratio: 0.0,
            prose_min_sentence_words: 0.0,
            prose_max_link_density: 1.0,
        }
    }

    /// 启用质量门（bootstrap 从 `settings.content_extraction` 四键构造）
    pub const fn active(
        min_word_count: usize,
        replacement_length_ratio: f64,
        prose_min_sentence_words: f64,
        prose_max_link_density: f64,
    ) -> Self {
        Self {
            enabled: true,
            min_word_count,
            replacement_length_ratio,
            prose_min_sentence_words,
            prose_max_link_density,
        }
    }
}

/// 从提取文本 + 可选正文 HTML 计算 prose 度量
///
/// - `word_count`：`text` 空白切词计数
/// - `sentence_avg_words`：`text` 按句末标点切分的句均词数
/// - `link_density`：`html` 中 `<a>` 文本词数占比；`html = None` 时为 0
pub fn evaluate(text: &str, html: Option<&str>) -> ProseMetrics {
    ProseMetrics {
        word_count: word_count(text),
        sentence_avg_words: sentence_avg_words(text),
        link_density: link_density(html),
    }
}

/// prose 硬门槛判定：句均词数过低 ‖ 链接密度过高
///
/// 纯谓词（不含 `enabled` 分支——禁用语义由调用方在门槛入口处理）。
pub fn is_hard_fail(metrics: &ProseMetrics, cfg: &QualityGateConfig) -> bool {
    metrics.sentence_avg_words < cfg.prose_min_sentence_words
        || metrics.link_density > cfg.prose_max_link_density
}

/// 空白切词计数
fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// 句均词数：按句末标点切分，非空句计一句，均值 = 总词数 / 句数
///
/// 无句末标点的文本整段视为一句（avg = word_count）。
fn sentence_avg_words(text: &str) -> f64 {
    let mut sentences = 0usize;
    let mut total_words = 0usize;
    for piece in text.split(['。', '！', '？', '.', '!', '?']) {
        let words = piece.split_whitespace().count();
        if words > 0 {
            sentences += 1;
            total_words += words;
        }
    }
    if sentences == 0 {
        0.0
    } else {
        total_words as f64 / sentences as f64
    }
}

/// 链接密度 = `<a>` 文本词数 / 全文档词数；`html = None` 或全文档无词时为 0
fn link_density(html: Option<&str>) -> f64 {
    let Some(html) = html else {
        return 0.0;
    };
    let document = scraper::Html::parse_document(html);
    let total = document
        .root_element()
        .text()
        .map(|t| t.split_whitespace().count())
        .sum::<usize>();
    if total == 0 {
        return 0.0;
    }
    let link_words = scraper::Selector::parse("a")
        .ok()
        .map(|sel| {
            document
                .select(&sel)
                .map(|el| {
                    el.text()
                        .map(|t| t.split_whitespace().count())
                        .sum::<usize>()
                })
                .sum::<usize>()
        })
        .unwrap_or(0);
    link_words as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(n: usize) -> String {
        vec!["word"; n].join(" ")
    }

    // ===== evaluate =====

    /// word_count 按空白切词计数
    #[test]
    fn evaluate_counts_whitespace_separated_words() {
        let m = evaluate("one two three\nfour\tfive", None);
        assert_eq!(m.word_count, 5);
    }

    /// 连续空白不产生空词
    #[test]
    fn evaluate_ignores_repeated_whitespace() {
        let m = evaluate("  a   b  ", None);
        assert_eq!(m.word_count, 2);
    }

    /// 空文本词数为 0
    #[test]
    fn evaluate_empty_text_has_zero_word_count() {
        let m = evaluate("", None);
        assert_eq!(m.word_count, 0);
        assert_eq!(m.sentence_avg_words, 0.0);
        assert_eq!(m.link_density, 0.0);
    }

    /// sentence_avg_words 按句末标点切分求均值（中英文句末标点均支持）
    #[test]
    fn evaluate_sentence_avg_splits_on_terminators() {
        // 3 句：2 + 3 + 4 词 → avg = 3.0
        let m = evaluate("one two. three four five! 六 七 八 九？", None);
        assert!((m.sentence_avg_words - 3.0).abs() < f64::EPSILON);
    }

    /// 无句末标点的文本整段视为一句
    #[test]
    fn evaluate_text_without_terminators_is_one_sentence() {
        let m = evaluate(&words(6), None);
        assert!((m.sentence_avg_words - 6.0).abs() < f64::EPSILON);
        assert_eq!(m.word_count, 6);
    }

    /// 小数点（"3.5"）按切分口径会被切开——固定行为固化（defuddle 同款简化）
    #[test]
    fn evaluate_decimal_point_splits_sentence() {
        // "version 3.5 here" → "version 3" + "5 here" 两句，各 2 词
        let m = evaluate("version 3.5 here", None);
        assert!((m.sentence_avg_words - 2.0).abs() < f64::EPSILON);
    }

    /// link_density 从 HTML `<a>` 文本词数占比测量
    #[test]
    fn evaluate_link_density_measures_link_text_ratio() {
        let html = r#"<html><body>
            <p>plain prose words here</p>
            <a href="/x">link text words</a>
        </body></html>"#;
        let m = evaluate("ignored", Some(html));
        // 3 链接词 / 7 总词（根文本含链接词）
        assert!(
            (m.link_density - 3.0 / 7.0).abs() < 1e-9,
            "got {}",
            m.link_density
        );
    }

    /// html = None 时 link_density 记 0（视为通过）
    #[test]
    fn evaluate_link_density_zero_when_html_none() {
        let m = evaluate(&words(10), None);
        assert_eq!(m.link_density, 0.0);
    }

    /// 纯链接导航页 link_density 接近 1
    #[test]
    fn evaluate_pure_nav_page_has_high_link_density() {
        let html = r#"<html><body>
            <a href="/a">home link one</a>
            <a href="/b">about link two</a>
            <a href="/c">contact link three</a>
        </body></html>"#;
        let m = evaluate("ignored", Some(html));
        assert!(m.link_density > 0.9, "got {}", m.link_density);
    }

    // ===== is_hard_fail =====

    fn cfg() -> QualityGateConfig {
        QualityGateConfig::active(150, 1.5, 7.0, 0.7)
    }

    /// 句均词数低于下限判 hard fail
    #[test]
    fn is_hard_fail_triggers_on_low_sentence_avg() {
        let m = evaluate("buy now. click here", None); // 2 句各 2 词 → avg 2 < 7
        assert!(is_hard_fail(&m, &cfg()));
    }

    /// 句均词数达到下限不判 hard fail（边界：等于阈值放行）
    #[test]
    fn is_hard_fail_passes_at_sentence_threshold_boundary() {
        let m = evaluate(&words(7), None); // 1 句 7 词 → avg = 7（等于下限）
        assert!(!is_hard_fail(&m, &cfg()));
    }

    /// 链接密度超上限判 hard fail
    #[test]
    fn is_hard_fail_triggers_on_high_link_density() {
        let html = r#"<html><body>
            <p>one</p>
            <a href="/a">link words here</a>
        </body></html>"#;
        let m = evaluate(&words(50), Some(html)); // 3 链接词 / 4 总词 = 0.75 > 0.7
        assert!(is_hard_fail(&m, &cfg()));
    }

    /// 链接密度等于上限不判 hard fail（边界：等于阈值放行）
    #[test]
    fn is_hard_fail_passes_at_link_density_boundary() {
        let html = r#"<html><body>
            <p>plain words</p>
            <a href="/a">link text</a>
        </body></html>"#; // 2/4 = 0.5 ≤ 0.7
        let m = evaluate(&words(50), Some(html));
        assert!(!is_hard_fail(&m, &cfg()));
    }

    /// 双门槛均通过时判 false
    #[test]
    fn is_hard_fail_false_for_prose_like_text() {
        let m = evaluate(&words(200), None); // 1 句 200 词，link_density 0
        assert!(!is_hard_fail(&m, &cfg()));
    }

    // ===== QualityGateConfig =====

    /// disabled() 配置 enabled=false 且阈值不影响判定入口（由调用方短路）
    #[test]
    fn disabled_config_has_enabled_false() {
        let c = QualityGateConfig::disabled();
        assert!(!c.enabled);
        assert_eq!(c.min_word_count, 0);
    }

    /// active() 配置按入参启用
    #[test]
    fn active_config_enables_gates_with_given_values() {
        let c = QualityGateConfig::active(150, 1.5, 7.0, 0.7);
        assert!(c.enabled);
        assert_eq!(c.min_word_count, 150);
        assert!((c.replacement_length_ratio - 1.5).abs() < f64::EPSILON);
        assert!((c.prose_min_sentence_words - 7.0).abs() < f64::EPSILON);
        assert!((c.prose_max_link_density - 0.7).abs() < f64::EPSILON);
    }

    /// disabled() 与 active() 的 PartialEq 可区分
    #[test]
    fn configs_compare_by_value() {
        assert_ne!(
            QualityGateConfig::disabled(),
            QualityGateConfig::active(150, 1.5, 7.0, 0.7)
        );
        assert_eq!(
            QualityGateConfig::active(150, 1.5, 7.0, 0.7),
            QualityGateConfig::active(150, 1.5, 7.0, 0.7)
        );
    }
}
