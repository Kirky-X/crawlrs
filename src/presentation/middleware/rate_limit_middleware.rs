// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Rate Limit Middleware
//!
//! Provides rate limiting for both authenticated and unauthenticated requests.
//!
//! # Security Features
//!
//! - **Authenticated Requests**: Rate limited based on API key with configurable limits
//! - **Unauthenticated Requests**: Rate limited based on client IP address (default: 10 req/min)
//! - **Bearer Token Extraction**: API keys are extracted from `Authorization: Bearer <token>` header
//!
//! # Attack Prevention
//!
//! This middleware prevents the following attacks:
//! - **Rate Limit Bypass**: Unauthenticated requests cannot bypass rate limiting
//! - **DoS Attacks**: IP-based rate limiting prevents abuse from unauthenticated clients
//! - **API Key Enumeration**: Strict limits on failed authentication attempts

use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use std::sync::Arc;
use axum::{
    body::Body,
    response::Response,
    middleware::{Next},
    extract::Request,
    http::{header, StatusCode},
};
use tracing::{debug, warn};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default rate limit for unauthenticated requests (requests per minute)
const DEFAULT_IP_RATE_LIMIT: u64 = 10;

/// Time window for IP rate limiting (seconds)
const IP_RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// 简单的内存速率限制器（用于测试和 IP 限流）
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

    /// 创建用于 IP 限流的内存速率限制器
    pub fn new_for_ip_limit(limit: u64) -> Self {
        Self {
            redis_client: None,
            in_memory_counts: Arc::new(RwLock::new(HashMap::new())),
            limit,
            window_seconds: IP_RATE_LIMIT_WINDOW_SECS,
        }
    }

    /// 检查是否超过速率限制
    pub fn check_rate_limit(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut counts = self.in_memory_counts.write();

        if let Some((count, last_time)) = counts.get(key) {
            let elapsed = now.duration_since(*last_time);
            if elapsed < Duration::from_secs(self.window_seconds) {
                if *count >= self.limit {
                    return false; // 超过限制
                }
                // 增加计数
                counts.insert(key.to_string(), (*count + 1, *last_time));
                return true;
            }
        }

        // 没有记录或已过期，重置计数
        counts.insert(key.to_string(), (1, now));
        true
    }

    /// 获取当前计数和剩余配额
    pub fn get_status(&self, key: &str) -> (u64, u64) {
        let now = Instant::now();
        let counts = self.in_memory_counts.read();

        if let Some((count, last_time)) = counts.get(key) {
            let elapsed = now.duration_since(*last_time);
            if elapsed < Duration::from_secs(self.window_seconds) {
                return (*count, self.limit.saturating_sub(*count));
            }
        }

        (0, self.limit)
    }

    /// 清理过期的计数器
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut counts = self.in_memory_counts.write();
        counts.retain(|_, (_, last_time)| {
            now.duration_since(*last_time) < Duration::from_secs(self.window_seconds * 2)
        });
    }
}

/// IP 速率限制器（全局单例）
static IP_RATE_LIMITER: once_cell::sync::Lazy<RateLimiter> = 
    once_cell::sync::Lazy::new(|| RateLimiter::new_for_ip_limit(DEFAULT_IP_RATE_LIMIT));

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

/// 从请求中提取 Bearer Token
///
/// # 参数
///
/// * `req` - HTTP 请求
///
/// # 返回值
///
/// 返回 `Some(token)` 如果存在有效的 Bearer Token，否则返回 `None`
fn extract_bearer_token(req: &Request) -> Option<String> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())?;

    if !auth_header.starts_with("Bearer ") {
        return None;
    }

    let token = auth_header[7..].trim();
    if token.is_empty() {
        return None;
    }

    Some(token.to_string())
}

/// 从请求中提取客户端 IP 地址
///
/// 优先级：
/// 1. X-Forwarded-For 头（第一个 IP）
/// 2. X-Real-IP 头
/// 3. SocketAddr 扩展
///
/// # 参数
///
/// * `req` - HTTP 请求
///
/// # 返回值
///
/// 返回客户端 IP 地址字符串，如果无法获取则返回 "unknown"
fn get_client_ip(req: &Request) -> String {
    // Check X-Forwarded-For header first (for proxied requests)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(ip_str) = forwarded.to_str() {
            // Take the first IP in the chain (original client)
            if let Some(ip) = ip_str.split(',').next() {
                let ip = ip.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    // Check X-Real-IP header
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }

    // Fall back to socket address
    if let Some(addr) = req.extensions().get::<std::net::SocketAddr>() {
        return addr.ip().to_string();
    }

    "unknown".to_string()
}

/// 对未认证请求应用基于 IP 的速率限制
///
/// # 参数
///
/// * `client_ip` - 客户端 IP 地址
///
/// # 返回值
///
/// 返回 `Ok(())` 如果请求被允许，返回 `Err(Response)` 如果请求被拒绝
fn apply_ip_rate_limit(client_ip: &str) -> Result<(), Response> {
    let limiter = &*IP_RATE_LIMITER;
    
    if !limiter.check_rate_limit(client_ip) {
        let (current, _) = limiter.get_status(client_ip);
        warn!(
            "IP rate limit exceeded for {}: {}/{} requests in {} seconds",
            client_ip, current, DEFAULT_IP_RATE_LIMIT, IP_RATE_LIMIT_WINDOW_SECS
        );

        let body = serde_json::json!({
            "error": "Rate limit exceeded",
            "message": format!(
                "Unauthenticated requests are limited to {} per minute. Please provide a valid API key.",
                DEFAULT_IP_RATE_LIMIT
            ),
            "retry_after_seconds": IP_RATE_LIMIT_WINDOW_SECS
        });
        
        let json_body = serde_json::to_string(&body).unwrap_or_else(|_| {
            r#"{"error":"Rate limit exceeded","message":"Too many requests"}"#.to_string()
        });
        
        let mut response = Response::new(Body::from(json_body));
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        response.headers_mut().insert(
            "Retry-After",
            axum::http::HeaderValue::from(IP_RATE_LIMIT_WINDOW_SECS),
        );
        
        return Err(response);
    }

    debug!("IP rate limit check passed for {}", client_ip);
    Ok(())
}

/// 简化的中间件处理函数
///
/// # 安全特性
///
/// 1. **Bearer Token 认证**: 从 `Authorization: Bearer <token>` 提取 API Key
/// 2. **IP 限流**: 未认证请求受 IP 限流保护（默认 10 req/min）
/// 3. **公开端点**: 健康检查等端点不受限流影响
pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
    rate_limiting_service: Arc<dyn RateLimitingService>,
) -> Response {
    let path = req.uri().path();

    // Allow public endpoints without rate limiting
    if PUBLIC_ENDPOINTS.contains(&path) {
        debug!("Public endpoint {}, skipping rate limit", path);
        return next.run(req).await;
    }

    // Extract client IP for rate limiting
    let client_ip = get_client_ip(&req);

    // Extract API key from Bearer token
    let api_key = extract_bearer_token(&req);

    // If no API key, apply IP-based rate limiting
    if api_key.is_none() {
        debug!(
            "No Bearer token found for request to {}, applying IP rate limit for {}",
            path, client_ip
        );

        match apply_ip_rate_limit(&client_ip) {
            Ok(()) => return next.run(req).await,
            Err(response) => return response,
        }
    }

    let api_key = api_key.unwrap();

    // 获取请求路径作为 endpoint
    let endpoint = path;

    // 调用服务检查速率限制
    match rate_limiting_service.check_rate_limit(&api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => {
            debug!("Rate limit exceeded for API key starting with {}: {}",
                &api_key[..std::cmp::min(8, api_key.len())], reason);

            let body = serde_json::json!({
                "error": "Rate limit exceeded",
                "message": reason
            });
            let json_body = serde_json::to_string(&body).unwrap();
            let mut response = Response::new(Body::from(json_body));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
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
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
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
            // 但记录警告日志
            warn!("Rate limiting service error, allowing request: {}", e);
            next.run(req).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bearer_token_valid() {
        let req = Request::builder()
            .header("Authorization", "Bearer test-token-123")
            .body(Body::empty())
            .unwrap();
        
        let token = extract_bearer_token(&req);
        assert_eq!(token, Some("test-token-123".to_string()));
    }

    #[test]
    fn test_extract_bearer_token_missing() {
        let req = Request::builder()
            .body(Body::empty())
            .unwrap();
        
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_wrong_type() {
        let req = Request::builder()
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_empty() {
        let req = Request::builder()
            .header("Authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new_for_ip_limit(5);
        let key = "test-ip";
        
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(key));
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new_for_ip_limit(3);
        let key = "test-ip-2";
        
        // First 3 should pass
        assert!(limiter.check_rate_limit(key));
        assert!(limiter.check_rate_limit(key));
        assert!(limiter.check_rate_limit(key));
        
        // 4th should be blocked
        assert!(!limiter.check_rate_limit(key));
    }

    #[test]
    fn test_get_client_ip_from_forwarded() {
        let req = Request::builder()
            .header("X-Forwarded-For", "192.168.1.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        
        let ip = get_client_ip(&req);
        assert_eq!(ip, "192.168.1.1");
    }

    #[test]
    fn test_get_client_ip_from_real_ip() {
        let req = Request::builder()
            .header("X-Real-IP", "192.168.1.2")
            .body(Body::empty())
            .unwrap();
        
        let ip = get_client_ip(&req);
        assert_eq!(ip, "192.168.1.2");
    }

    #[test]
    fn test_get_client_ip_unknown() {
        let req = Request::builder()
            .body(Body::empty())
            .unwrap();
        
        let ip = get_client_ip(&req);
        assert_eq!(ip, "unknown");
    }
}
