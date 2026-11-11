// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Markdown 转换器装配面（html2md-converter-chain → alchemize 收编）
//!
//! 结构：`alchemize_converter::AlchemizeMarkdownConverter`（自研 html 引擎链：
//! htmd 基础链 + 自研深链，内建空输出防呆/双失败诚实报错/math `$TeX$` 保真路由）
//! 为默认转换器；`HtmdMarkdownService` 保留直通诊断臂。moli-html2md 主 + htmd
//! 兜底的旧链已退役（见 CHANGELOG Unreleased）。
//!
//! - `alchemize_converter::AlchemizeMarkdownConverter`：默认转换器实现

pub mod alchemize_converter;
pub mod assembly;

pub use alchemize_converter::AlchemizeMarkdownConverter;
pub use assembly::build_converter;

#[cfg(test)]
mod assembly_tests;
