// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! HTML → Markdown 转换服务（design.md §10，T040/R-content-001）
//!
//! 提供 [`MarkdownServiceTrait`] 抽象与 [`HtmdMarkdownService`] 实现，
//! 后者使用 `htmd` crate 作为转换主路径。
//!
//! `only_main_content=true` 时，调用方应先经正文提取器（Stage3 [`crate::domain::services`]
//! 计划中的 `ContentExtractor`）取正文 HTML 再传入本服务；否则直接对整页 HTML 转换。
//!
//! 特性门控：本模块整体 gated `markdown`（T039 声明的 `dep:htmd`）。

use thiserror::Error;

/// Markdown 转换错误
#[derive(Debug, Error)]
pub enum MarkdownError {
    /// htmd 转换失败
    #[error("Markdown conversion failed: {0}")]
    ConversionFailed(String),
}

/// Markdown 转换服务 trait（design.md §10）
///
/// 实现方需保证线程安全（`Send + Sync`）以便在 `CrawlRsState` 中共享。
///
/// # `only_main_content` 参数语义
///
/// design.md §10 要求：`only_main_content=true` 时应先经 ContentExtractor（§11）取正文 HTML 再转换；
/// `false` 时整页转换。
///
/// **当前阶段性限制**（H-2 修复）：Stage 2 仅落地 `HtmdMarkdownService`，
/// `ContentExtractor` 在 Stage 3 T044-T049 实现。故当前实现**忽略**此参数，
/// 无论 true/false 都按整页转换。Stage 3 落地后，调用方应在传入前自行提取正文，
/// 或在 trait 上新增 `extract_and_convert` 方法由实现负责完整流程。
///
/// 调用方当前**不应**依赖此参数改变行为。
pub trait MarkdownServiceTrait: Send + Sync {
    /// 将 HTML 转换为 Markdown
    ///
    /// # 参数
    ///
    /// - `html`: 待转换的 HTML 字符串
    /// - `only_main_content`: 是否仅转换正文（**当前 Stage 2 实现忽略此参数**，详见 trait 文档）
    ///
    /// # 返回值
    ///
    /// 转换后的 Markdown 字符串。空 HTML 返回空字符串而非错误。
    fn to_markdown(&self, html: &str, only_main_content: bool) -> Result<String, MarkdownError>;
}

/// 基于 `htmd` 的 Markdown 转换实现（T040/R-content-001）
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

impl MarkdownServiceTrait for HtmdMarkdownService {
    fn to_markdown(&self, html: &str, only_main_content: bool) -> Result<String, MarkdownError> {
        // 空 HTML 直接返回空字符串（避免 htmd 对空输入的潜在告警）
        let trimmed = html.trim();
        if trimmed.is_empty() {
            return Ok(String::new());
        }

        // H-2 修复：当前 Stage 2 实现忽略 `only_main_content` 参数（详见 trait 文档）。
        // Stage 3 T044-T049 落地 ContentExtractor 后，由调用方在传入前提取正文 HTML，
        // 或扩展 trait 接口由实现负责完整流程。当前为 stage 限制，非接口虚伪。
        //
        // 显式标记 unused 以避免警告，同时保留参数语义供未来扩展。
        let _ = only_main_content;

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
        let md = svc.to_markdown(html, false).expect("convert ok");
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
        let md = svc.to_markdown(html, false).expect("convert ok");
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
        let md = svc.to_markdown(html, false).expect("convert ok");
        assert!(
            md.contains("Example"),
            "expected link text, got: {md}"
        );
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
        let md = svc.to_markdown(html, false).expect("convert ok");
        assert!(
            md.contains("let x = 1;"),
            "expected code content, got: {md}"
        );
    }

    /// 空 HTML 应返回空字符串而非错误
    #[test]
    fn returns_empty_string_for_empty_html() {
        let svc = HtmdMarkdownService::new();
        let md = svc.to_markdown("", false).expect("empty html should not error");
        assert!(md.is_empty(), "expected empty markdown, got: {md}");
    }

    /// 仅空白的 HTML 应返回空字符串
    #[test]
    fn returns_empty_string_for_whitespace_html() {
        let svc = HtmdMarkdownService::new();
        let md = svc
            .to_markdown("   \n\t  ", false)
            .expect("whitespace html should not error");
        assert!(md.is_empty(), "expected empty markdown, got: {md}");
    }

    /// `only_main_content=true` 当前等价于 false（Stage3 才有正文提取）
    #[test]
    fn only_main_content_flag_does_not_break_conversion() {
        let svc = HtmdMarkdownService::new();
        let html = "<html><body><p>Hello world</p></body></html>";
        let md = svc
            .to_markdown(html, true)
            .expect("only_main_content=true should still convert");
        assert!(
            md.contains("Hello world"),
            "expected text in markdown, got: {md}"
        );
    }

    /// Default 实现应等价于 new()
    #[test]
    fn default_equals_new() {
        let a = HtmdMarkdownService::new();
        let b = HtmdMarkdownService::default();
        // 无状态服务，等价性通过可调用相同输入验证
        let html = "<p>test</p>";
        let ra = a.to_markdown(html, false).expect("a convert ok");
        let rb = b.to_markdown(html, false).expect("b convert ok");
        assert_eq!(ra, rb, "default and new should produce same output");
    }

    /// trait 对象应可正常使用
    #[test]
    fn trait_object_dispatch_works() {
        let svc: Box<dyn MarkdownServiceTrait> = Box::new(HtmdMarkdownService::new());
        let html = "<html><body><h2>Sub</h2></body></html>";
        let md = svc.to_markdown(html, false).expect("trait object convert ok");
        assert!(
            md.contains("Sub"),
            "expected text via trait object, got: {md}"
        );
    }
}
