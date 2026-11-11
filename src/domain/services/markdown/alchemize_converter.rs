// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! alchemize html 引擎单转换器（`alchemize` 特性；试验接入）
//!
//! 桥接自研引擎仓 alchemize 的同步门面 [`alchemize::Alchemize::convert_sync`]：
//! `html` 特性链（htmd 基础链 + 自研深链主路径；深链名 `html-deep`，含空输出
//! 防呆/双失败诚实报错、math `$TeX$` 保真路由）在 alchemize 侧内建，此处单
//! 转换器直通——与 htmd 臂同构，无 crawlrs 侧二次兜底。空壳/纯脚本页返回空串
//! （正确抑制，与 moli 链语义一致）。
//!
//! 与 moli 链的可观察差异（切换评估已完成，见 alchemize-html-deep-replacement
//! 实测记录）：math 页产出 `$TeX$` 而非泄漏 MathML；隐藏/导航元素启发式不同；
//! 内建兜底不产生 `crawlrs_md_converter_fallback_total` 指标（quality 标注
//! `local_fallback` 随结果丢弃）。

use crate::domain::services::markdown_service::{MarkdownConverter, MarkdownError};

/// alchemize html-deep 转换器（进程内构建一次，`convert_sync` 为纯 CPU 调用）
pub struct AlchemizeMarkdownConverter {
    inner: alchemize::Alchemize,
}

impl AlchemizeMarkdownConverter {
    pub fn new() -> Self {
        Self {
            inner: alchemize::AlchemizeBuilder::new().build(),
        }
    }
}

impl Default for AlchemizeMarkdownConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownConverter for AlchemizeMarkdownConverter {
    fn name(&self) -> &'static str {
        "alchemize"
    }

    fn convert(&self, html: &str) -> Result<String, MarkdownError> {
        let input = alchemize::domain::input::Input::Bytes {
            data: html.as_bytes().to_vec(),
            filename: "page.html".to_string(),
            mimetype: None,
            extension: Some("html".to_string()),
        };
        let result = self
            .inner
            .convert_sync(&input)
            .map_err(|e| MarkdownError::ConversionFailed(format!("alchemize: {e}")))?;
        Ok(result.markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_alchemize() {
        assert_eq!(AlchemizeMarkdownConverter::new().name(), "alchemize");
    }

    #[test]
    fn converts_basic_structures() {
        let md = AlchemizeMarkdownConverter::new()
            .convert("<h1>Title</h1><p>para with <b>bold</b> and <a href=\"/x\">link</a></p>")
            .expect("convert ok");
        assert!(md.contains("# Title"), "md={md}");
        assert!(md.contains("**bold**"), "md={md}");
        assert!(md.contains("[link](/x)"), "md={md}");
    }

    #[test]
    fn converts_table_with_thead() {
        let md = AlchemizeMarkdownConverter::new()
            .convert(
                "<table><thead><tr><th>A</th><th>B</th></tr></thead>\
                 <tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
            )
            .expect("convert ok");
        assert!(md.contains("| A | B |"), "md={md}");
        assert!(md.contains("| 1 | 2 |"), "md={md}");
    }

    #[test]
    fn math_routes_to_tex_not_mathml() {
        let md = AlchemizeMarkdownConverter::new()
            .convert("<p>Euler: <math alttext=\"e^{i\\pi}+1=0\"><mrow><mi>e</mi></mrow></math></p>")
            .expect("convert ok");
        assert!(md.contains("$e^{i\\pi}+1=0$"), "md={md}");
        assert!(!md.contains("<math>"), "math 元素不得泄漏: {md}");
    }

    #[test]
    fn script_only_page_suppresses_to_empty() {
        let md = AlchemizeMarkdownConverter::new()
            .convert("<script>var data={\"k\":\"v\"};</script>")
            .expect("convert ok");
        assert!(
            md.trim().is_empty(),
            "空壳页空输出是正确抑制，不得注入占位文本: {md}"
        );
    }

    #[test]
    fn deep_nesting_does_not_overflow_stack() {
        let depth = 2000;
        let html = format!(
            "<div>{}content{}</div>",
            "<div>".repeat(depth),
            "</div>".repeat(depth)
        );
        let md = AlchemizeMarkdownConverter::new()
            .convert(&html)
            .expect("convert ok");
        assert!(md.contains("content"), "md={md}");
    }
}
