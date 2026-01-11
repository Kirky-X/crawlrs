// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::{
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Google ARC_ID 缓存结构
struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

/// Google 搜索引擎实现
pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
}

impl Default for GoogleSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleSearchEngine {
    pub fn new() -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
        }
    }

    /// 生成 23 位随机 ARC_ID
    fn generate_random_id() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

        let mut rng = rand::rng();
        (0..23)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// 获取 ARC_ID（每小时自动刷新）
    pub async fn get_arc_id(&self, start_offset: usize) -> String {
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
            cache.arc_id, start_offset
        )
    }

    /// 解析 Google HTML 结果
    fn parse_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html);

        let mut results = Vec::new();

        info!("开始解析 Google 搜索结果...");

        // 策略 1: 使用 jscontroller 选择器
        let selector_v1 = Selector::parse("div[jscontroller*='SC7lYd']").unwrap();
        let selector_v2 = Selector::parse("div.g").unwrap();
        let selector_v3 = Selector::parse("div[data-hveid]").unwrap();
        let selector_v4 = Selector::parse("div:has(> a > h3)").unwrap();

        // 尝试不同的选择器策略
        let mut result_elements: Vec<_> = document.select(&selector_v1).collect();
        if result_elements.is_empty() {
            result_elements = document.select(&selector_v2).collect();
        }

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v3).collect();
        }

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v4).collect();
        }

        // 标题选择器
        let title_selector = Selector::parse("h3").unwrap();

        // 链接选择器
        let link_selector = Selector::parse("a[href]").unwrap();

        // 摘要选择器
        let snippet_selector_1 = Selector::parse("[data-sncf], div[data-snc]").unwrap();
        let snippet_selector_2 = Selector::parse("span.st, div.st, p.st").unwrap();
        let snippet_selector_3 =
            Selector::parse("div[class*='snippet'], div[class*='desc']").unwrap();

        for element in result_elements {
            // 提取标题
            let title = {
                let mut title_text = String::new();

                if let Some(a) = element.select(&link_selector).next() {
                    if let Some(h3) = a.select(&title_selector).next() {
                        let text = h3.text().collect::<String>();
                        if !text.is_empty() {
                            title_text = text;
                        }
                    }
                }

                if title_text.is_empty() {
                    if let Some(h3) = element.select(&title_selector).next() {
                        let text = h3.text().collect::<String>();
                        if !text.is_empty() {
                            title_text = text;
                        }
                    }
                }

                title_text
            };

            if title.is_empty() {
                continue;
            }

            // 提取 URL
            let url = {
                let mut found_url = String::new();

                if let Some(a) = element.select(&link_selector).next() {
                    if let Some(_h3) = a.select(&title_selector).next() {
                        if let Some(href) = a.value().attr("href") {
                            if !href.is_empty() {
                                found_url = href.to_string();
                            }
                        }
                    }
                }

                if found_url.is_empty() {
                    for a in element.select(&link_selector) {
                        if let Some(href) = a.value().attr("href") {
                            if !href.is_empty() && href.starts_with("http") {
                                found_url = href.to_string();
                                break;
                            }
                        }
                    }
                }

                found_url
            };

            // 清理 URL
            let clean_url = if url.starts_with("/url?q=") {
                url.replace("/url?q=", "")
                    .split('&')
                    .next()
                    .unwrap_or(&url)
                    .to_string()
            } else if url.starts_with("/") && !url.starts_with("//") {
                format!("https://www.google.com{}", url)
            } else {
                url
            };

            if clean_url.is_empty() || !clean_url.starts_with("http") {
                continue;
            }

            // 提取摘要
            let mut description = String::new();
            for selector in &[
                &snippet_selector_1,
                &snippet_selector_2,
                &snippet_selector_3,
            ] {
                if let Some(e) = element.select(selector).next() {
                    let text = e.text().collect::<String>();
                    if !text.is_empty() {
                        description = text;
                        break;
                    }
                }
            }

            if description.is_empty() {
                for a in element.select(&link_selector) {
                    if let Some(next_sibling) = a.next_sibling() {
                        if let Some(elem_ref) = ElementRef::wrap(next_sibling) {
                            let text = elem_ref.text().collect::<String>();
                            let text = text.trim().to_string();
                            if text.len() > 10 && text.len() < 500 {
                                description = text;
                                break;
                            }
                        }
                    }
                }
            }

            // 去重
            if results.iter().any(|r: &ResponseItem| r.url == clean_url) {
                continue;
            }

            results.push(ResponseItem {
                title,
                url: clean_url,
                description,
                engine: SearchEngineType::Google,
            });

            if results.len() >= 20 {
                break;
            }
        }

        info!("成功解析到 {} 个 Google 搜索结果", results.len());

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    fn get_name(&self) -> &'static str {
        "Google"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Google
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let page = 1;
        let start = (page - 1) * request.limit;
        let start_str = start.to_string();

        // 构建查询参数
        let mut query_params: Vec<(&str, String)> = vec![
            ("q", request.query.clone()),
            ("ie", "utf8".to_string()),
            ("oe", "utf8".to_string()),
            ("start", start_str),
            ("num", request.limit.to_string()),
        ];

        // 添加异步搜索参数和 ARC_ID
        query_params.push(("asearch", "arc".to_string()));
        query_params.push(("async", self.get_arc_id(start as usize).await));

        info!(
            "Google搜索请求: query={}, limit={}",
            request.query, request.limit
        );

        // 构建 Google 搜索 URL
        let mut google_url = "https://www.google.com/search?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        google_url.push_str(&query_string);
        info!("Constructed Google Search URL: {}", google_url);

        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SearchError::Network(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .get(&google_url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("DNT", "1")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await
            .map_err(|e| SearchError::Network(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(SearchError::Engine(format!(
                "Google search returned status: {}",
                response.status()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| SearchError::Network(format!("Failed to read response body: {}", e)))?;

        info!("Google search returned HTML length: {} bytes", html.len());

        if html.len() < 1000 {
            warn!("Google search returned insufficient content (likely blocked)");
            return Err(SearchError::Engine(
                "Google search returned insufficient content (likely blocked)".to_string(),
            ));
        }

        // 解析结果
        let items = self.parse_results(&html)?;

        Ok(Response {
            items,
            total_results: Some(items.len() as u64),
            engine: SearchEngineType::Google,
        })
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
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_google_search_engine_creation() {
        let engine = GoogleSearchEngine::new();
        assert_eq!(engine.get_name(), "Google");
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        let engine = GoogleSearchEngine::new();

        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;

        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2);
    }
}
