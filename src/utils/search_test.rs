// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

//! 搜索引擎测试工具模块
//!
//! 提供统一的搜索引擎测试框架，包含：
//! - 公共常量和配置
//! - 测试结果结构
//! - 搜索引擎测试辅助函数
//! - URL 可访问性检查

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::SearchEngine;
use crate::domain::search::engine::SearchError;
use html_escape;
use tokio::time::Duration;
use tracing::{error, info};

pub const DEFAULT_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_KEYWORD: &str = "gemini-3-pro";
pub const DEFAULT_RESULT_LIMIT: u32 = 10;

pub struct TestResult {
    pub accessible: usize,
    pub inaccessible: usize,
    pub total: usize,
}

impl Default for TestResult {
    fn default() -> Self {
        Self {
            accessible: 0,
            inaccessible: 0,
            total: 0,
        }
    }
}

pub async fn run_engine_test_with_output<E: SearchEngine + Send + 'static>(
    name: &'static str,
    engine: E,
    keyword: Option<&str>,
    timeout_secs: u64,
    result_limit: Option<u32>,
) -> Result<TestResult, SearchError> {
    let keyword = keyword.unwrap_or(DEFAULT_KEYWORD);
    let result_limit = result_limit.unwrap_or(DEFAULT_RESULT_LIMIT);

    info!("==========================================");
    info!("测试 {} 搜索引擎", name);
    info!("关键词: {}", keyword);
    info!("超时: {} 秒", timeout_secs);
    info!("==========================================");

    let start_time = std::time::Instant::now();

    match engine.search(keyword, result_limit, None, None).await {
        Ok(results) => {
            let elapsed = start_time.elapsed();
            print_search_results(name, &results).await;

            let test_result = check_urls_accessibility(&results).await;

            info!("");
            info!("{} 搜索完成, 耗时: {:?}", name, elapsed);
            info!("共返回 {} 个结果", results.len());
            info!("✅ 可访问: {} 个", test_result.accessible);
            info!("❌ 不可访问: {} 个", test_result.inaccessible);

            Ok(test_result)
        }
        Err(e) => {
            error!("{} 搜索出错: {:?}", name, e);
            Err(e)
        }
    }
}

pub async fn print_search_results(name: &str, results: &[SearchResult]) {
    if results.is_empty() {
        info!("{} 返回空结果", name);
        return;
    }

    info!("");
    info!("搜索结果详情:");
    info!("----------------------------------------");

    for (i, result) in results.iter().enumerate() {
        let cleaned_url = html_escape::decode_html_entities(&result.url);

        info!("  [{}] {}", i + 1, result.title);
        info!("      URL: {}", cleaned_url);

        if let Some(ref desc) = result.description {
            let desc_len = desc.len();
            let desc_short: String = if desc_len > 150 {
                desc.chars().take(150).collect::<String>() + "..."
            } else {
                desc.clone()
            };
            info!("      描述: {}", desc_short);
        }

        info!("      来源: {}", result.engine);
        info!("");
    }

    info!("----------------------------------------");
}

pub async fn check_urls_accessibility(results: &[SearchResult]) -> TestResult {
    let mut test_result = TestResult::default();

    for result in results {
        let cleaned_url = html_escape::decode_html_entities(&result.url);

        match check_url_accessibility(&cleaned_url).await {
            Ok(true) => {
                test_result.accessible += 1;
            }
            Ok(false) => {
                test_result.inaccessible += 1;
            }
            Err(_) => {
                test_result.inaccessible += 1;
            }
        }

        test_result.total += 1;
    }

    test_result
}

pub async fn check_url_accessibility(url: &str) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("客户端创建失败: {}", e))?;

    let response = client
        .head(url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    Ok(response.status().is_success() || response.status().is_redirection())
}

pub async fn print_engine_summary(results: &[(&str, &Result<TestResult, impl std::fmt::Debug>)]) {
    info!("");
    info!("========================================");
    info!("搜索引擎测试汇总");
    info!("========================================");

    for (name, result) in results {
        match result {
            Ok(r) => {
                let status = if r.inaccessible == 0 && r.total > 0 {
                    "✅ 完全正常"
                } else if r.accessible > 0 {
                    "⚠️ 部分异常"
                } else {
                    "❌ 完全异常"
                };
                info!(
                    "  {}: {} (可访问: {}, 不可访问: {})",
                    name, status, r.accessible, r.inaccessible
                );
            }
            Err(e) => {
                info!("  {}: ❌ 搜索失败 ({:?})", name, e);
            }
        }
    }

    info!("========================================");
}
