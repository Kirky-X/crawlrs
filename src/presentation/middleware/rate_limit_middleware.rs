// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::infrastructure::cache::redis_client::RedisClient;
use std::sync::Arc;
use axum::{
    body::Body,
    response::Response,
    middleware::{Next},
    extract::Request,
};
use tracing::debug;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 简单的内存速率限制器（用于测试）
#[derive(Clone)]
#[allow(dead_code)]
pub struct RateLimiter {
    /// Redis 客户端（预留用于分布式速率限制）
    redis_client: Option<Arc<RedisClient>>,
    /// 内存中速率限制计数器
    in_memory_counts: Arc<RwLock<HashMap<String, (u64, Instant)>>>,
    /// 请求限制数
    limit: u64,
    /// 时间窗口（秒）
    window_seconds: u64,
}

impl RateLimiter {
    /// 创建新的速率限制器
    pub fn new(redis_client: Arc<RedisClient>, limit: u64) -> Self {
        Self {
            redis_client: Some(redis_client),
            in_memory_counts: Arc::new(RwLock::new(HashMap::new())),
            limit,
            window_seconds: 60,
        }
    }

    /// 检查是否超过速率限制
    pub fn check_rate_limit(&self, key: &str) -> bool {
        let now = Instant::now();
        let counts = self.in_memory_counts.write();

        if let Some((count, last_time)) = counts.get(key) {
            let elapsed = now.duration_since(*last_time);
            if elapsed < Duration::from_secs(self.window_seconds) {
                return *count < self.limit;
            }
        }

        true // 没有记录或已过期，允许请求
    }
}

/// 速率限制中间件
///
/// 使用注入的 RateLimitingService 进行 API 密钥速率限制检查
#[derive(Clone)]
#[allow(dead_code)]
pub struct RateLimitMiddleware {
    /// 速率限制服务
    rate_limiting_service: Arc<dyn RateLimitingService>,
}

impl RateLimitMiddleware {
    /// 创建新的速率限制中间件实例
    ///
    /// # 参数
    ///
    /// * `rate_limiting_service` - 速率限制服务实例
    ///
    /// # 返回值
    ///
    /// 返回新的速率限制中间件实例
    pub fn new(rate_limiting_service: Arc<dyn RateLimitingService>) -> Self {
        Self {
            rate_limiting_service,
        }
    }
}

/// 简化的中间件处理函数
pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
    rate_limiting_service: Arc<dyn RateLimitingService>,
) -> Response {
    // 从请求中提取 API 密钥
    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    // 如果没有 API 密钥，直接放行
    if api_key.is_empty() {
        return next.run(req).await;
    }

    // 获取请求路径作为 endpoint
    let endpoint = req.uri().path();

    // 调用服务检查速率限制
    match rate_limiting_service.check_rate_limit(api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => {
            debug!("Rate limit exceeded for API key starting with {}: {}",
                &api_key[..std::cmp::min(8, api_key.len())], reason);

            let body = serde_json::json!({
                "error": "Rate limit exceeded",
                "message": reason
            });
            let json_body = serde_json::to_string(&body).unwrap();
            let mut response = Response::new(Body::from(json_body));
            *response.status_mut() = axum::http::StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            response
        }
        Ok(RateLimitResult::RetryAfter { retry_after_seconds }) => {
            debug!("Rate limit retry after for API key starting with {}: {} seconds",
                &api_key[..std::cmp::min(8, api_key.len())], retry_after_seconds);

            let body = serde_json::json!({
                "error": "Rate limit exceeded",
                "message": format!("Retry after {} seconds", retry_after_seconds)
            });
            let json_body = serde_json::to_string(&body).unwrap();
            let mut response = Response::new(Body::from(json_body));
            *response.status_mut() = axum::http::StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            response.headers_mut().insert(
                "Retry-After",
                axum::http::HeaderValue::from(retry_after_seconds),
            );
            response
        }
        Ok(RateLimitResult::Allowed) => {
            debug!("Rate limit check passed for API key starting with: {}",
                &api_key[..std::cmp::min(8, api_key.len())]);
            next.run(req).await
        }
        Err(e) => {
            debug!("Rate limit check failed: {}", e);
            // 速率限制服务出错时，允许请求通过（fail open）
            next.run(req).await
        }
    }
}
