// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information

//! Trafilatura 正文提取器（design.md §11，T046/R-content-002、R-content-003）
//!
//! [`TrafilaturaExtractor`] 是基于 [`rs_trafilatura`] crate 的主路径实现，
//! 文章正文提取质量最高。
//!
//! 特性门控：本模块整体 gated `extractor-trafilatura`（T043 声明的 `dep:rs-trafilatura`）。
//! 特性未启用时由 [`super::facade::ContentExtractionFacade`] 跳过本实现回退到下一优先级。
//!
//! 提取策略：
//! 1. 调用 `rs_trafilatura::extract_with_options(html, opts)` 提取正文（传入 URL 用于
//!    相对链接解析与 page_type 分类）
//! 2. metadata.title / metadata.author 作为标题 / 作者
//! 3. confidence 根据提取文本长度与原文比例计算（0.7-0.95 区间）
//! 4. 通过 `page_type::classify_url` 分类页面类型，映射到 crawlrs `PageType`
//! 5. 空 / 解析失败 → 返回类型化错误

use rs_trafilatura::{self, page_type, Options};

use super::traits::{ContentExtractor, ExtractError, ExtractedContent, PageType};

/// 基于 `rs_trafilatura` 的正文提取器
///
/// 无状态，可安全共享单例。建议作为 [`super::facade::ContentExtractionFacade`] 的首选实现。
#[derive(Debug, Clone, Default)]
pub struct TrafilaturaExtractor;

impl TrafilaturaExtractor {
    /// 创建新实例（无状态）
    pub fn new() -> Self {
        Self
    }

    /// 将 rs_trafilatura 的 page_type 映射到 crawlrs `PageType`
    ///
    /// 映射策略（crawlrs PageType 仅有 4 个变体）：
    /// - `Article` / `Documentation` → `Article`（文档属文章类）
    /// - `Category` / `Listing` → `Listing`（列表/分类页）
    /// - `Product` / `Forum` / `Service` → `Unknown`（crawlrs 无对应分类）
    fn map_page_type(pt: page_type::PageType) -> PageType {
        match pt {
            page_type::PageType::Article | page_type::PageType::Documentation => PageType::Article,
            page_type::PageType::Category | page_type::PageType::Listing => PageType::Listing,
            page_type::PageType::Product
            | page_type::PageType::Forum
            | page_type::PageType::Service => PageType::Unknown,
        }
    }

    /// 构造 rs_trafilatura Options，仅传入 url（其他用默认）
    fn build_options(url: &str) -> Options {
        Options {
            url: Some(url.to_string()),
            ..Default::default()
        }
    }

    /// 根据提取文本长度与原文 HTML 长度的比例计算 confidence（0.7-0.95）
    ///
    /// 设计取舍：trafilatura 的 `extraction_quality` 字段在不同版本行为不一致，
    /// 改用文本长度比例作为更稳定的代理指标：
    /// - ratio = extracted_len / html_len（clamp [0, 1]）
    /// - confidence = 0.7 + ratio * 0.25 → 区间 [0.7, 0.95]
    /// - 比例越高（提取文本占原文比例越大）→ 置信度越高
    fn calculate_confidence(extracted_len: usize, html_len: usize) -> f32 {
        if html_len == 0 || extracted_len == 0 {
            return 0.7;
        }
        let ratio = (extracted_len as f32 / html_len as f32).clamp(0.0, 1.0);
        0.7 + ratio * 0.25
    }
}

impl ContentExtractor for TrafilaturaExtractor {
    fn extract(&self, html: &str, url: &str) -> Result<ExtractedContent, ExtractError> {
        let trimmed_html = html.trim();
        if trimmed_html.is_empty() {
            return Err(ExtractError::NoContent);
        }

        let opts = Self::build_options(url);

        // 主提取路径
        let result = rs_trafilatura::extract_with_options(trimmed_html, &opts)
            .map_err(|e| ExtractError::ExtractorFailed(format!("trafilatura: {e}")))?;

        let text = result.content_text.trim().to_string();
        if text.is_empty() {
            // 提取成功但正文为空（如纯导航/广告页）→ NoContent
            return Err(ExtractError::NoContent);
        }

        // page_type 分类（URL 缺失时 classify_url 返回 Article 默认值）
        let raw_page_type = page_type::classify_url(url);
        let page_type = Self::map_page_type(raw_page_type);

        // confidence 根据提取文本长度与原文 HTML 长度比例计算
        let confidence = Self::calculate_confidence(text.len(), trimmed_html.len());

        Ok(ExtractedContent {
            text,
            title: result.metadata.title,
            author: result.metadata.author,
            confidence,
            page_type,
        })
    }

    fn name(&self) -> &'static str {
        "trafilatura"
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
        let result = TrafilaturaExtractor::new()
            .extract(html, "https://example.com/article")
            .expect("extract ok");

        assert!(!result.text.is_empty(), "text should not be empty");
        assert!(
            result.text.contains("main content"),
            "text should contain main content, got: {}",
            result.text
        );
    }

    /// 空 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_empty_html() {
        let result = TrafilaturaExtractor::new().extract("", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// 仅空白的 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_whitespace_only_html() {
        let result = TrafilaturaExtractor::new().extract("   \n\t  ", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// confidence 应在 [0.7, 0.95] 范围内
    #[test]
    fn confidence_is_within_valid_range() {
        let html = r#"<html><head><title>Article</title></head>
            <body><article><p>Body content with enough text to be considered as main content.
            More text here to ensure it passes length thresholds.</p></article></body></html>"#;
        let result = TrafilaturaExtractor::new()
            .extract(html, "https://example.com/a")
            .expect("extract ok");
        assert!(
            result.confidence >= 0.7 && result.confidence <= 0.95,
            "confidence should be in [0.7, 0.95], got: {}",
            result.confidence
        );
    }

    /// extractor 名称
    #[test]
    fn name_returns_trafilatura() {
        assert_eq!(TrafilaturaExtractor::new().name(), "trafilatura");
    }

    /// Default 实现应等价于 new
    #[test]
    fn default_equals_new() {
        let a = TrafilaturaExtractor::new();
        let b = TrafilaturaExtractor::default();
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
        let extractor: Box<dyn ContentExtractor> = Box::new(TrafilaturaExtractor::new());
        let html = r#"<html><body><article><p>via trait</p></article></body></html>"#;
        let result = extractor
            .extract(html, "https://example.com/")
            .expect("trait dispatch ok");
        assert!(!result.text.is_empty());
        assert_eq!(extractor.name(), "trafilatura");
    }

    /// map_page_type 应正确映射 rs_trafilatura PageType 到 crawlrs PageType
    #[test]
    fn map_page_type_correctly_translates_all_variants() {
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Article),
            PageType::Article
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Documentation),
            PageType::Article
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Category),
            PageType::Listing
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Listing),
            PageType::Listing
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Product),
            PageType::Unknown
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Forum),
            PageType::Unknown
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Service),
            PageType::Unknown
        );
    }

    /// build_options 应将 url 注入 Options.url
    #[test]
    fn build_options_injects_url() {
        let opts = TrafilaturaExtractor::build_options("https://example.com/x");
        assert_eq!(opts.url.as_deref(), Some("https://example.com/x"));
    }

    /// calculate_confidence 应在 [0.7, 0.95] 区间
    #[test]
    fn calculate_confidence_in_valid_range() {
        // 空输入 → 默认 0.7
        assert!((TrafilaturaExtractor::calculate_confidence(0, 0) - 0.7).abs() < f32::EPSILON);
        assert!((TrafilaturaExtractor::calculate_confidence(10, 0) - 0.7).abs() < f32::EPSILON);

        // 高比例 → 接近 0.95
        let c = TrafilaturaExtractor::calculate_confidence(100, 100);
        assert!(
            (c - 0.95).abs() < f32::EPSILON,
            "ratio=1.0 → 0.95, got: {c}"
        );

        // 中等比例 → 中间值
        let c = TrafilaturaExtractor::calculate_confidence(50, 100);
        assert!(
            c > 0.7 && c < 0.95,
            "ratio=0.5 → between 0.7 and 0.95, got: {c}"
        );

        // 低比例 → 接近 0.7
        let c = TrafilaturaExtractor::calculate_confidence(1, 1000);
        assert!(c >= 0.7 && c < 0.75, "ratio=0.001 → near 0.7, got: {c}");
    }

    /// 超长 HTML 应能正常提取（不 panic）
    #[test]
    fn handles_very_long_html() {
        let mut body = String::from("<html><body><article>");
        for _ in 0..1_000 {
            body.push_str("<p>paragraph content here. </p>");
        }
        body.push_str("</article></body></html>");
        let result = TrafilaturaExtractor::new()
            .extract(&body, "https://example.com/long")
            .expect("extract ok");
        assert!(!result.text.is_empty());
    }
}
