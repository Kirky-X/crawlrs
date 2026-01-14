// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::rate_limit_middleware::RateLimiter;
use crate::presentation::middleware::RATE_LIMIT_EXCLUDED_ENDPOINTS;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::{debug, error, warn};

/// 分布式速率限制中间件
///
/// 基于API密钥应用分布式速率限制
///
/// # 参数
///
/// * `rate_limiter` - 速率限制器状态
/// * `request` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(impl IntoResponse)` - 处理成功的响应
/// * `Err(StatusCode)` - 处理失败的状态码
pub async fn distributed_rate_limit_middleware(
    State(rate_limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let path = request.uri().path();
    debug!("DistributedRateLimitMiddleware: Path = {}", path);

    // Allow public endpoints (no rate limiting for these)
    if RATE_LIMIT_EXCLUDED_ENDPOINTS
        .iter()
        .any(|&endpoint| path == endpoint || path.starts_with(endpoint))
    {
        debug!(
            "DistributedRateLimitMiddleware: Skipping public endpoint {}",
            path
        );
        return Ok(next.run(request).await);
    }

    debug!("DistributedRateLimitMiddleware: Checking for API key in extensions");

    // Try to get API key from token_str first (set by auth middleware - this is the raw API key)
    // Fall back to using api_key_id from AuthState if token is not available
    let api_key = if let Some(token_str) = request.extensions().get::<String>().cloned() {
        debug!(
            "Found API key token: {}...",
            &token_str[..std::cmp::min(8, token_str.len())]
        );
        token_str // This is the raw API key from Authorization header
    } else if let Some(auth_state) = request.extensions().get::<AuthState>() {
        debug!("Using api_key_id from AuthState");
        auth_state.api_key_id.to_string() // This is the database ID
    } else {
        error!("Neither API key token nor AuthState found in request extensions.");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    debug!(
        "DistributedRateLimitMiddleware: Rate limiting check for API key: {}...",
        &api_key[..std::cmp::min(8, api_key.len())]
    );

    let api_key_prefix = &api_key[..std::cmp::min(8, api_key.len())];
    debug!(
        "DistributedRateLimitMiddleware: Checking rate limit for API Key starting with: {}",
        api_key_prefix
    );

    debug!("DistributedRateLimitMiddleware: Calling rate_limiter.check()");
    match rate_limiter.check(&api_key).await {
        Ok(()) => {
            debug!("DistributedRateLimitMiddleware: Rate limit check passed");
            Ok(next.run(request).await)
        }
        Err(e) => {
            warn!(
                "Rate limit check failed for API Key starting with {}: {}",
                api_key_prefix, e
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit check failed: {}", e),
            )
                .into_response())
        }
    }
}
