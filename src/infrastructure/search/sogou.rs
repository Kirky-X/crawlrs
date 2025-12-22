// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use async_trait::async_trait;
use scraper::{Html, Selector};

pub struct SogouSearchEngine {
    client: reqwest::Client,
}

impl Default for SogouSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SogouSearchEngine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .build()
            .unwrap_or_default();

        Self { client }
    }

    /// 解析搜狗搜索HTML结果（用于单元测试）
    pub fn parse_search_results(
        &self,
        html_content: &str,
        query: &str,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let document = Html::parse_document(html_content);
        let result_selector = Selector::parse(".vrwrap, .rb").unwrap();
        let title_selector = Selector::parse("h3").unwrap();
        let link_selector = Selector::parse("h3 > a").unwrap();

        let mut results = Vec::new();

        // Create relevance scorer for this query
        let scorer = RelevanceScorer::new(query);

        for element in document.select(&result_selector) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();

            let url = element
                .select(&link_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();

            if !title.is_empty() && !url.is_empty() {
                let mut res =
                    SearchResult::new(title.clone(), url.clone(), None, "sogou".to_string());

                // Calculate relevance score using PRD-compliant algorithm
                let relevance_score = scorer.calculate_score(
                    &title, None, // No description available
                    &url,
                );

                // Try to extract publication date from title or surrounding text
                let full_text = element.text().collect::<String>();
                if let Some(published_date) = RelevanceScorer::extract_published_date(&full_text) {
                    res.published_time = Some(published_date);
                }

                // Calculate freshness score
                let freshness_score = if let Some(published_time) = res.published_time {
                    RelevanceScorer::calculate_freshness_score(published_time)
                } else {
                    0.5 // Default freshness score for unknown dates
                };

                // Combine relevance and freshness scores (70% relevance, 30% freshness)
                res.score = relevance_score * 0.7 + freshness_score * 0.3;

                results.push(res);
            }
        }

        Ok(results)
    }
}

#[async_trait]
    impl SearchEngine for SogouSearchEngine {
        async fn search(
            &self,
            query: &str,
            limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            let url = "https://www.sogou.com/web";
            let limit_str = limit.to_string();

            let query_params = vec![("query", query), ("num", limit_str.as_str())];

            let response = self
                .client
                .get(url)
                .query(&query_params)
                .send()
                .await
                .map_err(|e| SearchError::NetworkError(e.to_string()))?;

            if !response.status().is_success() {
                return Err(SearchError::EngineError(format!(
                    "Sogou Search error: {}",
                    response.status()
                )));
            }

            let html_content = response
                .text()
                .await
                .map_err(|e| SearchError::EngineError(e.to_string()))?;

            let mut results = self.parse_search_results(&html_content, query)?;
            
            // 限制结果数量
            if results.len() > limit as usize {
                results.truncate(limit as usize);
            }

            Ok(results)
        }

    fn name(&self) -> &'static str {
        "sogou"
    }
}
