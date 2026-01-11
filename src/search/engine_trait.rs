// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;

use super::response::ResponseItem;
use super::types::EngineHealth;
use super::{error::SearchError, response::Response, types::SearchEngineType};

/// 搜索请求
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub engine: Option<SearchEngineType>,
    pub limit: u32,
    pub offset: u32,
}

impl SearchRequest {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            engine: None,
            limit: 10,
            offset: 0,
        }
    }

    pub fn with_engine(mut self, engine: SearchEngineType) -> Self {
        self.engine = Some(engine);
        self
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }
}

/// 搜索引擎 trait
#[async_trait]
pub trait SearchEngine: Send + Sync {
    fn name(&self) -> &'static str;
    fn engine_type(&self) -> SearchEngineType;
    fn health(&self) -> EngineHealth;
    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError>;

    /// Search with a specific engine (if engine is None, search all engines)
    /// Default implementation: searches without specific engine
    async fn search_with_engine(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        _engine: Option<&str>,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let request = SearchRequest::new(query).with_limit(limit);
        let response = self.search(&request).await?;
        Ok(response.items)
    }
}
