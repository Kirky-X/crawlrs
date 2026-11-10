// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Markdown 转换器链子模块（html2md-converter-chain 变更）
//!
//! 结构：`moli-html2md` 主转换器（`MoliHtml2MdConverter`）→ `htmd` 兜底
//! （`HtmdMarkdownService`），由 `ConverterChain` 组合；与既有
//! 正文提取链（Trafilatura→DomSmoothie）同构——主路径失败/空输出时诚实降级。
//!
//! - `moli_adapter::ScraperDom`：`scraper::Html`（html5ever 树）→ `moli_html2md::Dom` 适配器
//! - `moli_converter::MoliHtml2MdConverter`：主转换器实现
//! - `converter_chain::ConverterChain`：链执行器（fallback 触发 + 日志 + 指标）

#[cfg(feature = "alchemize")]
pub mod alchemize_converter;
pub mod converter_chain;
pub mod moli_adapter;
pub mod moli_converter;

#[cfg(feature = "alchemize")]
pub use alchemize_converter::AlchemizeMarkdownConverter;
pub use converter_chain::ConverterChain;
pub use moli_adapter::ScraperDom;
pub use moli_converter::MoliHtml2MdConverter;

use crate::domain::services::markdown_service::{HtmdMarkdownService, MarkdownConverter};
use std::sync::Arc;

/// 按 `[markdown].converter` 配置装配转换器（装配面单一入口）
///
/// - `moli`（默认）：[`ConverterChain::default_chain`]（moli-html2md 主 + htmd 兜底）
/// - `htmd`：htmd 单转换器直通（诊断/对比用，无兜底语义）
/// - `alchemize`：自研 html-deep 引擎直通（`alchemize` 特性；内建 htmd 兜底，
///   试验特性，MSRV 1.97.1）
///
/// 非法值在配置校验期已被拒绝（`invalid_markdown_converter`）；此处在装配期
/// 再 fail-closed 一次（防御编程错误绕过校验的调用路径）。
pub fn build_converter(
    settings: &crate::config::settings::MarkdownSettings,
) -> Arc<dyn MarkdownConverter> {
    match settings.converter_kind() {
        Ok(crate::config::settings::MarkdownConverterKind::Htmd) => {
            Arc::new(HtmdMarkdownService::new())
        }
        Ok(crate::config::settings::MarkdownConverterKind::Moli) => {
            Arc::new(ConverterChain::default_chain())
        }
        #[cfg(feature = "alchemize")]
        Ok(crate::config::settings::MarkdownConverterKind::Alchemize) => {
            Arc::new(alchemize_converter::AlchemizeMarkdownConverter::new())
        }
        Err(e) => {
            log::error!(
                target: "crawlrs::markdown",
                "{}; falling back to moli chain (default)",
                e
            );
            Arc::new(ConverterChain::default_chain())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::MarkdownSettings;

    #[test]
    fn build_converter_moli_default_returns_chain() {
        let settings = MarkdownSettings::default();
        assert_eq!(build_converter(&settings).name(), "moli+htmd");
    }

    #[test]
    fn build_converter_htmd_returns_htmd_direct() {
        let settings = MarkdownSettings {
            converter: "htmd".to_string(),
        };
        assert_eq!(build_converter(&settings).name(), "htmd");
    }

    #[test]
    fn build_converter_invalid_falls_back_to_chain() {
        // 配置校验期已拒绝非法值；此分支防御编程绕过校验的调用路径
        let settings = MarkdownSettings {
            converter: "banana".to_string(),
        };
        assert_eq!(build_converter(&settings).name(), "moli+htmd");
    }
}
