// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::search::engine::{
    SearchEngine as DomainSearchEngine, SearchError as DomainSearchError,
};

use super::client::SearchClient;
use super::types::SearchEngineType;

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
    ) -> Result<Vec<super::super::domain::models::search_result::SearchResult>, DomainSearchError>
    {
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
            .map(
                |item| super::super::domain::models::search_result::SearchResult {
                    title: item.title,
                    url: item.url,
                    description: Some(item.description),
                    engine: item.engine.name().to_string(),
                    score: 0.0,
                    published_time: None,
                },
            )
            .collect();

        Ok(search_results)
    }

    fn name(&self) -> &'static str {
        "SearchClientAdapter"
    }

    async fn search_with_engine(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        engine: Option<&str>,
    ) -> Result<Vec<super::super::domain::models::search_result::SearchResult>, DomainSearchError>
    {
        let result = if let Some(e) = engine {
            let engine_type = SearchEngineType::from_name(e)
                .ok_or_else(|| DomainSearchError::EngineError(format!("Unknown engine: {}", e)))?;
            self.client.search_with_engine(query, engine_type).await
        } else {
            self.client.search(query).limit(limit).execute().await
        }
        .map_err(|e| DomainSearchError::EngineError(e.to_string()))?;

        let search_results = result
            .items
            .into_iter()
            .map(
                |item| super::super::domain::models::search_result::SearchResult {
                    title: item.title,
                    url: item.url,
                    description: Some(item.description),
                    engine: item.engine.name().to_string(),
                    score: 0.0,
                    published_time: None,
                },
            )
            .collect();

        Ok(search_results)
    }
}

/// 创建适配器的便捷函数
pub fn create_domain_adapter() -> Arc<dyn DomainSearchEngine> {
    let client = SearchClient::global().clone();
    Arc::new(SearchEngineAdapter::new(client))
}
