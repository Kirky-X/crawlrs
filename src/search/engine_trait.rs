// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
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
    pub lang: Option<String>,
    pub country: Option<String>,
}

impl SearchRequest {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_string(),
            engine: None,
            limit: 10,
            offset: 0,
            lang: None,
            country: None,
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

    pub fn with_lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_string());
        self
    }

    pub fn with_country(mut self, country: &str) -> Self {
        self.country = Some(country.to_string());
        self
    }
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self::new("")
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
        engine: Option<&str>,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let mut request = SearchRequest::new(query).with_limit(limit);
        if let Some(engine_name) = engine {
            if let Some(engine_type) = SearchEngineType::from_name(engine_name) {
                request = request.with_engine(engine_type);
            }
        }
        let response = self.search(&request).await?;
        Ok(response.items)
    }
}
