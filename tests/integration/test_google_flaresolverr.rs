use async_trait::async_trait;
use crawlrs::domain::models::search_result::SearchResult;
use crawlrs::domain::search::engine::{SearchEngine, SearchError};
use scraper::{Html, Selector};
use tracing::info;

/// 基于FlareSolverr的Google搜索引擎
/// 用于绕过反爬虫保护
pub struct FlareSolverrGoogleEngine {
    flaresolverr_url: String,
    client: reqwest::Client,
}

impl FlareSolverrGoogleEngine {
    pub fn new(flaresolverr_url: String) -> Self {
        Self {
            flaresolverr_url,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap(),
        }
    }

    /// 使用FlareSolverr发送请求
    async fn flaresolverr_request(&self, url: &str) -> Result<String, SearchError> {
        let request = serde_json::json!({
            "cmd": "request.get",
            "url": url,
            "maxTimeout": 60000,
            "headers": {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            }
        });

        let response = self.client
            .post(&format!("{}/v1", self.flaresolverr_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        let json_response: serde_json::Value = response.json().await
            .map_err(|e| SearchError::RequestFailed(e.to_string()))?;

        if json_response["status"] == "ok" {
            Ok(json_response["solution"]["response"].as_str()
                .ok_or_else(|| SearchError::RequestFailed("No response content".to_string()))?
                .to_string())
        } else {
            Err(SearchError::RequestFailed(
                json_response["message"].as_str().unwrap_or("Unknown error").to_string()
            ))
        }
    }
}

#[tokio::main]
async fn main() {
    println!("=== 测试FlareSolverr Google搜索 ===");
    
    let engine = FlareSolverrGoogleEngine::new("http://localhost:8191".to_string());
    
    match engine.search("rust programming", 5, Some("en"), Some("US")).await {
        Ok(results) => {
            println!("✅ 搜索成功！找到 {} 个结果", results.len());
            for (i, result) in results.iter().enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
                println!("     描述: {}", result.description);
            }
        }
        Err(e) => {
            println!("❌ 搜索失败: {:?}", e);
        }
    }
}