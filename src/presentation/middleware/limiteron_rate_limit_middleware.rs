// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron 速率限制中间件
//!
//! 直接使用 limiteron::Governor 进行速率限制检查

use std::net::SocketAddr;
use std::sync::Arc;

use ahash::AHashMap;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use limiteron::prelude::{Decision, Governor, RequestContext};
use tracing::{debug, error, warn};

use crate::presentation::middleware::RATE_LIMIT_EXCLUDED_ENDPOINTS;

/// Limiteron 速率限制中间件状态
#[derive(Clone)]
pub struct LimiteronMiddlewareState {
    /// Limiteron Governor 实例
    pub governor: Arc<Governor>,
}

/// 提取客户端 IP 地址
fn extract_client_ip(request: &Request, remote_addr: Option<SocketAddr>) -> Option<String> {
    // 尝试从 X-Forwarded-For 头提取
    if let Some(forwarded) = request.headers().get("X-Forwarded-For") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            // 取第一个 IP（最原始的客户端 IP）
            if let Some(ip) = forwarded_str.split(',').next() {
                return Some(ip.trim().to_string());
            }
        }
    }

    // 尝试从 X-Real-IP 头提取
    if let Some(real_ip) = request.headers().get("X-Real-IP") {
        if let Ok(ip) = real_ip.to_str() {
            return Some(ip.trim().to_string());
        }
    }

    // 从连接信息获取
    if let Some(addr) = remote_addr {
        return Some(addr.ip().to_string());
    }

    None
}

/// Limiteron 分布式速率限制中间件
///
/// 直接使用 limiteron::Governor 进行分布式速率限制检查
///
/// # 参数
///
/// * `State(governor)` - Limiteron Governor 状态
/// * `request` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(impl IntoResponse)` - 处理成功的响应
/// * `Err(StatusCode)` - 处理失败的状态码
pub async fn limiteron_rate_limit_middleware(
    State(state): State<LimiteronMiddlewareState>,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let path = request.uri().path();
    debug!("LimiteronMiddleware: Path = {}", path);

    // Allow public endpoints (no rate limiting for these)
    if RATE_LIMIT_EXCLUDED_ENDPOINTS
        .iter()
        .any(|&endpoint| path == endpoint || path.starts_with(endpoint))
    {
        debug!(
            "LimiteronMiddleware: Skipping public endpoint {}",
            path
        );
        return Ok(next.run(request).await);
    }

    debug!("LimiteronMiddleware: Building request context");

    // 尝试获取 API key
    let api_key = if let Some(token_str) = request.extensions().get::<String>().cloned() {
        debug!(
            "LimiteronMiddleware: Found API key: {}...",
            &token_str[..std::cmp::min(8, token_str.len())]
        );
        Some(token_str)
    } else {
        None
    };

    // 尝试获取用户 ID
    let user_id = request
        .extensions()
        .get::<crate::presentation::middleware::auth_middleware::AuthState>()
        .map(|state| state.api_key_id.to_string());

    // 提取客户端 IP（尝试从 ConnectInfo 获取）
    let remote_addr: Option<SocketAddr> = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|conn| conn.0.clone());

    let client_ip = extract_client_ip(&request, remote_addr);

    // 构建请求头映射
    let mut headers: AHashMap<String, String> = AHashMap::new();
    if let Some(ref api_key) = api_key {
        headers.insert("x-api-key".to_string(), api_key.clone());
    }
    if let Some(ref user_id) = user_id {
        headers.insert("x-user-id".to_string(), user_id.clone());
    }

    // 构建请求上下文
    let context = RequestContext {
        ip: client_ip.clone(),
        user_id,
        api_key,
        path: path.to_string(),
        method: request.method().to_string(),
        headers,
        query_params: AHashMap::new(),
        client_ip,
        mac: None,
        device_id: None,
    };

    debug!(
        "LimiteronMiddleware: Checking rate limit for path: {}, method: {}",
        path,
        request.method()
    );

    // 使用 Governor 检查限流
    match state.governor.check(&context).await {
        Ok(Decision::Allowed(_)) => {
            debug!("LimiteronMiddleware: Rate limit check passed");
            Ok(next.run(request).await)
        }
        Ok(Decision::Rejected(reason)) => {
            warn!(
                "LimiteronMiddleware: Rate limit exceeded for path {}: {}",
                path, reason
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded: {}", reason),
            )
                .into_response())
        }
        Ok(Decision::Banned(ban_info)) => {
            warn!(
                "LimiteronMiddleware: Request banned for path {}: {}",
                path,
                ban_info.reason()
            );
            Ok((
                StatusCode::FORBIDDEN,
                format!("Access forbidden: {}", ban_info.reason()),
            )
                .into_response())
        }
        Err(e) => {
            error!(
                "LimiteronMiddleware: Rate limit check error for path {}: {}",
                path, e
            );
            // Fail open - allow request if rate limiting service fails
            debug!("LimiteronMiddleware: Failing open due to error");
            Ok(next.run(request).await)
        }
    }
}
