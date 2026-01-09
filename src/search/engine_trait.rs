// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;

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
    fn get_name(&self) -> &'static str;
    fn engine_type(&self) -> SearchEngineType;
    fn health(&self) -> EngineHealth;
    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError>;
}

use super::response::ResponseItem;
use super::types::EngineHealth;
