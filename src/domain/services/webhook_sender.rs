// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! WebhookSender - 抽象 webhook HTTP 发送接口
//!
//! 此模块定义了 webhook 发送的抽象接口，用于解耦
//! WebhookService 与具体 HTTP 实现之间的依赖。
//!
//! 通过依赖注入可以轻松替换不同的 HTTP 客户端实现，
//! 而不影响业务逻辑层。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// WebhookSender trait - webhook HTTP 发送接口
///
/// 此 trait 定义了发送 webhook HTTP 请求的最小接口。
/// 用于解耦业务逻辑与具体的 HTTP 实现。
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use anyhow::Result;
/// use serde_json::json;
///
/// #[async_trait]
/// impl WebhookSender for MyHttpClient {
///     async fn send(
///         &self,
///         url: &str,
///         payload: &Value,
///         headers: Option<&HashMap<String, String>>,
///     ) -> Result<()> {
///         // 实现发送逻辑
///     }
/// }
/// ```
#[async_trait]
pub trait WebhookSender: Send + Sync {
    /// 发送 webhook HTTP POST 请求
    ///
    /// # Arguments
    ///
    /// * `url` - 目标 webhook URL
    /// * `payload` - 要发送的 JSON payload
    /// * `headers` - 可选的 HTTP 请求头
    ///
    /// # Returns
    ///
    /// * `Ok(())` - 发送成功
    /// * `Err(anyhow::Error)` - 发送失败
    async fn send(
        &self,
        url: &str,
        payload: &Value,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<()>;

    /// 发送 webhook 并返回响应状态码
    ///
    /// 此方法提供更详细的响应信息，包括 HTTP 状态码。
    ///
    /// # Arguments
    ///
    /// * `url` - 目标 webhook URL
    /// * `payload` - 要发送的 JSON payload
    /// * `headers` - 可选的 HTTP 请求头
    ///
    /// # Returns
    ///
    /// * `Ok(status_code)` - 发送成功，返回 HTTP 状态码
    /// * `Err(anyhow::Error)` - 发送失败
    async fn send_with_status(
        &self,
        url: &str,
        payload: &Value,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<u16>;
}
