use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;
use scraper::{Html, Selector};
use tracing::info;
use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use rand::Rng;

struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
    client: reqwest::Client,
}

impl GoogleSearchEngine {
    pub fn new() -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }
    
    /// 生成 23 位随机 ARC_ID
    fn generate_random_id() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
        
        let mut rng = rand::rng();
        (0..23)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }
    
    /// 获取 ARC_ID（每小时自动刷新）
    async fn get_arc_id(&self, start_offset: usize) -> String {
        let mut cache = self.arc_id_cache.write().await;
        let now = Utc::now().timestamp();
        
        // 超过 1 小时重新生成
        if now - cache.generated_at > 3600 {
            cache.arc_id = Self::generate_random_id();
            cache.generated_at = now;
            info!("Google ARC_ID refreshed: {}", cache.arc_id);
        }
        
        format!(
            "arc_id:srp_{}_1{:02},use_ac:true,_fmt:prog",
            cache.arc_id,
            start_offset
        )
    }
    
    /// 解析 Google HTML 结果
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        let document = Html::parse_document(html);
        
        // Google 结果容器 selector（可能变化，需监控）
        let result_selector = Selector::parse("div[jscontroller='SC7lYd']")
            .map_err(|_| SearchError::EngineError("Failed to parse result selector".to_string()))?;
        let title_selector = Selector::parse("h3").unwrap();
        let url_selector = Selector::parse("a[href]").unwrap();
        let content_selector = Selector::parse("div[data-sncf='1']").unwrap();
        
        let mut results = Vec::new();
        
        for element in document.select(&result_selector) {
            // 提取标题
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
            
            if title.is_empty() {
                continue;
            }
            
            // 提取 URL
            let url = element
                .select(&url_selector)
                .find_map(|e| e.value().attr("href"))
                .unwrap_or("")
                .to_string();
            
            // 提取摘要
            let content = element
                .select(&content_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
            
            results.push(SearchResult {
                title,
                url,
                description: Some(content),
                engine: "google".to_string(),
                score: 1.0,  // 后续可优化
                published_time: None,
            });
        }
        
        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    async fn search(&self, query: &str, limit: u32, lang: Option<&str>, country: Option<&str>) -> Result<Vec<SearchResult>, SearchError> {
        let page = 1; // Default to first page for simplicity
        let start = (page - 1) * limit;
        
        let lang_code = lang.unwrap_or("en");
        let country_code = country.unwrap_or("US");
        let start_str = start.to_string();
        let limit_str = limit.to_string();
        let hl_str = format!("{}-{}", lang_code, country_code);
        let arc_id_str = self.get_arc_id(start as usize).await;
        
        // Build query parameters
        let mut query_params: Vec<(&str, &str)> = vec![
            ("q", query),
            ("hl", &hl_str),
            ("start", &start_str),
            ("num", &limit_str),
            ("asearch", "arc"),
            ("async", &arc_id_str),
            ("filter", "0"),
            ("safe", "medium"),
        ];
        
        // Add language parameter if provided
        let lang_param;
        if let Some(l) = lang {
            lang_param = format!("lang_{}", l);
            query_params.push(("lr", &lang_param));
        }
        
        // Add country parameter if provided
        let country_param;
        if let Some(c) = country {
            country_param = format!("country{}", c.to_uppercase());
            query_params.push(("cr", &country_param));
        }
        
        let response = self.client
            .get("https://www.google.com/search")
            .query(&query_params)
            .send()
            .await
            .map_err(|e| SearchError::NetworkError(e.to_string()))?;
        
        let html = response.text().await
            .map_err(|e| SearchError::NetworkError(e.to_string()))?;
        
        // Parse HTML results
        let mut results = self.parse_results(&html)?;
        
        // Apply relevance scoring and freshness calculation
        let scorer = RelevanceScorer::new(query);
        
        for result in &mut results {
            // Calculate relevance score
            let relevance_score = scorer.calculate_score(
                &result.title,
                result.description.as_deref(),
                &result.url
            );
            
            // Extract publication date from description if available
            if let Some(description) = &result.description {
                if let Some(published_date) = RelevanceScorer::extract_published_date(description) {
                    result.published_time = Some(published_date);
                }
            }
            
            // Calculate freshness score
            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5 // Default freshness score for unknown dates
            };
            
            // Combine relevance and freshness scores (70% relevance, 30% freshness)
            result.score = relevance_score * 0.7 + freshness_score * 0.3;
        }
        
        // Sort by score (highest first)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        Ok(results)
    }

    fn name(&self) -> &'static str {
        "google"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_id() {
        let id1 = GoogleSearchEngine::generate_random_id();
        let id2 = GoogleSearchEngine::generate_random_id();
        
        assert_eq!(id1.len(), 23);
        assert_eq!(id2.len(), 23);
        assert_ne!(id1, id2); // Should generate different IDs
    }

    #[test]
    fn test_arc_id_cache_generation() {
        let cache = ArcIdCache {
            arc_id: "test123".to_string(),
            generated_at: Utc::now().timestamp() - 3700, // 1+ hour ago
        };
        
        assert_eq!(cache.arc_id, "test123");
        assert!(Utc::now().timestamp() - cache.generated_at > 3600);
    }

    #[tokio::test]
    async fn test_google_search_engine_creation() {
        let engine = GoogleSearchEngine::new();
        assert_eq!(engine.name(), "google");
        
        // Test that arc_id_cache is properly initialized
        let cache = engine.arc_id_cache.read().await;
        assert_eq!(cache.arc_id.len(), 23);
        assert!(cache.generated_at > 0);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        let engine = GoogleSearchEngine::new();
        
        // First call should generate initial ID
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));
        
        // Wait a bit and call again - should use same ID
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;
        
        // Should contain different offset but same base ID
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2); // Different due to offset
    }

    #[test]
    fn test_parse_results_empty_html() {
        let engine = GoogleSearchEngine::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_parse_results_with_sample_data() {
        let engine = GoogleSearchEngine::new();
        let html = r#"
        <html>
        <body>
            <div jscontroller="SC7lYd">
                <div>
                    <h3>Test Title</h3>
                    <a href="https://example.com">Link</a>
                </div>
                <div data-sncf="1">Test content here</div>
            </div>
        </body>
        </html>
        "#;
        
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        
        let result = &results[0];
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.url, "https://example.com");
        assert_eq!(result.description, Some("Test content here".to_string()));
        assert_eq!(result.engine, "google");
    }

    #[tokio::test]
    async fn test_google_search_harmonyos_stars() {
        // 测试Google搜索：鸿蒙星光大赏
        let google_engine = GoogleSearchEngine::new();
        
        let query = "鸿蒙星光大赏";
        let results = google_engine.search(query, 5, Some("zh-CN"), Some("CN")).await;
        
        match results {
            Ok(search_results) => {
                println!("Google搜索 '{}' 找到 {} 个结果:", query, search_results.len());
                
                for (i, result) in search_results.iter().enumerate() {
                    println!("\n结果 {}:", i + 1);
                    println!("标题: {}", result.title);
                    println!("URL: {}", result.url);
                    if let Some(desc) = &result.description {
                        println!("描述: {}", desc.chars().take(100).collect::<String>());
                    }
                    println!("评分: {:.2}", result.score);
                }
                
                // 验证至少找到一个结果
                assert!(!search_results.is_empty(), "应该至少找到一个搜索结果");
                
                // 验证结果包含预期的中文关键词
                let has_relevant_result = search_results.iter().any(|r| {
                    r.title.contains("鸿蒙") || 
                    r.title.contains("星光") || 
                    r.title.contains("大赏") ||
                    r.description.as_ref().map_or(false, |d| 
                        d.contains("鸿蒙") || d.contains("星光") || d.contains("大赏")
                    )
                });
                
                assert!(has_relevant_result, "搜索结果应该包含相关的中文关键词");
            }
            Err(e) => {
                println!("搜索失败: {}", e);
                panic!("Google搜索测试失败: {}", e);
            }
        }
    }
}