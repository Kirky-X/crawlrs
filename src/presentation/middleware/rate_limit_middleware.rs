// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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

use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::infrastructure::security::secure_ip::{get_secure_client_ip, TrustedProxyConfig};
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};
use limiteron::sync::SyncFixedWindowLimiter;
use log::{debug, warn};
use std::sync::Arc;
use std::time::Duration;

/// Default rate limit for unauthenticated requests (requests per minute)
const DEFAULT_IP_RATE_LIMIT: u64 = 10;

/// Time window for IP rate limiting (seconds)
const IP_RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Rate limiting service fail-open behavior
///
/// When the rate limiting service encounters an error:
/// - If true: Allow the request to pass (fail open) - for availability
/// - If false: Reject the request with 503 Service Unavailable - for security (default)
///
/// Controlled via RATE_LIMIT_FAIL_OPEN environment variable at startup.
pub(crate) static RATE_LIMIT_FAIL_OPEN: once_cell::sync::Lazy<bool> =
    once_cell::sync::Lazy::new(|| {
        std::env::var("RATE_LIMIT_FAIL_OPEN")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
    });

/// 内存速率限制器（用于测试和 IP 限流）。
///
/// 2026-09 自研库收敛：窗口计数/淘汰交由 limiteron
/// [`SyncFixedWindowLimiter`]（per-identifier 固定窗口，拒绝不消耗预算，
/// 标识表满员自动清理过期条目），本结构体仅保留原 API 形状。
/// 可观察语义与旧手写版一致：limit=0 时首个请求放行（计数记 1）后续全拒。
#[derive(Clone)]
pub struct RateLimiter {
    /// limiteron 固定窗口限流器（线程安全，内部自动淘汰过期条目）
    inner: Arc<SyncFixedWindowLimiter>,
    /// 请求限制数（get_status 对未跟踪键的 remaining 展示值）
    limit: u64,
}

impl RateLimiter {
    /// 创建新的速率限制器
    pub fn new(limit: u64) -> Self {
        Self::with_window(limit, 60)
    }

    /// 创建用于 IP 限流的内存速率限制器
    pub fn new_for_ip_limit(limit: u64) -> Self {
        Self::with_window(limit, IP_RATE_LIMIT_WINDOW_SECS)
    }

    fn with_window(limit: u64, window_seconds: u64) -> Self {
        Self {
            // limit<1 抬升为 1：与旧实现 limit=0「首请求计数记 1、其后全拒」
            // 的可观察语义一致
            inner: Arc::new(SyncFixedWindowLimiter::new(
                limit.max(1),
                Duration::from_secs(window_seconds.max(1)),
            )),
            limit,
        }
    }

    /// 检查是否超过速率限制
    pub fn check_rate_limit(&self, key: &str) -> bool {
        self.inner.check(key).is_ok()
    }

    /// 获取当前计数和剩余配额
    pub fn get_status(&self, key: &str) -> (u64, u64) {
        self.inner.peek(key).unwrap_or((0, self.limit))
    }

    /// 清理过期的计数器。
    ///
    /// limiteron 在标识表满员时自动清除过期条目（`make_room`），无需手动 GC；
    /// 保留空实现以维持 API 兼容。
    pub fn cleanup_expired(&self) {}
}

/// 匿名/非 API 路径的 IP 速率限制器（全局单例，10 req/min）
static IP_RATE_LIMITER: once_cell::sync::Lazy<RateLimiter> =
    once_cell::sync::Lazy::new(|| RateLimiter::new_for_ip_limit(DEFAULT_IP_RATE_LIMIT));

/// 受保护 API 路径的每 IP 聚合限制器（600 req/min）
static AUTH_IP_RATE_LIMITER: once_cell::sync::Lazy<RateLimiter> =
    once_cell::sync::Lazy::new(|| RateLimiter::new_for_ip_limit(DEFAULT_AUTH_IP_RATE_LIMIT));

/// 速率限制中间件配置容器。
///
/// 结构体被构造并 Clone 到 axum 中间件栈中，字段由
/// `rate_limit_middleware` 异步函数通过 axum 的 `Extension`/`State` 提取使用，
/// 非直接方法调用，故需抑制 dead_code 警告。
#[derive(Clone)]
pub struct RateLimitMiddleware {
    /// 速率限制服务（由 axum middleware 函数提取，非直接读取）
    #[allow(dead_code)]
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
fn get_client_ip(req: &Request, trusted: &TrustedProxyConfig) -> String {
    get_secure_client_ip(req, trusted)
}

/// 对未认证请求应用基于 IP 的速率限制
///
/// # 参数
///
/// * `client_ip` - 客户端 IP 地址
///
/// # 返回值
///
/// 返回 `Ok()` 如果请求被允许，返回 `Err(Response)` 如果请求被拒绝
fn apply_ip_rate_limit(client_ip: &str) -> Result<(), Box<Response>> {
    apply_ip_rate_limit_for(
        &IP_RATE_LIMITER,
        client_ip,
        DEFAULT_IP_RATE_LIMIT,
        "anonymous",
    )
}

/// 指定限制器/阈值的 IP 限流（匿名 10/min 与 API 前缀 600/min 共用）。
fn apply_ip_rate_limit_for(
    limiter: &RateLimiter,
    client_ip: &str,
    limit: u64,
    category: &str,
) -> Result<(), Box<Response>> {
    if !limiter.check_rate_limit(client_ip) {
        let (current, _) = limiter.get_status(client_ip);
        warn!(
            "IP rate limit exceeded ({}) for {}: {}/{} requests in {} seconds",
            category, client_ip, current, limit, IP_RATE_LIMIT_WINDOW_SECS
        );

        let body = serde_json::json!({
            "error": "Rate limit exceeded",
            "message": format!(
                "Requests from this IP are limited to {} per minute ({}).",
                limit, category
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

    debug!(
        "IP rate limit check passed for {} ({})",
        client_ip, category
    );
    Ok(())
}

/// 限流中间件运行时配置（bootstrap 从 Settings 构造一次，层内共享）。
#[derive(Clone)]
pub struct RateLimitRuntimeConfig {
    /// `[rate_limiting].enabled`：false 时跳过 IP 限流（安全兜底可显式关闭）
    pub enabled: bool,
    /// `[trusted_proxies]`：可信代理判定（XFF 信任边界）
    pub trusted_proxies: TrustedProxyConfig,
}

impl Default for RateLimitRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trusted_proxies: TrustedProxyConfig::default(),
        }
    }
}

impl RateLimitRuntimeConfig {
    /// 从应用配置构造（bootstrap 调用）。
    pub fn from_settings(settings: &crate::config::settings::Settings) -> Self {
        Self {
            enabled: settings.rate_limiting.enabled,
            trusted_proxies: TrustedProxyConfig {
                enabled: settings.trusted_proxies.enabled,
                proxies: settings.trusted_proxies.proxies.clone(),
            },
        }
    }
}

/// 已认证 API 路径的每 IP 聚合上限（requests per minute）。
///
/// 认证后的 per-API-key 配额由各 handler 在认证之后按 `api_key_id` 执行；
/// 中间件阶段**无法验证 token 真伪**，因此不再按 Authorization 头做任何
/// 信任判断（审查 H1：任意 Bearer 串即可跳过 IP 限流并以原始 token 建桶）。
/// 改为凭证存在性分级：带非空 Bearer → 宽松 IP 兜底（600/min），无凭证 →
/// 严格匿名阈值（10/min）。token 内容不参与键、不落库、不写日志；无效
/// token 仍由 auth 中间件拒绝。
const DEFAULT_AUTH_IP_RATE_LIMIT: u64 = 600;

/// 请求是否携带非空 Bearer 凭证（只用于选择阈值，**不做任何真伪判断**）。
///
/// 携带凭证 → 600/min 的每 IP 上限（认证后的 per-key 配额由 handler 执行）；
/// 无凭证 → 10/min 严格匿名上限。凭证内容不进入限流键、不落库、不写日志，
/// 因而无法通过伪造 token 制造无界桶（审查 H1）。
fn has_bearer_credentials(req: &Request) -> bool {
    req.headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|token| !token.trim().is_empty())
        .unwrap_or(false)
}

/// 限流豁免判定：公开端点精确名单 + `/health/*` 前缀（K8s 探针）。
///
/// `/metrics` 保持免限流：Prometheus 常用 5s 抓取间隔，10/min 匿名桶会
/// 造成指标缺口；租户级敏感标签已移除（见 team_semaphore）。
fn is_rate_limit_exempt(path: &str) -> bool {
    PUBLIC_ENDPOINTS.contains(&path) || path.starts_with("/health/")
}

/// 速率限制中间件。
///
/// # 安全特性
///
/// 1. **IP 限流**：所有非公开请求都受 IP 限流；受保护 API 前缀 600/min，
///    其余（含未认证探测）10/min。不再信任 Authorization 头做免限流判断。
/// 2. **公开端点**：`/health`、`/health/*`、`/ready`、`/metrics`、`/v1/version`
///    不受限流影响（K8s 探针不会因 429 触发重启）。
/// 3. **per-key 配额**：认证后由 handler 按 `api_key_id` 调用
///    `RateLimitingService` 执行（本中间件不接触原始 token）。
pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
    config: &RateLimitRuntimeConfig,
) -> Response {
    let path = req.uri().path();

    if !config.enabled {
        debug!("Rate limiting disabled by [rate_limiting].enabled=false");
        return next.run(req).await;
    }

    if is_rate_limit_exempt(path) {
        debug!("Public endpoint {}, skipping rate limit", path);
        return next.run(req).await;
    }

    let client_ip = get_client_ip(&req, &config.trusted_proxies);
    let result = if has_bearer_credentials(&req) {
        apply_ip_rate_limit_for(
            &AUTH_IP_RATE_LIMITER,
            &client_ip,
            DEFAULT_AUTH_IP_RATE_LIMIT,
            "bearer-credentials",
        )
    } else {
        apply_ip_rate_limit(&client_ip)
    };

    match result {
        Ok(()) => next.run(req).await,
        Err(response) => *response,
    }
}

/// axum `from_fn_with_state` 兼容入口（D1 修复：生产路由栈挂载点）。
///
/// 此前 [`RateLimitMiddleware::new`] 仅以 `Extension` 注入、生产路由从未
/// `.layer(from_fn(...))` 挂载——`rate_limiting.enabled=true / 60rpm / burst20`
/// 全部形同虚设（E2E SG-09：40 并发连打零 429）。本包装由
/// `bootstrap/routes.rs::apply_global_api_layers` 以
/// `axum::middleware::from_fn_with_state` 挂载到全量 API 路由。
pub async fn rate_limit_layer(
    axum::extract::State(config): axum::extract::State<Arc<RateLimitRuntimeConfig>>,
    req: Request,
    next: Next,
) -> Response {
    rate_limit_middleware(req, next, &config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

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

        let ip = get_client_ip(&req, &TrustedProxyConfig::default());
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

        let ip = get_client_ip(&req, &TrustedProxyConfig::default());
        // 安全版本会返回 "unknown" 因为没有 SocketAddr 扩展
        assert_eq!(ip, "unknown");
    }

    #[test]
    fn test_get_client_ip_unknown() {
        let req = Request::builder().body(Body::empty()).unwrap();

        let ip = get_client_ip(&req, &TrustedProxyConfig::default());
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

        let ip = get_client_ip(&req, &TrustedProxyConfig::default());
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

        let ip = get_client_ip(&req, &TrustedProxyConfig::default());
        // 来自不可信代理，应该使用直接连接的 IP
        assert_eq!(ip, "8.8.8.8");
    }

    // ========== RateLimiter::new tests ==========

    #[test]
    fn test_rate_limiter_new() {
        let limiter = RateLimiter::new(100);
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

    // ========== rate_limit_middleware function tests ==========

    async fn test_middleware_wrapper(
        axum::extract::State(state): axum::extract::State<Arc<RateLimitRuntimeConfig>>,
        req: Request,
        next: Next,
    ) -> Response {
        rate_limit_middleware(req, next, &state).await
    }

    #[test]
    fn test_is_rate_limit_exempt_paths() {
        // K8s 探针：/health/live、/health/ready 必须免限流（审查 H2）
        assert!(is_rate_limit_exempt("/health"));
        assert!(is_rate_limit_exempt("/health/live"));
        assert!(is_rate_limit_exempt("/health/ready"));
        assert!(is_rate_limit_exempt("/ready"));
        assert!(is_rate_limit_exempt("/v1/version"));
        assert!(is_rate_limit_exempt("/metrics"));
        assert!(!is_rate_limit_exempt("/v1/scrape"));
    }

    #[tokio::test]
    async fn test_middleware_health_live_skips_rate_limit() {
        use axum::routing::get;
        use tower::ServiceExt;

        let state = Arc::new(RateLimitRuntimeConfig::default());
        let app = axum::Router::new()
            .route("/health/live", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                state,
                test_middleware_wrapper,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_endpoint_skips_rate_limit() {
        use axum::routing::get;
        use tower::ServiceExt;

        let state = Arc::new(RateLimitRuntimeConfig::default());
        let app = axum::Router::new()
            .route("/health", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                state,
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

        let state = Arc::new(RateLimitRuntimeConfig::default());
        let app = axum::Router::new()
            .route("/metrics", get(|| async { "metrics" }))
            .layer(axum::middleware::from_fn_with_state(
                state,
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
    async fn test_middleware_no_bearer_token_ip_limit_passes() {
        use axum::routing::get;
        use tower::ServiceExt;

        // Use a unique IP to avoid interference with other tests
        let state = Arc::new(RateLimitRuntimeConfig::default());
        let app = axum::Router::new()
            .route("/v1/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(
                state,
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
}
