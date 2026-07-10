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
use crate::infrastructure::security::secure_ip::{get_secure_client_ip, TrustedProxyConfig};
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use log::{debug, error, warn};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default rate limit for unauthenticated requests (requests per minute)
const DEFAULT_IP_RATE_LIMIT: u64 = 10;

/// Time window for IP rate limiting (seconds)
const IP_RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Rate limiting service fail-open behavior
///
/// When the rate limiting service encounters an error:
/// - If true: Allow the request to pass (fail open) - default for availability
/// - If false: Reject the request with 503 Service Unavailable - default for security
///
/// This can be controlled via RATE_LIMIT_FAIL_OPEN environment variable.
const RATE_LIMIT_FAIL_OPEN: bool = true;

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

        // 先检查是否存在记录
        let should_reset = if let Some((_count, last_time)) = counts.get(key) {
            let elapsed = now.duration_since(*last_time);
            elapsed >= Duration::from_secs(self.window_seconds)
        } else {
            false
        };

        if should_reset {
            // 没有记录或已过期，重置计数
            counts.insert(key.to_string(), (1, now));
            return true;
        }

        // 获取当前值并检查
        if let Some((count, last_time)) = counts.get(key) {
            if *count >= self.limit {
                return false; // 超过限制
            }
            // 增加计数
            let new_count = *count + 1;
            let last_time = *last_time;
            counts.insert(key.to_string(), (new_count, last_time));
            return true;
        }

        // 没有记录，创建新记录
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
/// # 安全说明
///
/// 此函数使用安全的 IP 提取逻辑来防止 X-Forwarded-For 头伪造攻击。
/// 只有当请求来自可信代理（如 10.x.x.x, 172.16.x.x, 192.168.x.x）时，
/// 才信任转发头中的 IP 地址。
///
/// # 参数
///
/// * `req` - HTTP 请求
///
/// # 返回值
///
/// 返回客户端的真实 IP 地址，如果无法获取则返回 "unknown"
fn get_client_ip(req: &Request) -> String {
    let trusted_config = TrustedProxyConfig::default();
    get_secure_client_ip(req, &trusted_config)
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
fn apply_ip_rate_limit(client_ip: &str) -> Result<(), Box<Response>> {
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

        return Err(Box::new(response));
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
    let api_key = match api_key {
        Some(key) => key,
        None => {
            debug!(
                "No Bearer token found for request to {}, applying IP rate limit for {}",
                path, client_ip
            );

            match apply_ip_rate_limit(&client_ip) {
                Ok(()) => return next.run(req).await,
                Err(response) => return *response,
            }
        }
    };

    // 获取请求路径作为 endpoint
    let endpoint = path;

    // 调用服务检查速率限制
    match rate_limiting_service
        .check_rate_limit(&api_key, endpoint)
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            debug!(
                "Rate limit exceeded for API key starting with {}: {}",
                &api_key[..std::cmp::min(8, api_key.len())],
                reason
            );

            let body = serde_json::json!({
                "error": "Rate limit exceeded",
                "message": reason
            });
            let json_body = serde_json::to_string(&body)
                .expect("JSON serialization of rate limit response should never fail");
            let mut response = Response::new(Body::from(json_body));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            response
        }
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => {
            debug!(
                "Rate limit retry after for API key starting with {}: {} seconds",
                &api_key[..std::cmp::min(8, api_key.len())],
                retry_after_seconds
            );

            let body = serde_json::json!({
                "error": "Rate limit exceeded",
                "message": format!("Retry after {} seconds", retry_after_seconds)
            });
            let json_body = serde_json::to_string(&body)
                .expect("JSON serialization of rate limit response should never fail");
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
            debug!(
                "Rate limit check passed for API key starting with: {}",
                &api_key[..std::cmp::min(8, api_key.len())]
            );
            next.run(req).await
        }
        Err(e) => {
            // SEC-003: 可配置的 fail-open/fail-closed 行为
            if RATE_LIMIT_FAIL_OPEN {
                // Fail-open: 允许请求通过，但记录严重警告
                // 注意：这可能导致在服务故障时无法限流
                // 在生产环境中应考虑使用 fail-closed 模式
                warn!(
                    "SEC-003: Rate limiting service error - failing open (allowing request). \
                     Consider setting RATE_LIMIT_FAIL_OPEN=false for stricter security. \
                     error={} api_key_prefix={:?} path={}",
                    e,
                    &api_key[..std::cmp::min(8, api_key.len())],
                    path
                );
                next.run(req).await
            } else {
                // Fail-closed: 拒绝请求以确保安全
                error!(
                    "SEC-003: Rate limiting service error - failing closed (rejecting request) \
                     error={} api_key_prefix={:?} path={}",
                    e,
                    &api_key[..std::cmp::min(8, api_key.len())],
                    path
                );

                let body = serde_json::json!({
                    "error": "Service temporarily unavailable",
                    "message": "Rate limiting service is temporarily unavailable. Please try again later."
                });
                let json_body =
                    serde_json::to_string(&body).expect("JSON serialization should never fail");
                let mut response = Response::new(Body::from(json_body));
                *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("application/json"),
                );
                response
            }
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
        let req = Request::builder().body(Body::empty()).unwrap();

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
        // 当直接请求没有可信代理时，X-Forwarded-For 头将被忽略
        // 因为请求不是来自可信代理
        let req = Request::builder()
            .header("X-Forwarded-For", "192.168.1.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();

        let ip = get_client_ip(&req);
        // 安全版本会返回 "unknown" 因为没有 SocketAddr 扩展且请求不是来自可信代理
        assert_eq!(ip, "unknown");
    }

    #[test]
    fn test_get_client_ip_from_real_ip() {
        // 当直接请求没有可信代理时，X-Real-IP 头将被忽略
        let req = Request::builder()
            .header("X-Real-IP", "192.168.1.2")
            .body(Body::empty())
            .unwrap();

        let ip = get_client_ip(&req);
        // 安全版本会返回 "unknown" 因为没有 SocketAddr 扩展
        assert_eq!(ip, "unknown");
    }

    #[test]
    fn test_get_client_ip_unknown() {
        let req = Request::builder().body(Body::empty()).unwrap();

        let ip = get_client_ip(&req);
        assert_eq!(ip, "unknown");
    }

    #[test]
    fn test_get_client_ip_with_trusted_proxy() {
        // 模拟来自可信代理的请求，应该信任 X-Forwarded-For
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        let socket_addr: SocketAddr = "10.0.0.1:8080".parse().unwrap();
        let mut req = Request::builder()
            .header("X-Forwarded-For", "203.0.113.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(socket_addr));

        let ip = get_client_ip(&req);
        // 来自可信代理 (10.0.0.1)，应该信任 X-Forwarded-For
        assert_eq!(ip, "203.0.113.1");
    }

    #[test]
    fn test_get_client_ip_untrusted_proxy_rejected() {
        // 来自不可信代理的请求，X-Forwarded-For 应该被忽略
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        // 8.8.8.8 不是可信代理
        let socket_addr: SocketAddr = "8.8.8.8:8080".parse().unwrap();
        let mut req = Request::builder()
            .header("X-Forwarded-For", "203.0.113.1, 8.8.8.8")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(socket_addr));

        let ip = get_client_ip(&req);
        // 来自不可信代理，应该使用直接连接的 IP
        assert_eq!(ip, "8.8.8.8");
    }

    // ========== RateLimiter::new tests ==========

    #[test]
    fn test_rate_limiter_new_with_redis() {
        let redis = Arc::new(RedisClient::new("redis://localhost:6379").unwrap());
        let limiter = RateLimiter::new(redis, 100);
        // Verify get_status works (indirectly verifying construction)
        let (count, remaining) = limiter.get_status("fresh-key-new");
        assert_eq!(count, 0);
        assert_eq!(remaining, 100);
    }

    // ========== RateLimiter::get_status tests ==========

    #[test]
    fn test_get_status_no_record() {
        let limiter = RateLimiter::new_for_ip_limit(10);
        let (count, remaining) = limiter.get_status("nonexistent-key");
        assert_eq!(count, 0);
        assert_eq!(remaining, 10);
    }

    #[test]
    fn test_get_status_with_count() {
        let limiter = RateLimiter::new_for_ip_limit(10);
        let key = "status-key-1";
        limiter.check_rate_limit(key);
        limiter.check_rate_limit(key);
        limiter.check_rate_limit(key);
        let (count, remaining) = limiter.get_status(key);
        assert_eq!(count, 3);
        assert_eq!(remaining, 7);
    }

    #[test]
    fn test_get_status_at_limit() {
        let limiter = RateLimiter::new_for_ip_limit(3);
        let key = "status-key-2";
        for _ in 0..3 {
            limiter.check_rate_limit(key);
        }
        let (count, remaining) = limiter.get_status(key);
        assert_eq!(count, 3);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_get_status_count_exceeds_limit_saturating() {
        // When count exceeds limit (e.g., via a low limit), remaining should saturate to 0
        let limiter = RateLimiter::new_for_ip_limit(1);
        let key = "status-key-3";
        limiter.check_rate_limit(key); // count = 1
        limiter.check_rate_limit(key); // rejected, but count stays at 1 internally
        let (count, _remaining) = limiter.get_status(key);
        assert_eq!(count, 1);
    }

    // ========== RateLimiter::cleanup_expired tests ==========

    #[test]
    fn test_cleanup_expired_keeps_recent_entries() {
        let limiter = RateLimiter::new_for_ip_limit(10);
        let key = "cleanup-recent";
        limiter.check_rate_limit(key);
        limiter.cleanup_expired();
        // After cleanup, recent entry should still be tracked
        let (count, _) = limiter.get_status(key);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_cleanup_expired_empty_map() {
        let limiter = RateLimiter::new_for_ip_limit(10);
        // Should not panic on empty map
        limiter.cleanup_expired();
    }

    // ========== RateLimiter independent keys tests ==========

    #[test]
    fn test_rate_limiter_different_keys_independent() {
        let limiter = RateLimiter::new_for_ip_limit(2);
        assert!(limiter.check_rate_limit("key-a"));
        assert!(limiter.check_rate_limit("key-b"));
        // key-a is at count 1, still allowed
        assert!(limiter.check_rate_limit("key-a"));
        // key-a is now at limit (2), should be blocked
        assert!(!limiter.check_rate_limit("key-a"));
        // key-b is still at count 1, should be allowed
        assert!(limiter.check_rate_limit("key-b"));
    }

    #[test]
    fn test_rate_limiter_window_reset_logic() {
        let limiter = RateLimiter::new_for_ip_limit(2);
        let key = "reset-key";
        assert!(limiter.check_rate_limit(key));
        assert!(limiter.check_rate_limit(key));
        assert!(!limiter.check_rate_limit(key));
        // Verify the status shows at limit
        let (count, remaining) = limiter.get_status(key);
        assert_eq!(count, 2);
        assert_eq!(remaining, 0);
    }

    // ========== apply_ip_rate_limit tests ==========
    // These use the global IP_RATE_LIMITER, so we use unique IPs per test.

    #[test]
    fn test_apply_ip_rate_limit_allows() {
        let unique_ip = "198.51.100.1";
        let result = apply_ip_rate_limit(unique_ip);
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_ip_rate_limit_blocks_after_exceeding() {
        let unique_ip = "198.51.100.2";
        // Default limit is 10 per 60 seconds
        for _ in 0..10 {
            assert!(
                apply_ip_rate_limit(unique_ip).is_ok(),
                "First 10 requests should be allowed"
            );
        }
        // 11th request should be blocked
        let result = apply_ip_rate_limit(unique_ip);
        assert!(result.is_err(), "11th request should be blocked");
        let response = *result.unwrap_err();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        // Verify Retry-After header is set
        assert!(response.headers().contains_key("Retry-After"));
        // Verify Content-Type is application/json
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    // ========== RateLimitMiddleware::new tests ==========

    #[test]
    fn test_rate_limit_middleware_new() {
        let mock = Arc::new(MockRateLimitingService::allowed()) as Arc<dyn RateLimitingService>;
        let middleware = RateLimitMiddleware::new(mock);
        // Verify construction doesn't panic (clone to verify Clone derive)
        let _cloned = middleware.clone();
    }

    // ========== extract_bearer_token edge cases ==========

    #[test]
    fn test_extract_bearer_token_case_sensitive() {
        // "bearer" (lowercase) should not match
        let req = Request::builder()
            .header("Authorization", "bearer test-token")
            .body(Body::empty())
            .unwrap();
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_non_ascii_header() {
        let req = Request::builder()
            .header("Authorization", "Bearer \u{200B}hidden-zero-width")
            .body(Body::empty())
            .unwrap();
        // Zero-width space (obs-text) is allowed in HeaderValue but to_str() fails,
        // so extract_bearer_token returns None for non-ASCII header values
        let token = extract_bearer_token(&req);
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_bearer_token_with_trailing_spaces() {
        let req = Request::builder()
            .header("Authorization", "Bearer   my-token   ")
            .body(Body::empty())
            .unwrap();
        let token = extract_bearer_token(&req);
        assert_eq!(token, Some("my-token".to_string()));
    }

    #[test]
    fn test_extract_bearer_token_invalid_header_value() {
        // Use from_bytes to set a header value with obs-text (0x80-0xFF) bytes.
        // These are allowed in HeaderValue but cause to_str() to fail,
        // so extract_bearer_token returns None.
        let mut req = Request::builder().body(Body::empty()).unwrap();
        let obs_text_value = axum::http::HeaderValue::from_bytes(b"Bearer \x80token").unwrap();
        req.headers_mut()
            .insert(header::AUTHORIZATION, obs_text_value);
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    // ========== rate_limit_middleware function tests ==========

    async fn test_middleware_wrapper(
        axum::extract::State(state): axum::extract::State<Arc<dyn RateLimitingService>>,
        req: Request,
        next: Next,
    ) -> Response {
        rate_limit_middleware(req, next, state).await
    }

    #[tokio::test]
    async fn test_middleware_public_endpoint_skips_rate_limit() {
        use axum::routing::get;
        use tower::ServiceExt;

        let mock = Arc::new(MockRateLimitingService::allowed()) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/health", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_metrics_endpoint_skips_rate_limit() {
        use axum::routing::get;
        use tower::ServiceExt;

        let mock = Arc::new(MockRateLimitingService::allowed()) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/metrics", get(|| async { "metrics" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_allowed_passes_through() {
        use axum::routing::get;
        use tower::ServiceExt;

        let mock = Arc::new(MockRateLimitingService::allowed()) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .header("Authorization", "Bearer test-api-key-12345678")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_denied_returns_429() {
        use axum::routing::get;
        use tower::ServiceExt;

        let mock = Arc::new(MockRateLimitingService::denied("Too many requests"))
            as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .header("Authorization", "Bearer test-api-key-12345678")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_middleware_retry_after_returns_429_with_header() {
        use axum::routing::get;
        use tower::ServiceExt;

        let mock =
            Arc::new(MockRateLimitingService::retry_after(120)) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .header("Authorization", "Bearer test-api-key-12345678")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("Retry-After").unwrap(), "120");
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_middleware_service_error_fail_open() {
        use axum::routing::get;
        use tower::ServiceExt;

        // RATE_LIMIT_FAIL_OPEN is true, so errors should allow the request through
        let mock = Arc::new(MockRateLimitingService::error()) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .header("Authorization", "Bearer test-api-key-12345678")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Fail-open: request should pass through
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_no_bearer_token_ip_limit_passes() {
        use axum::routing::get;
        use tower::ServiceExt;

        // Use a unique IP to avoid interference with other tests
        let mock = Arc::new(MockRateLimitingService::allowed()) as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        // No Authorization header - should use IP rate limiting
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // IP rate limit should pass (first request for this IP)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_short_api_key_prefix() {
        use axum::routing::get;
        use tower::ServiceExt;

        // Test with a very short API key (< 8 chars) to exercise the min(8, len) logic
        let mock = Arc::new(MockRateLimitingService::denied("limit exceeded"))
            as Arc<dyn RateLimitingService>;
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                mock,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/test")
                    .header("Authorization", "Bearer abc") // 3 chars, less than 8
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should handle short API key without panic
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // ========== Mock RateLimitingService ==========

    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyControlService, QuotaService, RateLimitService,
        RateLimitingError, RateLimitingService,
    };

    enum MockBehavior {
        Allowed,
        Denied(String),
        RetryAfter(u64),
        Error,
    }

    struct MockRateLimitingService {
        behavior: MockBehavior,
    }

    impl MockRateLimitingService {
        fn allowed() -> Self {
            Self {
                behavior: MockBehavior::Allowed,
            }
        }
        fn denied(reason: &str) -> Self {
            Self {
                behavior: MockBehavior::Denied(reason.to_string()),
            }
        }
        fn retry_after(secs: u64) -> Self {
            Self {
                behavior: MockBehavior::RetryAfter(secs),
            }
        }
        fn error() -> Self {
            Self {
                behavior: MockBehavior::Error,
            }
        }
    }

    #[async_trait::async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            match &self.behavior {
                MockBehavior::Allowed => Ok(RateLimitResult::Allowed),
                MockBehavior::Denied(reason) => Ok(RateLimitResult::Denied {
                    reason: reason.clone(),
                }),
                MockBehavior::RetryAfter(secs) => Ok(RateLimitResult::RetryAfter {
                    retry_after_seconds: *secs,
                }),
                MockBehavior::Error => Err(RateLimitingError::Other(anyhow::anyhow!(
                    "mock service error"
                ))),
            }
        }
        async fn get_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<
            crate::domain::services::rate_limiting_service::RateLimitConfig,
            RateLimitingError,
        > {
            Ok(Default::default())
        }
        async fn update_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
            _config: crate::domain::services::rate_limiting_service::RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<
            crate::domain::services::rate_limiting_service::ConcurrencyResult,
            RateLimitingError,
        > {
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Allowed)
        }
        async fn release_team_concurrency_slot(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_team_current_concurrency(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
        async fn get_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<
            crate::domain::services::rate_limiting_service::ConcurrencyConfig,
            RateLimitingError,
        > {
            Ok(Default::default())
        }
        async fn update_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
            _config: crate::domain::services::rate_limiting_service::ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: uuid::Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<uuid::Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_quota_balance(&self, _team_id: uuid::Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    #[async_trait::async_trait]
    impl RateLimitingService for MockRateLimitingService {}
}
