// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::{error, warn};

use crate::presentation::middleware::rate_limit_middleware::RateLimiter;

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
    // Allow public endpoints
    let path = request.uri().path();
    if path == "/health"
        || path == "/metrics"
        || path == "/v1/version"
        || path == "/v1/extract"
        || path.starts_with("/v1/crawl")
    {
        return Ok(next.run(request).await);
    }

    let api_key_id = request.extensions().get::<String>().cloned().ok_or_else(|| {
        error!("API Key not found in request extensions. Ensure AuthMiddleware is applied before DistributedRateLimitMiddleware.");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    rate_limiter.check(&api_key_id).await.map_err(|e| {
        warn!("Rate limit check failed for API Key {}: {}", api_key_id, e);
        StatusCode::TOO_MANY_REQUESTS
    })?;

    Ok(next.run(request).await)
}
