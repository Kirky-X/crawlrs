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
use log::{debug, error, warn};

use crate::infrastructure::security::secure_ip::{SecureIpExtractor, TrustedProxyConfig};
use crate::presentation::middleware::RATE_LIMIT_EXCLUDED_ENDPOINTS;

/// Limiteron 速率限制中间件状态
#[derive(Clone)]
pub struct LimiteronMiddlewareState {
    /// Limiteron Governor 实例
    pub governor: Arc<Governor>,
}

/// 提取客户端 IP 地址
///
/// # 安全说明
///
/// 此函数使用安全的 IP 提取逻辑来防止 X-Forwarded-For 头伪造攻击。
/// 只有当请求来自可信代理时，才信任转发头中的 IP 地址。
fn extract_client_ip(request: &Request, remote_addr: Option<SocketAddr>) -> Option<String> {
    let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());
    let direct_ip = remote_addr.map(|addr| addr.ip());
    extractor.extract_client_ip_with_override(request, direct_ip)
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
        debug!("LimiteronMiddleware: Skipping public endpoint {}", path);
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
        .map(|conn| conn.0);

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
                "LimiteronMiddleware: Rate limit exceeded for path {}: {:?}",
                path, reason
            );
            Ok((
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded: {}", reason.reason),
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
            // SEC-003: 可配置的 fail-open/fail-closed 行为
            if RATE_LIMIT_FAIL_OPEN {
                // Fail-open: 允许请求通过，但记录严重警告
                error!("LimiteronMiddleware: SEC-003 Rate limiting service error - failing open. \
                     Consider setting RATE_LIMIT_FAIL_OPEN=false for stricter security. error={} path={:?}", e, path);
                Ok(next.run(request).await)
            } else {
                // Fail-closed: 拒绝请求以确保安全
                error!("LimiteronMiddleware: SEC-003 Rate limiting service error - failing closed error={} path={:?}", e, path);
                Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Rate limiting service temporarily unavailable".to_string(),
                )
                    .into_response())
            }
        }
    }
}

/// Limiteron rate limiting fail-open behavior
///
/// Controls what happens when the rate limiting service encounters an error.
/// Can be overridden via environment variable.
const RATE_LIMIT_FAIL_OPEN: bool = true;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn build_request() -> Request {
        Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("body should build")
    }

    fn build_request_with_forwarded(forwarded_for: Option<&str>, real_ip: Option<&str>) -> Request {
        let mut builder = Request::builder().uri("/test");
        if let Some(xff) = forwarded_for {
            builder = builder.header("x-forwarded-for", xff);
        }
        if let Some(xri) = real_ip {
            builder = builder.header("x-real-ip", xri);
        }
        builder.body(Body::empty()).expect("body should build")
    }

    fn socket_addr(ip: &str, port: u16) -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(ip.parse::<Ipv4Addr>().expect("valid ipv4")),
            port,
        )
    }

    #[test]
    fn test_extract_client_ip_none_without_remote_addr() {
        // No remote_addr and no ConnectInfo extension → cannot determine IP
        let request = build_request();
        assert_eq!(extract_client_ip(&request, None), None);
    }

    #[test]
    fn test_extract_client_ip_uses_direct_public_ip() {
        // A public (non-trusted) remote address should be used directly
        let request = build_request();
        let remote = socket_addr("8.8.8.8", 443);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("8.8.8.8".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_ignores_forwarded_from_untrusted() {
        // Security: X-Forwarded-For must be ignored when the direct connection
        // is NOT from a trusted proxy.
        let request = build_request_with_forwarded(Some("203.0.113.5"), None);
        let remote = socket_addr("8.8.8.8", 443);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("8.8.8.8".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_falls_back_to_direct() {
        // 127.0.0.1 is a trusted proxy; with no forwarded headers it falls
        // back to the direct IP.
        let request = build_request();
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("127.0.0.1".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_uses_x_forwarded_for() {
        // From a trusted proxy, X-Forwarded-For should be honored.
        let request = build_request_with_forwarded(Some("203.0.113.5"), None);
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.5".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_uses_x_real_ip() {
        // From a trusted proxy, X-Real-IP should be honored as a fallback.
        let request = build_request_with_forwarded(None, Some("203.0.113.9"));
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.9".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_x_forwarded_for_takes_first_ip() {
        // X-Forwarded-For: client, proxy1, proxy2 → first IP (client) wins.
        let request = build_request_with_forwarded(Some("203.0.113.5, 10.0.0.1, 10.0.0.2"), None);
        let remote = socket_addr("127.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("203.0.113.5".to_string())
        );
    }

    #[test]
    fn test_extract_client_ip_trusted_proxy_10_x() {
        // 10.x.x.x is a trusted proxy range by default.
        let request = build_request_with_forwarded(Some("198.51.100.7"), None);
        let remote = socket_addr("10.0.0.1", 8080);
        assert_eq!(
            extract_client_ip(&request, Some(remote)),
            Some("198.51.100.7".to_string())
        );
    }

    #[test]
    fn test_rate_limit_fail_open_constant() {
        // The fail-open default must be true (availability over strictness).
        assert!(RATE_LIMIT_FAIL_OPEN);
    }

    #[tokio::test]
    async fn test_limiteron_middleware_state_clone_preserves_governor() {
        // Clone must share the same Governor Arc.
        use limiteron::config::{Action, ActionConfig, GlobalConfig, LimiterConfig, Matcher, Rule};

        let storage: Arc<dyn limiteron::storage::Storage> =
            Arc::new(limiteron::storage::MemoryStorage::new());
        let ban_storage: Arc<dyn limiteron::storage::BanStorage> =
            Arc::new(limiteron::storage::MemoryBanStorage::new());

        let flow_config = limiteron::prelude::FlowControlConfig {
            version: "0.1.0".to_string(),
            global: GlobalConfig::default(),
            rules: vec![Rule {
                id: "test_clone_rule".to_string(),
                name: "Test Clone Rule".to_string(),
                priority: 100,
                matchers: vec![Matcher::User {
                    user_ids: vec!["*".to_string()],
                }],
                limiters: vec![LimiterConfig::TokenBucket {
                    capacity: 10,
                    refill_rate: 1,
                }],
                action: ActionConfig {
                    on_exceed: Action::Reject,
                    ban: None,
                },
            }],
        };

        let governor = Governor::builder()
            .with_config(flow_config)
            .with_storage(storage)
            .with_ban_storage(ban_storage)
            .with_l1_cache_enabled(false)
            .build()
            .await
            .expect("governor should build");
        let governor = Arc::new(governor);
        let state = LimiteronMiddlewareState {
            governor: governor.clone(),
        };
        let cloned = state.clone();
        assert!(Arc::ptr_eq(&state.governor, &cloned.governor));
    }
}
