// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! HTTP 方法枚举（共享类型，跨层复用）
//!
//! 将 `HttpMethod` 从 `engines::engine_client` 提升至 `common` 层，
//! 消除原 `infrastructure::oxcache::cache_mode`（现已提升至 `common::cache_mode`）对 `engines` 层的反向依赖
//! （CRITICAL-1：infrastructure → engines 层级违规修复）。
//!
//! # 设计依据
//!
//! `HttpMethod` 是基础 HTTP 概念（GET / POST），无业务逻辑，属于所有层
//! 共享的通用类型。原定义在 `engines::engine_client` 导致：
//! - 原 `infrastructure::oxcache::cache_mode`（现已提升至 `common::cache_mode`）反向依赖 `engines` 层
//! - `application::dto::scrape_request` 间接依赖 `engines` 层
//!
//! 提升至 `common` 层后，依赖方向恢复正常：
//! - `infrastructure` → `common`（合法）
//! - `engines` → `common`（合法）
//! - `application` → `common`（合法）

/// HTTP 方法枚举
///
/// 仅覆盖爬取场景所需的 GET / POST 两种方法：
/// - `Get`：幂等读，可缓存
/// - `Post`：非幂等写，不可缓存
///
/// # 默认值
///
/// `Default::default()` 返回 `HttpMethod::Get`，与爬取场景的默认行为一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpMethod {
    /// HTTP GET（幂等读，可缓存）
    #[default]
    Get,
    /// HTTP POST（非幂等写，不可缓存）
    Post,
}
