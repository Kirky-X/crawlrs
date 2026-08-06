// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T043: 统一 mock 引擎实现

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use crawlrs::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};

/// Configurable mock scraper engine for tests.
///
/// - `name`: engine name (default `"mock"`)
/// - `score`: support_score return value (default `50`)
/// - `response`: optional fixed response to return from `scrape()`
/// - `error`: optional fixed error to return from `scrape()`
/// - `mrt`: max response time (default 30s)
pub struct MockScraperEngine {
    pub name: &'static str,
    pub score: u8,
    pub response: Option<InternalScrapeResponse>,
    pub error: Option<EngineError>,
    pub mrt: Duration,
}

impl MockScraperEngine {
    /// Create a basic mock engine with default settings.
    pub fn new() -> Self {
        Self {
            name: "mock",
            score: 50,
            response: Some(InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>mock</body></html>".to_string(),
                headers: vec![],
                url: String::new(),
            }),
            error: None,
            mrt: Duration::from_secs(30),
        }
    }

    /// Create with a specific name and support score.
    pub fn with_name_and_score(name: &'static str, score: u8) -> Self {
        Self {
            name,
            score,
            ..Self::new()
        }
    }

    /// Wrap in Arc for use in engine lists.
    pub fn arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Default for MockScraperEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ScraperEngine for MockScraperEngine {
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        if let Some(ref resp) = self.response {
            let mut response = resp.clone();
            response.url = request.url.to_string();
            return Ok(response);
        }
        Err(EngineError::AllEnginesFailed(
            "MockScraperEngine: no response configured".to_string(),
        ))
    }

    fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
        self.score
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn max_response_time(&self) -> Duration {
        self.mrt
    }
}
