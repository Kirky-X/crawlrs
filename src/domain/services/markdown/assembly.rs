// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Markdown 转换器装配实现（mod.rs 仅声明/再导出，满足仓库 mod.rs 钩子约束）

use crate::domain::services::markdown::alchemize_converter::AlchemizeMarkdownConverter;
use crate::domain::services::markdown_service::{HtmdMarkdownService, MarkdownConverter};
use std::sync::Arc;

/// 按 `[markdown].converter` 配置装配转换器（装配面单一入口）
///
/// - `alchemize`（默认）：自研 html 引擎直通（内建兜底、math `$TeX$` 保真路由）
/// - `htmd`：htmd 单转换器直通（诊断/对比用，无兜底语义）
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
        Ok(crate::config::settings::MarkdownConverterKind::Alchemize) => {
            Arc::new(AlchemizeMarkdownConverter::new())
        }
        Err(e) => {
            log::error!(
                target: "crawlrs::markdown",
                "{}; falling back to alchemize (default)",
                e
            );
            Arc::new(AlchemizeMarkdownConverter::new())
        }
    }
}
