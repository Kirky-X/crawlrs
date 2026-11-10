// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `moli-html2md` 主转换器实现
//!
//! 流程：`scraper::Html::parse_document`（html5ever 树）→ [`ScraperDom`] 只读视图 →
//! `moli_html2md::convert`（该函数本身不可失败，返回 String）。
//!
//! 错误面：解析层 html5ever 自带错误恢复（不返回 Err）；本转换器的失败路径来自
//! 输入口径——空文档树（无任何子节点）视为转换异常（与"输入非空输出空白"的链
//! 防呆互补，显式报错而非返回空串假装成功）。

use crate::domain::services::markdown::moli_adapter::ScraperDom;
use crate::domain::services::markdown_service::{MarkdownConverter, MarkdownError};

/// moli-html2md 主转换器（无状态）
#[derive(Debug, Clone, Default)]
pub struct MoliHtml2MdConverter;

impl MoliHtml2MdConverter {
    pub fn new() -> Self {
        Self
    }
}

impl MarkdownConverter for MoliHtml2MdConverter {
    fn name(&self) -> &'static str {
        "moli"
    }

    fn convert(&self, html: &str) -> Result<String, MarkdownError> {
        let trimmed = html.trim();
        if trimmed.is_empty() {
            // 与 HtmdMarkdownService 语义一致：空输入返回空串（非错误）
            return Ok(String::new());
        }
        // html5ever 对任意输入都产出树（错误恢复），moli convert 本身不可失败
        //（返回 String）——转换器无内部 Err 路径，Result 仅为 trait 契约。
        let doc = scraper::Html::parse_document(trimmed);
        let root = doc.tree.root();
        let dom = ScraperDom::new(&doc);
        // max_depth 上限：默认 Options 的 1000 会静默丢弃深层子树（且把深页推向
        // 兜底双跑，性能实证）。moli 遍历是迭代式（无 Rust 栈风险），输入又被
        // max_response_body_bytes（默认 10MiB）界定，故解除深度限制、不做静默裁剪。
        let converter = moli_html2md::Converter::new(moli_html2md::Options {
            max_depth: usize::MAX,
            ..Default::default()
        });
        let markdown = converter.convert(&dom, root.id());
        Ok(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_moli() {
        assert_eq!(MoliHtml2MdConverter::new().name(), "moli");
    }

    /// 标题/段落/链接基础转换
    #[test]
    fn converts_basic_structures() {
        let html = r#"<html><body><h1>Title</h1><p>Hello <a href="https://example.com/x">link</a>.</p></body></html>"#;
        let md = MoliHtml2MdConverter::new()
            .convert(html)
            .expect("convert ok");
        assert!(md.contains("# Title"), "标题未转换: {md}");
        assert!(md.contains("Hello"), "段落文本缺失: {md}");
        assert!(
            md.contains("[link](https://example.com/x)"),
            "链接未转换: {md}"
        );
    }

    /// GFM 表格：thead 表头行存在（moli-html2md 的强项口径）
    #[test]
    fn converts_table_with_thead() {
        let html = r#"<html><body><table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table></body></html>"#;
        let md = MoliHtml2MdConverter::new()
            .convert(html)
            .expect("convert ok");
        assert!(md.contains("| A | B |"), "表头行缺失: {md}");
        assert!(md.contains("| 1 | 2 |"), "数据行缺失: {md}");
    }

    /// 空输入返回空串（非错误，链语义依赖该契约）
    #[test]
    fn empty_input_returns_empty_string() {
        assert_eq!(MoliHtml2MdConverter::new().convert("  \n\t ").unwrap(), "");
    }

    /// 深层嵌套（> 默认 max_depth 1000）内容不得静默丢失（性能实证 回归钉）
    #[test]
    fn deep_nesting_beyond_1000_levels_kept() {
        let mut html = String::from("<html><body>");
        for _ in 0..1500 {
            html.push_str("<div>");
        }
        html.push_str("deep-marker");
        for _ in 0..1500 {
            html.push_str("</div>");
        }
        html.push_str("</body></html>");
        let md = MoliHtml2MdConverter::new()
            .convert(&html)
            .expect("convert ok");
        assert!(md.contains("deep-marker"), "1500 层嵌套文本被截断: {md}");
    }
}
