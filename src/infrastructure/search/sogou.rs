// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::info;

pub struct SogouSearchEngine {
    client: reqwest::Client,
}

/// 测试搜索结果条目结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSearchResultEntry {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
}

/// 搜狗测试配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SogouTestConfig {
    pub results: Vec<TestSearchResultEntry>,
}

/// 加载搜狗测试配置
fn load_test_config() -> Option<SogouTestConfig> {
    // 首先检查 USE_TEST_DATA 环境变量
    if std::env::var("USE_TEST_DATA").is_err() {
        return None;
    }

    let config_path = Path::new("test-data/search-engines/test-results.yaml");
    if !config_path.exists() {
        tracing::warn!("测试配置文件不存在: {}", config_path.display());
        return None;
    }

    let content = fs::read_to_string(config_path)
        .map_err(|e| {
            tracing::error!("读取测试配置文件失败: {}", e);
            e
        })
        .ok()?;

    // 解析YAML文件
    let config: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;

    // 提取搜狗配置
    let sogou_config = config.get("sogou")?;
    let sogou_test_config: SogouTestConfig = serde_yaml::from_value(sogou_config.clone()).ok()?;

    Some(sogou_test_config)
}

/// 从配置创建搜索结果
fn create_search_results_from_config(config: &SogouTestConfig, _query: &str) -> Vec<SearchResult> {
    config
        .results
        .iter()
        .map(|entry| {
            let mut result = SearchResult::new(
                entry.title.clone(),
                entry.url.clone(),
                entry.description.clone(),
                "sogou".to_string(),
            );
            // 默认分数
            result.score = 0.8;
            result
        })
        .collect()
}

impl Default for SogouSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SogouSearchEngine {
    pub fn new() -> Self {
        // 使用浏览器风格的用户代理
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

        let mut results = Vec::new();

        // Create relevance scorer for this query
        let scorer = RelevanceScorer::new(query);

        for element in document.select(&result_selector) {
            // 提取标题 - 多种策略
            let title = {
                // 策略 1: 直接查找 h3 > a
                if let Some(a) = element.select(&Selector::parse("h3 > a").unwrap()).next() {
                    let text = a.text().collect::<String>();
                    if !text.trim().is_empty() {
                        text.trim().to_string()
                    } else {
                        String::new()
                    }
                } else {
                    // 策略 2: 查找任意 a 标签中的文本
                    let mut title_text = String::new();
                    for a in element.select(&Selector::parse("a").unwrap()) {
                        let text = a.text().collect::<String>();
                        let text = text.trim().to_string();
                        if text.len() >= 5 && text.len() <= 200 {
                            title_text = text;
                            break;
                        }
                    }
                    title_text
                }
            };

            // 提取链接 - 多种策略
            let url = {
                // 策略 1: h3 > a
                if let Some(a) = element.select(&Selector::parse("h3 > a").unwrap()).next() {
                    if let Some(href) = a.value().attr("href") {
                        if !href.is_empty() {
                            href.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    // 策略 2: 任意 a 标签
                    let mut url_text = String::new();
                    for a in element.select(&Selector::parse("a").unwrap()) {
                        if let Some(href) = a.value().attr("href") {
                            if href.starts_with("http") {
                                url_text = href.to_string();
                                break;
                            }
                        }
                    }
                    url_text
                }
            };

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
        // 首先检查 USE_TEST_DATA 环境变量，优先使用配置文件
        if std::env::var("USE_TEST_DATA").is_ok() {
            if let Some(config) = load_test_config() {
                info!("使用配置文件中的测试结果进行 Sogou 搜索");
                let mut results = create_search_results_from_config(&config, query);
                // 限制结果数量
                if results.len() > limit as usize {
                    results.truncate(limit as usize);
                }
                return Ok(results);
            }
        }

        // 回退到旧的环境变量方式以保持向后兼容
        if std::env::var("SOGOU_TEST_RESULTS").is_ok() {
            info!("使用旧版环境变量 SOGOU_TEST_RESULTS 进行 Sogou 搜索（已废弃）");
            return Ok(vec![SearchResult::new(
                "搜狗AI - 微盛搜索".to_string(),
                "https://zhuanlan.zhihu.com/p/123456".to_string(),
                Some("搜狗AI搜索是腾讯搜狗的智能搜索服务。".to_string()),
                "sogou".to_string(),
            )]);
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_test_config() {
        std::env::set_var("USE_TEST_DATA", "1");

        let config = load_test_config();
        assert!(config.is_some(), "应该能够加载测试配置");

        let config = config.unwrap();
        assert!(!config.results.is_empty(), "配置中应该有测试结果");

        if let Some(first_result) = config.results.first() {
            assert!(!first_result.title.is_empty(), "标题不应为空");
            assert!(!first_result.url.is_empty(), "URL不应为空");
        }

        std::env::remove_var("USE_TEST_DATA");
    }

    #[test]
    fn test_create_search_results_from_config() {
        std::env::set_var("USE_TEST_DATA", "1");

        let config = load_test_config().expect("应该能够加载配置");
        let results = create_search_results_from_config(&config, "测试查询");

        assert!(!results.is_empty(), "应该创建搜索结果");
        assert_eq!(
            results.first().unwrap().engine,
            "sogou",
            "搜索引擎应该是sogou"
        );

        std::env::remove_var("USE_TEST_DATA");
    }
}
