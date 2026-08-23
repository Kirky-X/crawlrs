// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information

//! DomSmoothie 正文提取器（design.md §11，T047/R-content-002、R-content-003）
//!
//! [`DomSmoothieExtractor`] 是基于 [`dom_smoothie`] crate（readability.js 移植）的性能回退实现，
//! DOM 启发式提取，比 Trafilatura 略低质量但速度更快。
//!
//! 特性门控：本模块整体 gated `dom-smoothie`（T043 声明的 `dep:dom_smoothie`）。
//! 特性未启用时由 [`super::facade::ContentExtractionFacade`] 跳过本实现。
//!
//! 提取策略：
//! 1. 调用 `dom_smoothie::Readability::new(html, Some(url), cfg).parse()` 提取文章
//! 2. `article.title` 作为标题（默认非空字符串）
//! 3. `article.byline` 作为作者
//! 4. confidence 由 `text_content` 长度启发式计算（dom_smoothie 不暴露内置 confidence）：
//!    - `< 100` 字符 → 0.6（极短内容，低可信度）
//!    - `100-1000` 字符 → 线性 0.6 → 0.75（介于 CssRule 兜底与高质量之间）
//!    - `> 1000` 字符 → 0.85（性能回退实现的最高可信度）
//! 5. page_type 统一返回 `Unknown`（dom_smoothie 不分类页面类型）

use dom_smoothie::{Article, Readability};

use super::traits::{ContentExtractor, ExtractError, ExtractedContent, PageType};

/// 基于 `dom_smoothie`（readability.js 移植）的正文提取器
///
/// 无状态，可安全共享单例。建议作为 [`super::facade::ContentExtractionFacade`] 的次选实现
/// （介于 Trafilatura 与 CssRule 之间）。
#[derive(Debug, Clone, Default)]
pub struct DomSmoothieExtractor;

impl DomSmoothieExtractor {
    /// 创建新实例（无状态）
    pub fn new() -> Self {
        Self
    }

    /// 根据 text_content 长度计算启发式 confidence（0.6-0.85 区间）
    ///
    /// 设计取舍：dom_smoothie 未暴露内置 confidence 字段，使用文本长度作为代理指标。
    /// 0.6 → 0.75 → 0.85 区间介于 CssRule（0.5）兜底与 Trafilatura 高质量提取（0.7-0.95）之间，
    /// 保证在 Facade 优先级链中 Trafilatura > DomSmoothie > CssRule 不会被反向触发。
    fn calculate_confidence(text_len: usize) -> f32 {
        if text_len < 100 {
            0.6
        } else if text_len < 1000 {
            // 线性插值 0.6 → 0.75
            0.6 + 0.15 * ((text_len - 100) as f32 / 900.0)
        } else {
            0.85
        }
    }
}

impl ContentExtractor for DomSmoothieExtractor {
    fn extract(&self, html: &str, url: &str) -> Result<ExtractedContent, ExtractError> {
        let trimmed_html = html.trim();
        if trimmed_html.is_empty() {
            return Err(ExtractError::NoContent);
        }

        // 构造 Readability（默认 Config，传入 url 用于相对链接解析）
        let mut readability = Readability::new(trimmed_html, Some(url), None)
            .map_err(|e| ExtractError::ExtractorFailed(format!("dom-smoothie: {e}")))?;

        // 提取 Article
        let article: Article = readability
            .parse()
            .map_err(|e| ExtractError::ExtractorFailed(format!("dom-smoothie parse: {e}")))?;

        let text = article.text_content.to_string();
        let trimmed_text = text.trim().to_string();
        if trimmed_text.is_empty() {
            return Err(ExtractError::NoContent);
        }

        let confidence = Self::calculate_confidence(trimmed_text.len());

        Ok(ExtractedContent {
            text: trimmed_text,
            title: Some(article.title),
            author: article.byline,
            confidence,
            page_type: PageType::Unknown,
        })
    }

    fn name(&self) -> &'static str {
        "dom-smoothie"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准 HTML 文章应能提取出正文文本
    #[test]
    fn extracts_main_content_from_simple_article() {
        let html = r#"<html><head><title>Test Article</title>
            <meta name="author" content="Kirky.X"></head>
            <body>
                <nav>Home About</nav>
                <article><p>This is the main content of the article.</p>
                <p>Second paragraph with more text.</p></article>
                <footer>Copyright 2025</footer>
            </body></html>"#;
        let result = DomSmoothieExtractor::new()
            .extract(html, "https://example.com/article")
            .expect("extract ok");

        assert!(!result.text.is_empty(), "text should not be empty");
    }

    /// 空 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_empty_html() {
        let result = DomSmoothieExtractor::new().extract("", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// 仅空白的 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_whitespace_only_html() {
        let result = DomSmoothieExtractor::new().extract("   \n\t  ", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// confidence 应在 [0.6, 0.85] 范围内
    #[test]
    fn confidence_is_within_valid_range() {
        let html = r#"<html><head><title>Article</title></head>
            <body><article><p>Body content with enough text to be considered as main content.
            More text here to ensure it passes length thresholds.</p></article></body></html>"#;
        let result = DomSmoothieExtractor::new()
            .extract(html, "https://example.com/a")
            .expect("extract ok");
        assert!(
            result.confidence >= 0.6 && result.confidence <= 0.85,
            "confidence should be in [0.6, 0.85], got: {}",
            result.confidence
        );
    }

    /// extractor 名称
    #[test]
    fn name_returns_dom_smoothie() {
        assert_eq!(DomSmoothieExtractor::new().name(), "dom-smoothie");
    }

    /// Default 实现应等价于 new
    #[test]
    fn default_equals_new() {
        let a = DomSmoothieExtractor::new();
        let b = DomSmoothieExtractor;
        let html = r#"<html><body><article><p>some content</p></article></body></html>"#;
        let ra = a
            .extract(html, "https://example.com/")
            .expect("a extract ok");
        let rb = b
            .extract(html, "https://example.com/")
            .expect("b extract ok");
        assert_eq!(ra.text, rb.text);
        assert_eq!(ra.confidence, rb.confidence);
    }

    /// trait 对象应可正常 dispatch
    #[test]
    fn trait_object_dispatch_works() {
        let extractor: Box<dyn ContentExtractor> = Box::new(DomSmoothieExtractor::new());
        let html = r#"<html><body><article><p>via trait</p></article></body></html>"#;
        let result = extractor
            .extract(html, "https://example.com/")
            .expect("trait dispatch ok");
        assert!(!result.text.is_empty());
        assert_eq!(extractor.name(), "dom-smoothie");
    }

    /// calculate_confidence 应按区间映射（0.6-0.85）
    #[test]
    fn calculate_confidence_uses_length_thresholds() {
        // 极短内容 → 0.6
        assert!((DomSmoothieExtractor::calculate_confidence(0) - 0.6).abs() < f32::EPSILON);
        assert!((DomSmoothieExtractor::calculate_confidence(99) - 0.6).abs() < f32::EPSILON);

        // 100-1000 区间线性插值 0.6 → 0.75
        let c100 = DomSmoothieExtractor::calculate_confidence(100);
        let c500 = DomSmoothieExtractor::calculate_confidence(500);
        let c999 = DomSmoothieExtractor::calculate_confidence(999);
        assert!(
            (c100 - 0.6).abs() < 0.01,
            "100 chars should be ~0.6, got: {c100}"
        );
        assert!(
            c500 > c100 && c999 > c500,
            "confidence should increase with text length"
        );

        // >=1000 饱和 → 0.85
        let c1000 = DomSmoothieExtractor::calculate_confidence(1000);
        let c10000 = DomSmoothieExtractor::calculate_confidence(10000);
        assert!((c1000 - 0.85).abs() < f32::EPSILON, "got: {c1000}");
        assert!((c10000 - 0.85).abs() < f32::EPSILON);
    }

    /// calculate_confidence 中间区间边界检查
    #[test]
    fn calculate_confidence_at_500_is_between_0_6_and_0_75() {
        let c = DomSmoothieExtractor::calculate_confidence(500);
        assert!(
            c > 0.6 && c < 0.75,
            "500 chars should be between 0.6 and 0.75, got: {c}"
        );
    }

    /// 超长 HTML 应能正常提取（不 panic）
    #[test]
    fn handles_very_long_html() {
        let mut body = String::from("<html><body><article>");
        for _ in 0..1_000 {
            body.push_str("<p>paragraph content here. </p>");
        }
        body.push_str("</article></body></html>");
        let result = DomSmoothieExtractor::new()
            .extract(&body, "https://example.com/long")
            .expect("extract ok");
        assert!(!result.text.is_empty());
    }
}
