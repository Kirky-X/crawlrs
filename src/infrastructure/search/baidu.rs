use async_trait::async_trait;
use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use scraper::{Html, Selector};

pub struct BaiduSearchEngine {
    client: reqwest::Client,
}

impl BaiduSearchEngine {
    pub fn new() -> Self {
        // Use a browser-like user agent
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .build()
            .unwrap_or_default();
            
        Self { client }
    }
}

#[async_trait]
impl SearchEngine for BaiduSearchEngine {
    async fn search(&self, query: &str, limit: u32, _lang: Option<&str>, _country: Option<&str>) -> Result<Vec<SearchResult>, SearchError> {
        let url = "https://www.baidu.com/s";
        let rn = limit.to_string(); // Results per page
        
        let query_params = vec![
            ("wd", query),
            ("rn", &rn),
        ];

        let response = self.client
            .get(url)
            .query(&query_params)
            .send()
            .await
            .map_err(|e| SearchError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::EngineError(format!(
                "Baidu Search error: {}",
                response.status()
            )));
        }

        let html_content = response
            .text()
            .await
            .map_err(|e| SearchError::EngineError(e.to_string()))?;

        let document = Html::parse_document(&html_content);
        // Baidu result selector might change, this is a common one
        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse("h3").unwrap();
        let link_selector = Selector::parse("h3 > a").unwrap();
        
        let mut results = Vec::new();
        
        // Create relevance scorer for this query
        let scorer = RelevanceScorer::new(query);
        
        for element in document.select(&result_selector) {
            let title = element.select(&title_selector).next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
                
            let url = element.select(&link_selector).next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();
                
            if !title.is_empty() && !url.is_empty() {
                let mut res = SearchResult::new(
                    title.clone(),
                    url.clone(),
                    None, // Description extraction is complex
                    "baidu".to_string(),
                );
                
                // Calculate relevance score using PRD-compliant algorithm
                let relevance_score = scorer.calculate_score(
                    &title,
                    None, // No description available
                    &url
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
            
            if results.len() >= limit as usize {
                break;
            }
        }

        Ok(results)
    }

    fn name(&self) -> &'static str {
        "baidu"
    }
}