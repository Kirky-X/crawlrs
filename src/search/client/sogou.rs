// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{
    EngineClient, ScrapeOptions, ScrapeRequest as EngineScrapeRequest,
};
use crate::search::{
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::sync::Arc;

/// 安全解析CSS选择器，如果解析失败则返回None
fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// Sogou Search Engine implementation with EngineClient support
pub struct SogouSearchEngine {
    engine_client: Arc<EngineClient>,
}

impl SogouSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self { engine_client }
    }

    /// 解析并补全搜狗搜索结果中的URL
    /// 处理相对路径和中转URL格式
    pub fn resolve_url(&self, url: &str) -> String {
        if url.is_empty() {
            return String::new();
        }

        // 处理直接完整URL (仅允许 http/https)
        if url.starts_with("http://") || url.starts_with("https://") {
            // 验证 URL 格式并检查协议
            if let Ok(parsed) = url::Url::parse(url) {
                // 只允许 http/https 协议
                if parsed.scheme() == "http" || parsed.scheme() == "https" {
                    return url.to_string();
                }
            }
            return String::new();
        }

        // 处理搜狗中转链接: /link?url=...
        if url.starts_with("/link?url=") {
            // 提取参数中的URL并解码
            let encoded_url = url.trim_start_matches("/link?url=");
            // URL解码
            match urlencoding::decode(encoded_url) {
                Ok(decoded) => {
                    // 递归验证解码后的 URL
                    return self.resolve_url(&decoded);
                }
                Err(_) => return String::new(),
            };
        }

        // 处理其他相对路径 (仅搜狗域名)
        if url.starts_with("/") {
            return format!("https://www.sogou.com{}", url);
        }

        // 其他情况拒绝
        String::new()
    }

    pub fn parse_search_results(
        &self,
        html_content: &str,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html_content);
        let result_selector =
            safe_parse_selector(".vrwrap, .rb").expect("Failed to parse Sogou result selector");
        let title_selector =
            safe_parse_selector("h3").expect("Failed to parse Sogou title selector");
        let link_selector =
            safe_parse_selector("h3 a").expect("Failed to parse Sogou link selector");

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            // 提取标题 - 获取纯文本并清理空白
            let title_node = element.select(&title_selector).next();
            let raw_title = match title_node {
                Some(node) => node.text().collect::<String>(),
                None => continue,
            };
            let title = html_escape::encode_text(raw_title.trim()).to_string();

            if title.is_empty() {
                continue;
            }

            // 提取链接
            let url_node = element.select(&link_selector).next();
            let raw_url = match url_node {
                Some(node) => node.value().attr("href").unwrap_or("").to_string(),
                None => continue,
            };

            // 解析并补全URL
            let resolved_url = self.resolve_url(&raw_url);

            if !resolved_url.is_empty() {
                results.push(ResponseItem {
                    title,
                    url: resolved_url,
                    description: String::new(),
                    engine: SearchEngineType::Sogou,
                });
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for SogouSearchEngine {
    fn name(&self) -> &'static str {
        "Sogou"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Sogou
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        if std::env::var("SOGOU_TEST_RESULTS").unwrap_or_default() == "true" {
            let escaped_query = html_escape::encode_text(&request.query);
            return Ok(Response {
                items: vec![
                    ResponseItem {
                        title: format!("Test Result 1 for {}", escaped_query),
                        url: "https://sogou.com/1".to_string(),
                        description: "Test description 1".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                    ResponseItem {
                        title: format!("Test Result 2 for {}", escaped_query),
                        url: "https://sogou.com/2".to_string(),
                        description: "Test description 2".to_string(),
                        engine: SearchEngineType::Sogou,
                    },
                ],
                total_results: Some(2),
                engine: SearchEngineType::Sogou,
            });
        }

        let base_url = "https://www.sogou.com/web";

        // 构建带查询参数的完整 URL
        let full_url = format!(
            "{}?query={}&num={}",
            base_url,
            urlencoding::encode(&request.query),
            request.limit
        );

        // 构建请求头
        let mut headers = HashMap::new();
        headers.insert(
            "Accept".to_string(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"
                .to_string(),
        );
        headers.insert(
            "Accept-Language".to_string(),
            "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
        );
        headers.insert("DNT".to_string(), "1".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());

        // 使用 EngineClient 进行请求
        let options = ScrapeOptions {
            headers,
            timeout: std::time::Duration::from_secs(30),
            ..Default::default()
        };

        let engine_request = EngineScrapeRequest {
            url: full_url,
            options,
        };

        let scrape_response = self
            .engine_client
            .scrape(&engine_request)
            .await
            .map_err(|e| SearchError::Engine(format!("EngineClient error: {}", e)))?;

        if scrape_response.status_code < 200 || scrape_response.status_code >= 300 {
            return Err(SearchError::Engine(format!(
                "Sogou search error: {}",
                scrape_response.status_code
            )));
        }

        let html_content = scrape_response.content;
        let items = self.parse_search_results(&html_content)?;
        let total_count = items.len() as u64;

        Ok(Response {
            items,
            total_results: Some(total_count),
            engine: SearchEngineType::Sogou,
        })
    }
}
