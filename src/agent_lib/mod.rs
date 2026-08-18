// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `agent-lib` 库面：面向 agent 的高层 `search()` / `fetch()` API。
//!
//! 复用 `search` / `engines` / `content_extractor` / `markdown_service` 既有能力，
//! 提供两个高层异步函数：
//!
//! - [`search`]：按 provider 检索网页并返回标题/URL/摘要
//! - [`fetch`]：拉取 URL 的 raw HTML → 正文提取（trafilatura 主，dom_smoothie 回退）
//!   → Markdown 转换，支持 `max_bytes` 限长、超时与逐跳 [`EgressGuard`] 裁决
//!
//! 本模块的错误独立为 [`AgentLibError`]（thiserror），与平台错误解耦。
//!
//! 该模块仅在 `agent-lib` feature 启用时编译，不依赖 dbnexus/axum 等平台依赖。

pub use error::AgentLibError;
pub use fetch::{fetch, FetchOptions, FetchedContent};
pub use search::{search, SearchProvider, SearchResult};

mod error;
mod fetch;
mod search;

/// 逐跳出口裁决回调（SSRF/策略授权载体）。
///
/// 由接入方（如 agentstem）将自身策略包装为实现；crawlrs 在 **每跳（含重定向）请求前**
/// 调用 `allow(url)`，返回 `false` 立即中止并返回 [`AgentLibError::EgressDenied`]。
pub trait EgressGuard: Send + Sync {
    /// 是否允许请求给定 URL。
    fn allow(&self, url: &url::Url) -> bool;
}
