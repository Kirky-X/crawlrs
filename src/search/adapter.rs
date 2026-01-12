// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::search::engine::{
    SearchEngine as DomainSearchEngine, SearchError as DomainSearchError,
};

use super::client::SearchClient;
use super::engine_trait::{SearchEngine as TraitSearchEngine, SearchRequest};

/// 域适配器 - 将新的 SearchEngine trait 适配到现有的 domain::search::engine::SearchEngine
pub struct SearchEngineAdapter {
    client: SearchClient,
}

impl SearchEngineAdapter {
    pub fn new(client: SearchClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DomainSearchEngine for SearchEngineAdapter {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<crate::domain::models::search_result::SearchResult>, DomainSearchError> {
        let result = self
            .client
            .search(query)
            .limit(limit)
            .execute()
            .await
            .map_err(|e| DomainSearchError::EngineError(e.to_string()))?;

        let search_results = result
            .items
            .into_iter()
            .map(|item| crate::domain::models::search_result::SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: format!("{:?}", item.engine),
                score: 0.0,
                published_time: None,
            })
            .collect();

        Ok(search_results)
    }

    fn name(&self) -> &'static str {
        "SearchEngineAdapter"
    }
}

/// 通用适配器 - 将 SearchEngine trait (src/search) 适配到 domain::search::engine::SearchEngine
pub struct GenericSearchEngineAdapter {
    engine: Arc<dyn TraitSearchEngine>,
}

impl GenericSearchEngineAdapter {
    pub fn new(engine: Arc<dyn TraitSearchEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl DomainSearchEngine for GenericSearchEngineAdapter {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<crate::domain::models::search_result::SearchResult>, DomainSearchError> {
        let request = SearchRequest::new(query).with_limit(limit);
        let result = self
            .engine
            .search(&request)
            .await
            .map_err(|e| DomainSearchError::EngineError(e.to_string()))?;

        let search_results = result
            .items
            .into_iter()
            .map(|item| crate::domain::models::search_result::SearchResult {
                title: item.title,
                url: item.url,
                description: Some(item.description),
                engine: format!("{:?}", item.engine),
                score: 0.0,
                published_time: None,
            })
            .collect();

        Ok(search_results)
    }

    fn name(&self) -> &'static str {
        self.engine.name()
    }
}
