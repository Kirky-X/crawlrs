// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search module for Shaku dependency injection.
//!
//! This module provides Shaku components for search layer dependencies
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

    fn register_engine(&self, engine: Arc<dyn crate::search::engine_trait::SearchEngine>) {
        // This would require interior mutability, skipping for now
        let _ = engine;
    }
}

// Search module components - for Shaku DI
