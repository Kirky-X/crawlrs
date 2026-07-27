// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 请求客户端句柄（H3 修复：tuple 泄漏状态 + M3/M4 修复）
//!
//! `ReqwestEngine::get_client` 之前返回 `(reqwest::Client, Option<String>)`，
//! 调用方需同时持有两者才能正确回填 `mark_failure` / `mark_success`，违反 SRP。
//!
//! `ClientHandle` 封装客户端 + 实际使用的代理 URL + fallback 标志，
//! 提供 `report_failure` / `report_success` 方法，
//! 调用方只需传入 `&dyn ProxyProvider` 即可完成状态回填。
//!
//! ## M3 修复（字段可见性）
//!
//! `used_proxy_url` 改为 `pub(crate)`，外部模块通过只读访问器 `used_proxy_url()`
//! 获取，避免外部代码直接修改字段状态。
//!
//! ## M4 修复（失败显性化）
//!
//! `build_custom_client` 失败时返回 fallback client 但只用 warn 日志，违反规则12。
//! 现在通过 `is_fallback` 标志让调用方感知 client 构建失败：
//! - `true`：client 构建失败，已回退到注入的 http_client
//! - `false`：client 构建成功

use crate::engines::provider::ProxyProvider;

/// 请求客户端句柄
///
/// 封装 `reqwest::Client` + 实际使用的代理 URL（如有） + fallback 标志，
/// 提供统一的状态回填接口（`report_failure` / `report_success`）。
pub struct ClientHandle {
    /// 用于发送请求的 reqwest 客户端
    pub client: reqwest::Client,
    /// 实际使用的代理 URL（用于回填 ProxyProvider）
    ///
    /// - `Some(url)`：本次请求使用了代理（请求级代理或池代理命中）
    /// - `None`：未使用代理（直接用注入的 http_client）
    ///
    /// M3 修复：改为 `pub(crate)`，外部模块通过 [`Self::used_proxy_url`] 访问器读取。
    pub(crate) used_proxy_url: Option<String>,
    /// 是否为 fallback 客户端（M4 修复：失败显性化）
    ///
    /// - `true`：`build_custom_client` 构建失败，已回退到注入的 http_client
    /// - `false`：client 构建成功（含正常无代理路径）
    ///
    /// 调用方可通过 [`Self::is_fallback`] 检查此标志，感知 client 构建失败
    /// （规则12：失败必须显性化，不藏默认值背后）。
    is_fallback: bool,
}

impl ClientHandle {
    /// 创建新的 ClientHandle
    ///
    /// # 参数
    ///
    /// - `client`: reqwest 客户端（成功构建或 fallback 的 http_client）
    /// - `used_proxy_url`: 实际使用的代理 URL（即使 fallback 也可保留，用于 report_failure）
    /// - `is_fallback`: 是否为 fallback 路径（M4 修复：失败显性化）
    ///
    /// `is_fallback=true` 时调用方应感知 client 构建失败（规则12）。
    /// `used_proxy_url` 在 fallback 路径仍可保留，因为 report_failure 会标记无效代理
    /// 为失败，防止后续重复选择（即使本次请求实际未走代理）。
    #[inline]
    #[must_use]
    pub fn new(
        client: reqwest::Client,
        used_proxy_url: Option<String>,
        is_fallback: bool,
    ) -> Self {
        Self {
            client,
            used_proxy_url,
            is_fallback,
        }
    }

    /// 请求失败时通知 ProxyProvider 标记代理进入冷却
    ///
    /// 仅当 `used_proxy_url` 为 `Some` 时才通知；`None` 时为空操作。
    /// 这样调用方无需感知代理状态，符合 SRP。
    #[inline]
    pub fn report_failure(&self, provider: &dyn ProxyProvider) {
        if let Some(url) = &self.used_proxy_url {
            provider.mark_failure(url);
        }
    }

    /// 请求成功时通知 ProxyProvider 恢复代理健康
    ///
    /// 仅当 `used_proxy_url` 为 `Some` 时才通知；`None` 时为空操作。
    #[inline]
    pub fn report_success(&self, provider: &dyn ProxyProvider) {
        if let Some(url) = &self.used_proxy_url {
            provider.mark_success(url);
        }
    }

    /// 是否使用了代理
    #[must_use]
    #[inline]
    pub fn has_proxy(&self) -> bool {
        self.used_proxy_url.is_some()
    }

    /// 获取实际使用的代理 URL（M3 修复：只读访问器）
    ///
    /// - `Some(url)`：本次请求使用了代理
    /// - `None`：未使用代理
    #[must_use]
    #[inline]
    pub fn used_proxy_url(&self) -> Option<&str> {
        self.used_proxy_url.as_deref()
    }

    /// 是否为 fallback 客户端（M4 修复：失败显性化）
    ///
    /// `true` 表示 `build_custom_client` 构建失败，已回退到注入的 http_client。
    /// 调用方应通过此标志感知 client 构建失败（规则12）。
    #[must_use]
    #[inline]
    pub fn is_fallback(&self) -> bool {
        self.is_fallback
    }
}
