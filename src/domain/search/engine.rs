// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum SearchError {
    #[error("Search engine error: {0}")]
    EngineError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Timeout")]
    Timeout,
}

#[async_trait]
pub trait SearchEngine: Send + Sync {
    /// Perform a search query
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError>;

    /// Get the name of the search engine
    fn name(&self) -> &'static str;
}
