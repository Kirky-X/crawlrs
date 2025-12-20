use async_trait::async_trait;
use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum BaiduSearchCategory {
    General,
    Images,
}

impl BaiduSearchCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            BaiduSearchCategory::General => "general",
            BaiduSearchCategory::Images => "images",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduResponse {
    feed: Option<BaiduFeed>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduFeed {
    entry: Option<Vec<BaiduEntry>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduEntry {
    title: Option<String>,
    url: Option<String>,
    abs: Option<String>, // 摘要字段
}

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

    /// 构建百度搜索URL，支持多端点
    pub fn build_baidu_url(&self, query: &str, page: u32, category: BaiduSearchCategory) -> (String, HashMap<String, String>) {
        let page_size = 10;
        let offset = (page - 1) * page_size;
        
        let (url, params) = match category {
            BaiduSearchCategory::General => {
                // 通用搜索 API
                let url = "https://www.baidu.com/s".to_string();
                let mut params = HashMap::new();
                params.insert("wd".to_string(), query.to_string());
                params.insert("rn".to_string(), page_size.to_string());
                params.insert("pn".to_string(), offset.to_string());
                params.insert("tn".to_string(), "json".to_string()); // 关键参数：请求 JSON 响应
                (url, params)
            }
            BaiduSearchCategory::Images => {
                // 图片搜索 API
                let url = "https://image.baidu.com/search/acjson".to_string();
                let mut params = HashMap::new();
                params.insert("word".to_string(), query.to_string());
                params.insert("rn".to_string(), page_size.to_string());
                params.insert("pn".to_string(), offset.to_string());
                params.insert("tn".to_string(), "resultjson_com".to_string());
                (url, params)
            }
        };
        
        (url, params)
    }

    /// 解析百度JSON响应
    pub fn parse_baidu_response(&self, json_text: &str) -> Result<Vec<SearchResult>, SearchError> {
        let data: BaiduResponse = serde_json::from_str(json_text)
            .map_err(|e| SearchError::EngineError(format!("JSON parsing error: {}", e)))?;
        
        let mut results = Vec::new();
        
        // 检查是否有结果
        if let Some(feed) = data.feed {
            if let Some(entries) = feed.entry {
                for entry in entries {
                    if let (Some(title), Some(url)) = (entry.title, entry.url) {
                        // HTML转义字符解码
                        let decoded_title = html_escape::decode_html_entities(&title).to_string();
                        let decoded_content = entry.abs
                            .as_ref()
                            .map(|abs| html_escape::decode_html_entities(abs).to_string());
                        
                        // 计算相关性分数
                        let scorer = RelevanceScorer::new(""); // 将在search方法中设置正确的查询词
                        let relevance_score = scorer.calculate_score(
                            &decoded_title,
                            decoded_content.as_deref(),
                            &url
                        );
                        
                        let mut search_result = SearchResult::new(
                            decoded_title,
                            url.clone(),
                            decoded_content, // 使用原始值，因为calculate_score只需要引用
                            "baidu".to_string(),
                        );
                        
                        search_result.score = relevance_score;
                        results.push(search_result);
                    }
                }
            }
        }
        
        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for BaiduSearchEngine {
    async fn search(&self, query: &str, limit: u32, _lang: Option<&str>, _country: Option<&str>) -> Result<Vec<SearchResult>, SearchError> {
        // 默认使用通用搜索，可以通过参数或配置扩展到支持图片搜索
        let category = BaiduSearchCategory::General;
        let page = 1; // 可以根据limit计算页数
        
        let (url, params) = self.build_baidu_url(query, page, category);

        let response = self.client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| SearchError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::EngineError(format!(
                "Baidu Search error: {}",
                response.status()
            )));
        }

        let json_content = response
            .text()
            .await
            .map_err(|e| SearchError::EngineError(e.to_string()))?;

        // 解析JSON响应
        let mut results = self.parse_baidu_response(&json_content)?;
        
        // 限制结果数量
        results.truncate(limit as usize);
        
        // 为每个结果计算相关性分数和新鲜度分数
        let scorer = RelevanceScorer::new(query);
        for result in &mut results {
            // 重新计算相关性分数，使用正确的查询词
            let relevance_score = scorer.calculate_score(
                &result.title,
                result.description.as_deref(),
                &result.url
            );
            
            // 尝试从标题中提取发布日期
            if let Some(published_date) = RelevanceScorer::extract_published_date(&result.title) {
                result.published_time = Some(published_date);
            }
            
            // 计算新鲜度分数
            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5 // 默认新鲜度分数
            };
            
            // 结合相关性分数和新鲜度分数（70% 相关性，30% 新鲜度）
            result.score = relevance_score * 0.7 + freshness_score * 0.3;
        }
        
        // 按分数排序
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }

    fn name(&self) -> &'static str {
        "baidu"
    }
}