// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 正文提取模块（content-processing R2/R3，T044-T049 / R-content-002、R-content-003）
//!
//! 模块结构：
//! - [`traits`]：`ContentExtractor` trait + `ExtractedContent` + `PageType`
//! - [`css_rule_extractor`]：`CssRuleExtractor` 兜底实现（无 feature 依赖）
//! - [`trafilatura_extractor`]：`TrafilaturaExtractor` 主路径（gated `extractor-trafilatura`）
//! - [`dom_smoothie_extractor`]：`DomSmoothieExtractor` 性能回退（gated `extractor-dom-smoothie`）
//! - [`facade`]：`ContentExtractionFacade` 优先级路由 + LLM 回退
//!
//! 特性门控策略（R-content-003）：
//! - `extractor-trafilatura`：启用 trafilatura 实现
//! - `extractor-dom-smoothie`：启用 dom_smoothie 实现
//! - `extractor-full`：聚合两者
//! - 三特性均关闭时 Facade 退化为 CssRule 兜底（编译通过，功能可用）

pub mod traits;
pub use traits::{ContentExtractor, ExtractError, ExtractedContent, PageType, Result};

pub mod css_rule_extractor;
pub use css_rule_extractor::CssRuleExtractor;

#[cfg(feature = "extractor-trafilatura")]
pub mod trafilatura_extractor;
#[cfg(feature = "extractor-trafilatura")]
pub use trafilatura_extractor::TrafilaturaExtractor;

#[cfg(feature = "extractor-dom-smoothie")]
pub mod dom_smoothie_extractor;
#[cfg(feature = "extractor-dom-smoothie")]
pub use dom_smoothie_extractor::DomSmoothieExtractor;

pub mod facade;
pub use facade::ContentExtractionFacade;
