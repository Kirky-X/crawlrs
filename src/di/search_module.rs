// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search module for dependency injection.
//!
//! This module provides components for search layer dependencies
//! including SearchClient, SearchAggregator, and individual search engine implementations.

use std::sync::Arc;

use crate::engines::engine_client::EngineClient;
use crate::search::aggregator::SearchAggregator;
use crate::search::client::{SearchClient, SearchClientTrait};

/// Trait for HttpClient component
pub trait HttpClientTrait: Send + Sync {
    fn get_client(&self) -> Arc<reqwest::Client>;
}

/// HttpClient component for unified HTTP client management
pub struct HttpClientComponent {
    /// The HTTP client
    client: Arc<reqwest::Client>,
}

impl HttpClientComponent {
    /// Create a new HttpClientComponent with explicit dependencies
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self { client }
    }
}

impl HttpClientTrait for HttpClientComponent {
    fn get_client(&self) -> Arc<reqwest::Client> {
        self.client.clone()
    }
}

/// Trait for EngineClient component
pub trait EngineClientTrait: Send + Sync {
    fn get_client(&self) -> Arc<EngineClient>;
}

/// EngineClient component for unified engine client management
pub struct EngineClientComponent {
    /// The engine client
    client: Arc<EngineClient>,
}

impl EngineClientComponent {
    /// Create a new EngineClientComponent with explicit dependencies
    pub fn new(client: Arc<EngineClient>) -> Self {
        Self { client }
    }
}

impl EngineClientTrait for EngineClientComponent {
    fn get_client(&self) -> Arc<EngineClient> {
        self.client.clone()
    }
}

/// Trait for SearchAggregator component
pub trait SearchAggregatorTrait: Send + Sync {
    fn get_aggregator(&self) -> &SearchAggregator;
}

/// SearchAggregator component
pub struct SearchAggregatorComponent {
    /// Search aggregator
    aggregator: Arc<SearchAggregator>,
}

impl SearchAggregatorComponent {
    /// Create a new SearchAggregatorComponent with explicit dependencies
    pub fn new(aggregator: Arc<SearchAggregator>) -> Self {
        Self { aggregator }
    }
}

impl SearchAggregatorTrait for SearchAggregatorComponent {
    fn get_aggregator(&self) -> &SearchAggregator {
        &self.aggregator
    }
}

/// SearchClient component
#[allow(dead_code)]
pub struct SearchClientComponent {
    /// Search client
    client: Arc<SearchClient>,
}

impl SearchClientComponent {
    /// Create a new SearchClientComponent with explicit dependencies
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            client: Arc::new(SearchClient::new(engine_client)),
        }
    }
}

#[async_trait::async_trait]
impl SearchClientTrait for SearchClientComponent {
    async fn search(&self, query: &str) -> crate::search::client::SearchCommand {
        crate::search::client::SearchCommand::new((*self.client).clone(), query)
    }

    async fn search_with_engine(
        &self,
        query: &str,
        engine: crate::search::types::SearchEngineType,
    ) -> Result<
        crate::search::response::Response<crate::search::response::ResponseItem>,
        crate::search::error::SearchError,
    > {
        self.client.search_with_engine(query, engine).await
    }

    fn default_engine(&self) -> crate::search::types::SearchEngineType {
        self.client.default_engine()
    }
}

// Search module components

#[cfg(test)]
mod tests {
    use super::*;

    // ========== HttpClientComponent ==========

    #[test]
    fn test_http_client_component_new_stores_client() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let retrieved = component.get_client();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    #[test]
    fn test_http_client_component_get_returns_clone() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let first = component.get_client();
        let second = component.get_client();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &client));
    }

    #[test]
    fn test_http_client_component_as_trait_object() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let trait_obj: &dyn HttpClientTrait = &component;
        let retrieved = trait_obj.get_client();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    // ========== EngineClientComponent ==========

    #[test]
    fn test_engine_client_component_new_stores_client() {
        let engine_client = Arc::new(EngineClient::new());
        let component = EngineClientComponent::new(engine_client.clone());
        let retrieved = component.get_client();
        assert!(Arc::ptr_eq(&retrieved, &engine_client));
    }

    #[test]
    fn test_engine_client_component_get_returns_clone() {
        let engine_client = Arc::new(EngineClient::new());
        let component = EngineClientComponent::new(engine_client.clone());
        let first = component.get_client();
        let second = component.get_client();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &engine_client));
    }

    #[test]
    fn test_engine_client_component_as_trait_object() {
        let engine_client = Arc::new(EngineClient::new());
        let component = EngineClientComponent::new(engine_client.clone());
        let trait_obj: &dyn EngineClientTrait = &component;
        let retrieved = trait_obj.get_client();
        assert!(Arc::ptr_eq(&retrieved, &engine_client));
    }

    // ========== SearchAggregatorComponent ==========

    #[test]
    fn test_search_aggregator_component_new_stores_aggregator() {
        let aggregator = Arc::new(SearchAggregator::new(Vec::new(), 5000));
        let component = SearchAggregatorComponent::new(aggregator.clone());
        let retrieved = component.get_aggregator();
        // get_aggregator() 返回 &SearchAggregator，应指向 Arc 内部的同一对象
        let aggregator_ptr = Arc::as_ptr(&aggregator) as *const SearchAggregator;
        let retrieved_ptr = retrieved as *const SearchAggregator;
        assert_eq!(aggregator_ptr, retrieved_ptr);
    }

    #[test]
    fn test_search_aggregator_component_as_trait_object() {
        let aggregator = Arc::new(SearchAggregator::new(Vec::new(), 3000));
        let component = SearchAggregatorComponent::new(aggregator.clone());
        let trait_obj: &dyn SearchAggregatorTrait = &component;
        let retrieved = trait_obj.get_aggregator();
        let aggregator_ptr = Arc::as_ptr(&aggregator) as *const SearchAggregator;
        let retrieved_ptr = retrieved as *const SearchAggregator;
        assert_eq!(aggregator_ptr, retrieved_ptr);
    }

    // ========== SearchClientComponent ==========

    #[test]
    fn test_search_client_component_new() {
        let engine_client = Arc::new(EngineClient::new());
        let component = SearchClientComponent::new(engine_client);
        // 构造成功即可验证
        let _trait_obj: &dyn SearchClientTrait = &component;
    }

    #[tokio::test]
    async fn test_search_client_component_default_engine() {
        let engine_client = Arc::new(EngineClient::new());
        let component = SearchClientComponent::new(engine_client);
        // default_engine() 应返回一个有效的搜索引擎类型
        let engine = component.default_engine();
        let _ = engine; // 验证方法可调用
    }

    #[test]
    fn test_search_client_component_as_trait_object() {
        let engine_client = Arc::new(EngineClient::new());
        let component = SearchClientComponent::new(engine_client);
        let trait_obj: &dyn SearchClientTrait = &component;
        // 通过 trait 对象访问，验证动态分发正常工作
        let _engine = trait_obj.default_engine();
    }
}
