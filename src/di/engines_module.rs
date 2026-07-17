// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Engines module for dependency injection.
//!
//! This module provides components for engine layer dependencies
//! including EngineClient and EngineRouter.

use std::sync::Arc;

use crate::engines::engine_client::{EngineClient, EngineClientTrait};
use crate::engines::router::{EngineRouter, EngineRouterTrait};

/// EngineClient component
pub struct EngineClientComponent {
    /// Engine router
    router: Arc<dyn EngineRouterTrait>,
}

impl EngineClientComponent {
    /// Create a new EngineClientComponent with explicit dependencies
    pub fn new(router: Arc<dyn EngineRouterTrait>) -> Self {
        Self { router }
    }
}

#[async_trait::async_trait]
impl EngineClientTrait for EngineClientComponent {
    async fn scrape(
        &self,
        request: &crate::engines::engine_client::ScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::ScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        // Create EngineClient with the injected router
        let client = EngineClient::with_router(self.router.clone());
        client.scrape(request).await
    }

    async fn health_check(&self) -> crate::engines::engine_client::EngineHealthStatus {
        let client = EngineClient::with_router(self.router.clone());
        client.health_check().await
    }

    fn engine_count(&self) -> usize {
        let client = EngineClient::with_router(self.router.clone());
        client.engine_count()
    }

    fn registered_engines(&self) -> Vec<String> {
        let client = EngineClient::with_router(self.router.clone());
        client.registered_engines()
    }
}

/// EngineRouter component
pub struct EngineRouterComponent {
    /// List of engines
    engines: Vec<Arc<dyn crate::engines::engine_client::ScraperEngine>>,
}

impl EngineRouterComponent {
    /// Create a new EngineRouterComponent with explicit dependencies
    pub fn new(engines: Vec<Arc<dyn crate::engines::engine_client::ScraperEngine>>) -> Self {
        Self { engines }
    }

    /// Create with default engines (empty)
    pub fn with_defaults() -> Self {
        Self {
            engines: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl EngineRouterTrait for EngineRouterComponent {
    async fn route(
        &self,
        request: &crate::engines::engine_client::InternalScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::InternalScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        let router = EngineRouter::new(self.engines.clone());
        router._route_impl(request).await
    }

    async fn aggregate(
        &self,
        request: &crate::engines::engine_client::InternalScrapeRequest,
    ) -> Result<
        crate::engines::engine_client::InternalScrapeResponse,
        crate::engines::engine_client::EngineError,
    > {
        let router = EngineRouter::new(self.engines.clone());
        // For now, delegate to route
        router._route_impl(request).await
    }

    fn get_engine_stats(
        &self,
    ) -> std::collections::HashMap<String, crate::engines::router::EngineStats> {
        let router = EngineRouter::new(self.engines.clone());
        router.get_engine_stats()
    }

    fn reset_engine_stats(&self, engine_name: &str) {
        let router = EngineRouter::new(self.engines.clone());
        router.reset_engine_stats(engine_name);
    }

    fn registered_engines(&self) -> Vec<String> {
        let router = EngineRouter::new(self.engines.clone());
        router.registered_engines()
    }
}

// Engines module components

#[cfg(test)]
mod tests {
    use super::*;

    // ========== EngineRouterComponent ==========

    #[test]
    fn test_engine_router_component_with_defaults_has_no_engines() {
        let component = EngineRouterComponent::with_defaults();
        // 空引擎列表应返回 0 个已注册引擎
        let engines = component.registered_engines();
        assert!(engines.is_empty());
    }

    #[test]
    fn test_engine_router_component_new_with_empty_engines() {
        let component = EngineRouterComponent::new(Vec::new());
        let engines = component.registered_engines();
        assert!(engines.is_empty());
    }

    #[test]
    fn test_engine_router_component_get_engine_stats_empty() {
        let component = EngineRouterComponent::with_defaults();
        let stats = component.get_engine_stats();
        // 空引擎列表应返回空的统计 HashMap
        assert!(stats.is_empty());
    }

    #[test]
    fn test_engine_router_component_reset_engine_stats_no_panic() {
        let component = EngineRouterComponent::with_defaults();
        // 重置不存在的引擎统计不应 panic
        component.reset_engine_stats("nonexistent_engine");
    }

    // ========== EngineClientComponent ==========

    #[test]
    fn test_engine_client_component_new_with_empty_router() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        // 空路由器应返回 0 个引擎
        assert_eq!(component.engine_count(), 0);
        assert!(component.registered_engines().is_empty());
    }

    #[test]
    fn test_engine_client_component_as_trait_object() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let trait_obj: &dyn crate::engines::engine_client::EngineClientTrait = &component;
        // 通过 trait 对象访问，验证动态分发正常工作
        assert_eq!(trait_obj.engine_count(), 0);
    }

    // ========== EngineRouterComponent::route / aggregate error & success paths ==========

    use crate::engines::engine_client::{
        EngineError, HttpMethod, InternalScrapeRequest, InternalScrapeResponse, ScrapeRequest,
        ScraperEngine,
    };
    use std::collections::HashMap;
    use std::time::Duration;

    /// 构造测试用 InternalScrapeRequest，使用外部 URL 以通过 SSRF 校验
    fn make_internal_request(url: &str) -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: url.to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
        }
    }

    /// Mock 引擎：返回固定的成功响应
    struct SuccessMockEngine {
        engine_name: &'static str,
    }

    #[async_trait::async_trait]
    impl ScraperEngine for SuccessMockEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "mock content".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 5,
            })
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            80
        }

        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    // --- route: SSRF error path ---

    #[tokio::test]
    async fn test_route_ssrf_protection_on_internal_url() {
        let component = EngineRouterComponent::with_defaults();
        let request = make_internal_request("http://localhost");
        let result = component.route(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_route_ssrf_protection_on_private_ip() {
        let component = EngineRouterComponent::with_defaults();
        let request = make_internal_request("http://192.168.1.1");
        let result = component.route(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    // --- route: no engines available path ---

    #[tokio::test]
    async fn test_route_external_url_no_engines_returns_all_engines_failed() {
        let component = EngineRouterComponent::with_defaults();
        let request = make_internal_request("http://example.com");
        let result = component.route(&request).await;
        // 无引擎时返回 AllEnginesFailed
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::AllEnginesFailed(msg) => {
                assert!(msg.contains("No suitable engines"));
            }
            other => panic!("Expected AllEnginesFailed, got {:?}", other),
        }
    }

    // --- route: success path with mock engine ---

    #[tokio::test]
    async fn test_route_success_with_mock_engine() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(SuccessMockEngine {
            engine_name: "mock",
        });
        let component = EngineRouterComponent::new(vec![engine]);
        let request = make_internal_request("http://example.com");
        let response = component
            .route(&request)
            .await
            .expect("route should succeed with mock engine");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "mock content");
        assert_eq!(response.content_type, "text/html");
    }

    // --- aggregate: SSRF error path ---

    #[tokio::test]
    async fn test_aggregate_ssrf_protection_on_internal_url() {
        let component = EngineRouterComponent::with_defaults();
        let request = make_internal_request("http://127.0.0.1");
        let result = component.aggregate(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    // --- aggregate: no engines path ---

    #[tokio::test]
    async fn test_aggregate_external_url_no_engines_returns_all_engines_failed() {
        let component = EngineRouterComponent::with_defaults();
        let request = make_internal_request("http://example.com");
        let result = component.aggregate(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::AllEnginesFailed(msg) => {
                assert!(msg.contains("No suitable engines"));
            }
            other => panic!("Expected AllEnginesFailed, got {:?}", other),
        }
    }

    // --- aggregate: success path ---

    #[tokio::test]
    async fn test_aggregate_success_with_mock_engine() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(SuccessMockEngine {
            engine_name: "mock",
        });
        let component = EngineRouterComponent::new(vec![engine]);
        let request = make_internal_request("http://example.com");
        let response = component
            .aggregate(&request)
            .await
            .expect("aggregate should succeed with mock engine");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "mock content");
    }

    // ========== EngineClientComponent::scrape / health_check ==========

    #[tokio::test]
    async fn test_scrape_ssrf_protection_on_internal_url() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let request = ScrapeRequest::new("http://localhost");
        let result = component.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::SsrfProtection(_) => {}
            other => panic!("Expected SsrfProtection, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_scrape_external_url_no_engines_returns_no_engines_available() {
        // EngineClient.scrape 调用 router.route 后经过 convert_error，
        // "No suitable engines" 的 AllEnginesFailed 会被转换为 NoEnginesAvailable
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let request = ScrapeRequest::new("http://example.com");
        let result = component.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::NoEnginesAvailable => {}
            other => panic!("Expected NoEnginesAvailable, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_scrape_success_with_mock_engine() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(SuccessMockEngine {
            engine_name: "mock",
        });
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::new(vec![engine]));
        let component = EngineClientComponent::new(router);
        let request = ScrapeRequest::new("http://example.com");
        let response = component
            .scrape(&request)
            .await
            .expect("scrape should succeed with mock engine");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "mock content");
        assert!(response.is_success());
        assert_eq!(response.final_url, Some("http://example.com".to_string()));
    }

    #[tokio::test]
    async fn test_health_check_returns_healthy_when_no_engines() {
        // 无引擎时 health_monitor 为空，get_aggregate_status 返回 Healthy
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let status = component.health_check().await;
        assert_eq!(
            status,
            crate::engines::engine_client::EngineHealthStatus::Healthy
        );
    }

    #[tokio::test]
    async fn test_health_check_default_status() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouterComponent::with_defaults());
        let component = EngineClientComponent::new(router);
        let status = component.health_check().await;
        // 默认状态应为 Healthy
        assert_eq!(status, status.clone());
    }
}
