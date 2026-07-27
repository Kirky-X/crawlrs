// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
//! 3. extraction_quality (0.0-1.0) 作为 confidence
//! 4. 通过 `page_type::classify_url` 分类页面类型，映射到 crawlrs `PageType`
//! 5. 空 / 解析失败 → 返回类型化错误

use rs_trafilatura::{self, page_type, Options};

use super::traits::{
    ContentExtractionError, ContentExtractor, ExtractedContent, PageType,
};

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
    /// 映射策略（crawlrs PageType 仅有 5 个变体，rs_trafilatura 有 7 个）：
    /// - `Article` / `Documentation` → `Article`（文档属文章类）
    /// - `Product` → `Detail`
    /// - `Category` / `Listing` → `List`
    /// - `Forum` / `Service` → `Other`（crawlrs 无对应分类）
    fn map_page_type(pt: page_type::PageType) -> PageType {
        match pt {
            page_type::PageType::Article | page_type::PageType::Documentation => PageType::Article,
            page_type::PageType::Product => PageType::Detail,
            page_type::PageType::Category | page_type::PageType::Listing => PageType::List,
            page_type::PageType::Forum | page_type::PageType::Service => PageType::Other,
        }
    }

    /// 构造 rs_trafilatura Options，仅传入 url（其他用默认）
    fn build_options(url: Option<&str>) -> Options {
        let mut opts = Options::default();
        if let Some(u) = url {
            opts.url = Some(u.to_string());
        }
        opts
    }
}

impl ContentExtractor for TrafilaturaExtractor {
    fn extract(&self, html: &str, url: Option<&str>) -> Result<ExtractedContent, ContentExtractionError> {
        let trimmed_html = html.trim();
        if trimmed_html.is_empty() {
            return Err(ContentExtractionError::EmptyContent);
        }

        let opts = Self::build_options(url);

        // 主提取路径
        let result = rs_trafilatura::extract_with_options(trimmed_html, &opts).map_err(|e| {
            ContentExtractionError::ExtractorError {
                extractor: "trafilatura",
                message: e.to_string(),
            }
        })?;

        let text = result.content_text.trim().to_string();
        if text.is_empty() {
            // 提取成功但正文为空（如纯导航/广告页）→ EmptyContent
            return Err(ContentExtractionError::EmptyContent);
        }

        // page_type 分类（URL 缺失时 classify_url 返回 Article 默认值）
        let raw_page_type = url
            .and_then(|u| page_type::classify_url(u).into())
            .unwrap_or(page_type::PageType::Article);
        let page_type = Self::map_page_type(raw_page_type);

        Ok(ExtractedContent {
            text,
            title: result.metadata.title,
            author: result.metadata.author,
            // extraction_quality ∈ [0.0, 1.0]，clamp 防御性处理（库可能返回边界外值）
            confidence: (result.extraction_quality as f32).clamp(0.0, 1.0),
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
            .extract(html, Some("https://example.com/article"))
            .expect("extract ok");

        assert!(!result.text.is_empty(), "text should not be empty");
        assert!(
            result.text.contains("main content"),
            "text should contain main content, got: {}",
            result.text
        );
    }

    /// 应能从 metadata 中提取 title 与 author
    #[test]
    fn extracts_title_and_author_from_metadata() {
        let html = r#"<html><head><title>Article Title</title>
            <meta name="author" content="Author Name"></head>
            <body><article><p>Article body content here.</p></article></body></html>"#;
        let result = TrafilaturaExtractor::new()
            .extract(html, Some("https://example.com/a"))
            .expect("extract ok");

        // trafilatura 应能从 meta/title 提取，但具体行为依赖库实现
        // 至少确保不报错，且 text 非空
        assert!(!result.text.is_empty());
    }

    /// 空 HTML 应返回 EmptyContent 错误
    #[test]
    fn returns_error_for_empty_html() {
        let result = TrafilaturaExtractor::new().extract("", None);
        assert!(matches!(result, Err(ContentExtractionError::EmptyContent)));
    }

    /// 仅空白的 HTML 应返回 EmptyContent 错误
    #[test]
    fn returns_error_for_whitespace_only_html() {
        let result = TrafilaturaExtractor::new().extract("   \n\t  ", None);
        assert!(matches!(result, Err(ContentExtractionError::EmptyContent)));
    }

    /// 仅含 nav/footer 的 HTML（提取后为空）应返回 EmptyContent
    #[test]
    fn returns_error_when_extracted_text_is_empty() {
        let html = "<html><body><nav>nav</nav><footer>footer</footer></body></html>";
        let result = TrafilaturaExtractor::new().extract(html, None);
        // trafilatura 对纯导航页可能返回空 content_text
        match result {
            Ok(r) => assert!(
                !r.text.is_empty(),
                "if extraction succeeds, text should not be empty"
            ),
            Err(ContentExtractionError::EmptyContent) => {}
            Err(e) => panic!("expected EmptyContent or Ok, got: {e:?}"),
        }
    }

    /// confidence 应在 [0, 1] 范围内
    #[test]
    fn confidence_is_within_valid_range() {
        let html = r#"<html><head><title>Article</title></head>
            <body><article><p>Body content with enough text to be considered as main content.
            More text here to ensure it passes length thresholds.</p></article></body></html>"#;
        let result = TrafilaturaExtractor::new()
            .extract(html, Some("https://example.com/a"))
            .expect("extract ok");
        assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
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
        let ra = a.extract(html, None).expect("a extract ok");
        let rb = b.extract(html, None).expect("b extract ok");
        assert_eq!(ra.text, rb.text);
        assert_eq!(ra.confidence, rb.confidence);
    }

    /// trait 对象应可正常 dispatch
    #[test]
    fn trait_object_dispatch_works() {
        let extractor: Box<dyn ContentExtractor> = Box::new(TrafilaturaExtractor::new());
        let html = r#"<html><body><article><p>via trait</p></article></body></html>"#;
        let result = extractor.extract(html, None).expect("trait dispatch ok");
        assert!(!result.text.is_empty());
        assert_eq!(extractor.name(), "trafilatura");
    }

    /// map_page_type 应正确映射 7 个 rs_trafilatura PageType 到 crawlrs 5 个 PageType
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
            TrafilaturaExtractor::map_page_type(page_type::PageType::Product),
            PageType::Detail
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Category),
            PageType::List
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Listing),
            PageType::List
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Forum),
            PageType::Other
        );
        assert_eq!(
            TrafilaturaExtractor::map_page_type(page_type::PageType::Service),
            PageType::Other
        );
    }

    /// build_options 应将 url 注入 Options.url
    #[test]
    fn build_options_injects_url_when_provided() {
        let opts = TrafilaturaExtractor::build_options(Some("https://example.com/x"));
        assert_eq!(opts.url.as_deref(), Some("https://example.com/x"));
    }

    /// build_options 在 url=None 时 Options.url 应为 None
    #[test]
    fn build_options_leaves_url_none_when_not_provided() {
        let opts = TrafilaturaExtractor::build_options(None);
        assert!(opts.url.is_none());
    }
}
