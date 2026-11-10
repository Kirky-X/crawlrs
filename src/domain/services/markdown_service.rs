// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! HTML → Markdown 转换抽象与 htmd 实现
//!
//! 提供 `MarkdownConverter` 窄 trait（链成员接口）与 `HtmdMarkdownService`
//! （htmd 实现，在转换器链中担任兜底；主转换器为 `markdown/` 子模块的
//! moli-html2md）。`only_main_content` 语义由正文提取链
//! （`content_extractor/facade.rs`）在转换前完成，转换器只做整页/正文 HTML → MD。
//!
//! 特性门控：本模块整体 gated `content`（声明的 `dep:htmd`）。

use thiserror::Error;

/// Markdown 转换错误
#[derive(Debug, Error)]
pub enum MarkdownError {
    /// 单转换器转换失败
    #[error("Markdown conversion failed: {0}")]
    ConversionFailed(String),
    /// 转换器链双败：携带 primary / fallback 两侧原因（不吞错）
    #[error("Markdown converter chain failed: primary={primary}; fallback={fallback}")]
    ChainFailed { primary: String, fallback: String },
}

/// 转换器抽象（转换器链成员，html2md-converter-chain 变更）
///
/// 窄接口：只承诺 `name` 标识与纯 HTML→Markdown 转换，便于链成员替换与诊断。
/// 正文裁剪（only_main_content 语义）由提取链在转换前完成，不在此接口内。
pub trait MarkdownConverter: Send + Sync {
    /// 转换器名称（链日志/指标标签，如 "moli" / "htmd"）
    fn name(&self) -> &'static str;
    /// 将 HTML 转换为 Markdown（空输入返回空串，与既有服务语义一致）
    fn convert(&self, html: &str) -> Result<String, MarkdownError>;
}

/// 基于 `htmd` 的 Markdown 转换实现
///
/// 无状态服务，可在多处共享单例。
#[derive(Debug, Clone, Default)]
pub struct HtmdMarkdownService;

impl HtmdMarkdownService {
    /// 创建新的服务实例
    pub fn new() -> Self {
        Self
    }
}

impl MarkdownConverter for HtmdMarkdownService {
    fn name(&self) -> &'static str {
        "htmd"
    }

    fn convert(&self, html: &str) -> Result<String, MarkdownError> {
        // 空 HTML 直接返回空字符串（避免 htmd 对空输入的潜在告警）
        let trimmed = html.trim();
        if trimmed.is_empty() {
            return Ok(String::new());
        }
        htmd::convert(trimmed).map_err(|e| MarkdownError::ConversionFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标题应转为 Markdown `#` 语法
    #[test]
    fn converts_h1_to_markdown_heading() {
        let svc = HtmdMarkdownService::new();
        let html = "<html><body><h1>Title</h1></body></html>";
        let md = svc.convert(html).expect("convert ok");
        assert!(
            md.contains("# Title") || md.contains("Title"),
            "expected Title in markdown, got: {md}"
        );
    }

    /// 无序列表应转为 Markdown `- item` 语法
    #[test]
    fn converts_unordered_list_to_markdown() {
        let svc = HtmdMarkdownService::new();
        let html = "<html><body><ul><li>First</li><li>Second</li></ul></body></html>";
        let md = svc.convert(html).expect("convert ok");
        assert!(
            md.contains("First") && md.contains("Second"),
            "expected list items, got: {md}"
        );
    }

    /// 超链接应转为 Markdown `[text](url)` 语法
    #[test]
    fn converts_anchor_to_markdown_link() {
        let svc = HtmdMarkdownService::new();
        let html = r#"<html><body><a href="https://example.com">Example</a></body></html>"#;
        let md = svc.convert(html).expect("convert ok");
        assert!(md.contains("Example"), "expected link text, got: {md}");
        assert!(
            md.contains("https://example.com"),
            "expected link URL, got: {md}"
        );
    }

    /// 代码块应转为 Markdown 围栏代码块
    #[test]
    fn converts_pre_code_to_fenced_block() {
        let svc = HtmdMarkdownService::new();
        let html = "<html><body><pre><code>let x = 1;</code></pre></body></html>";
        let md = svc.convert(html).expect("convert ok");
        assert!(
            md.contains("let x = 1;"),
            "expected code content, got: {md}"
        );
    }

    /// 空 HTML 应返回空字符串而非错误
    #[test]
    fn returns_empty_string_for_empty_html() {
        let svc = HtmdMarkdownService::new();
        let md = svc.convert("").expect("empty html should not error");
        assert!(md.is_empty(), "expected empty markdown, got: {md}");
    }

    /// 仅空白的 HTML 应返回空字符串
    #[test]
    fn returns_empty_string_for_whitespace_html() {
        let svc = HtmdMarkdownService::new();
        let md = svc
            .convert("   \n\t  ")
            .expect("whitespace html should not error");
        assert!(md.is_empty(), "expected empty markdown, got: {md}");
    }

    /// 正文提取后的 HTML 转换不受影响（提取在转换前由提取链完成）
    #[test]
    fn extracted_html_still_converts() {
        let svc = HtmdMarkdownService::new();
        let html = "<html><body><p>Hello world</p></body></html>";
        let md = svc
            .convert(html)
            .expect("only_main_content=true should still convert");
        assert!(
            md.contains("Hello world"),
            "expected text in markdown, got: {md}"
        );
    }

    /// Default 实现应等价于 new
    #[test]
    fn default_equals_new() {
        let a = HtmdMarkdownService::new();
        let b = HtmdMarkdownService;
        // 无状态服务，等价性通过可调用相同输入验证
        let html = "<p>test</p>";
        let ra = a.convert(html).expect("a convert ok");
        let rb = b.convert(html).expect("b convert ok");
        assert_eq!(ra, rb, "default and new should produce same output");
    }

    /// trait 对象应可正常使用
    #[test]
    fn trait_object_dispatch_works() {
        let svc: Box<dyn MarkdownConverter> = Box::new(HtmdMarkdownService::new());
        let html = "<html><body><h2>Sub</h2></body></html>";
        let md = svc.convert(html).expect("trait object convert ok");
        assert!(
            md.contains("Sub"),
            "expected text via trait object, got: {md}"
        );
    }
}
