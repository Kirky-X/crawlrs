// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information

//! CSS 规则兜底提取器（design.md §11，T045/R-content-002）
//!
//! [`CssRuleExtractor`] 是无 feature 依赖的兜底实现，复用现有
//! [`ExtractionService::get_clean_text`] 完成正文清洗（spec constraint：不引入第三套清理实现）。
//!
//! 提取策略：
//! 1. 正文：调用 `get_clean_text`（已剔除 script/style/nav/footer/aside/head/iframe/noscript/form）
//! 2. 标题：`<title>` 标签文本（缺失则尝试 `<h1>`）
//! 3. 作者：`<meta name="author">` content 属性
//! 4. 置信度：固定 0.5（兜底，最低可信度，触发 LLM 回退）
//! 5. 页面类型：默认 `PageType::Unknown`（CSS 启发式不足以分类）
//!
//! 设计取舍：作为 fallback 兜底，质量最低。Trafilatura/DomSmoothie 启用后由 Facade 优先选择。
//! 三特性均关闭时 Facade 退化为本实现，保证编译通过且功能可用（R-content-003）。

use scraper::{Html, Selector};

use crate::domain::services::extraction_service::ExtractionService;

use super::traits::{ContentExtractor, ExtractError, ExtractedContent, PageType};

/// CSS 规则兜底提取器
///
/// 无状态，可安全共享单例。建议由 [`super::facade::ContentExtractionFacade`] 在
/// 高优先级实现不可用时回退使用。
#[derive(Debug, Clone, Default)]
pub struct CssRuleExtractor;

impl CssRuleExtractor {
    /// 创建新实例（无状态）
    pub fn new() -> Self {
        Self
    }

    /// 从 HTML 提取 `<title>` 文本，缺失时回退到首个 `<h1>` 文本
    fn extract_title(document: &Html) -> Option<String> {
        if let Ok(selector) = Selector::parse("title") {
            if let Some(element) = document.select(&selector).next() {
                let text = element
                    .text()
                    .collect::<Vec<_>>()
                    .join("")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }

        // 回退：<h1>
        if let Ok(selector) = Selector::parse("h1") {
            if let Some(element) = document.select(&selector).next() {
                let text = element
                    .text()
                    .collect::<Vec<_>>()
                    .join("")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    /// 从 HTML `<meta name="author">` 提取作者
    fn extract_author(document: &Html) -> Option<String> {
        let selector = Selector::parse(r#"meta[name="author"]"#).ok()?;
        let element = document.select(&selector).next()?;
        let content = element.value().attr("content")?;
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}

impl ContentExtractor for CssRuleExtractor {
    fn extract(&self, html: &str, _url: &str) -> Result<ExtractedContent, ExtractError> {
        // 短输入直接判定为空内容（避免 scraper 解析空文档返回空）
        let trimmed_html = html.trim();
        if trimmed_html.is_empty() {
            return Err(ExtractError::NoContent);
        }

        // 1. 复用 ExtractionService::get_clean_text 完成正文清洗（不引入第三套清理实现）
        let text = ExtractionService::get_clean_text(html);
        let text = text.trim();
        if text.is_empty() {
            return Err(ExtractError::NoContent);
        }

        // 2. 提取标题与作者（基于 scraper::Html 一次解析）
        let document = Html::parse_document(html);
        let title = Self::extract_title(&document);
        let author = Self::extract_author(&document);

        Ok(ExtractedContent {
            text: text.to_string(),
            title,
            author,
            // 固定 0.5：兜底实现，最低可信度，触发 Facade 的 LLM 回退
            confidence: 0.5,
            // CSS 启发式不足以分类，统一返回 Unknown
            page_type: PageType::Unknown,
        })
    }

    fn name(&self) -> &'static str {
        "css-rule"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准 HTML 文章应能提取出正文文本，且 nav/footer/aside 应被剔除
    #[test]
    fn extracts_main_content_and_strips_nav_footer_aside() {
        let html = r#"<html><head><title>Test Article</title>
            <meta name="author" content="Kirky.X"></head>
            <body>
                <nav>Home About</nav>
                <aside>Sidebar content</aside>
                <article><p>This is the main content of the article.</p>
                <p>Second paragraph with more text.</p></article>
                <footer>Copyright 2025</footer>
            </body></html>"#;
        let extractor = CssRuleExtractor::new();
        let result = extractor
            .extract(html, "https://example.com/a")
            .expect("extract ok");

        assert!(!result.text.is_empty(), "text should not be empty");
        assert!(
            result.text.contains("main content"),
            "text should contain main content, got: {}",
            result.text
        );
        assert!(
            result.text.contains("Second paragraph"),
            "text should contain second paragraph"
        );
        // nav/footer/aside 应被剔除
        assert!(
            !result.text.contains("Home About"),
            "nav should be stripped, got: {}",
            result.text
        );
        assert!(
            !result.text.contains("Copyright 2025"),
            "footer should be stripped"
        );
        assert!(
            !result.text.contains("Sidebar content"),
            "aside should be stripped"
        );
    }

    /// title 应优先从 `<title>` 提取
    #[test]
    fn extracts_title_from_title_tag() {
        let html = "<html><head><title>Page Title</title></head><body><p>content</p></body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert_eq!(result.title.as_deref(), Some("Page Title"));
    }

    /// `<title>` 缺失时应回退到 `<h1>`
    #[test]
    fn extracts_title_from_h1_when_title_missing() {
        let html = "<html><body><h1>Heading Title</h1><p>content</p></body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert_eq!(result.title.as_deref(), Some("Heading Title"));
    }

    /// 无 title 与 h1 时 title 字段应为 None
    #[test]
    fn returns_none_title_when_no_title_or_h1() {
        let html = "<html><body><p>just some content here</p></body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert!(result.title.is_none(), "title should be None");
    }

    /// `<meta name="author">` 应被解析
    #[test]
    fn extracts_author_from_meta_tag() {
        let html = r#"<html><head>
            <meta name="author" content="Author Name">
            </head><body><p>content</p></body></html>"#;
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert_eq!(result.author.as_deref(), Some("Author Name"));
    }

    /// 空 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_empty_html() {
        let result = CssRuleExtractor::new().extract("", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// 仅空白的 HTML 应返回 NoContent 错误
    #[test]
    fn returns_error_for_whitespace_only_html() {
        let result = CssRuleExtractor::new().extract("   \n\t  ", "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// 仅含 nav/footer/aside 的 HTML（清洗后为空）应返回 NoContent
    #[test]
    fn returns_error_when_clean_text_is_empty() {
        let html =
            "<html><body><nav>nav</nav><aside>aside</aside><footer>footer</footer></body></html>";
        let result = CssRuleExtractor::new().extract(html, "https://example.com/");
        assert!(matches!(result, Err(ExtractError::NoContent)));
    }

    /// 兜底 confidence 应固定为 0.5（触发 LLM 回退）
    #[test]
    fn confidence_is_fixed_at_0_5() {
        let html = "<html><body><p>some content</p></body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert!((result.confidence - 0.5).abs() < f32::EPSILON);
        assert!(
            result.should_fallback_to_llm(),
            "0.5 should trigger fallback"
        );
    }

    /// page_type 应为 Unknown
    #[test]
    fn page_type_is_unknown() {
        let html = "<html><body><p>some content</p></body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert_eq!(result.page_type, PageType::Unknown);
    }

    /// extractor 名称
    #[test]
    fn name_returns_css_rule() {
        assert_eq!(CssRuleExtractor::new().name(), "css-rule");
    }

    /// Default 实现应等价于 new
    #[test]
    fn default_equals_new() {
        let a = CssRuleExtractor::new();
        let b = CssRuleExtractor::default();
        let html = "<p>same content</p>";
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
        let extractor: Box<dyn ContentExtractor> = Box::new(CssRuleExtractor::new());
        let html = "<html><body><p>via trait</p></body></html>";
        let result = extractor
            .extract(html, "https://example.com/")
            .expect("trait dispatch ok");
        assert!(result.text.contains("via trait"));
        assert_eq!(extractor.name(), "css-rule");
    }

    /// 超长 HTML 应能正常提取（不 panic，不截断）
    #[test]
    fn handles_very_long_html() {
        let mut body = String::from("<html><body>");
        for _ in 0..10_000 {
            body.push_str("<p>paragraph content here. </p>");
        }
        body.push_str("</body></html>");
        let result = CssRuleExtractor::new()
            .extract(&body, "https://example.com/long")
            .expect("extract ok");
        assert!(!result.text.is_empty());
        assert!(result.text.contains("paragraph content"));
    }

    /// 损坏的 HTML 标签应能被 scraper 容错解析
    #[test]
    fn handles_malformed_html() {
        let html = "<html><body><p>unclosed paragraph <b>bold text</body></html>";
        let result = CssRuleExtractor::new()
            .extract(html, "https://example.com/")
            .expect("extract ok");
        assert!(result.text.contains("unclosed paragraph") || result.text.contains("bold text"));
    }
}
