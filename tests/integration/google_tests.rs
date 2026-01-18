// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "engine-fire-cdp")]
use super::helpers::google_helpers::{create_google_engine, get_chrome_ws_url, set_chrome_ws_url};
use crawlrs::common::constants::testing::{CRAWL_TASK_TIMEOUT, QUICK_TEST_TIMEOUT};
use crawlrs::search::client::google::GoogleSearchEngine;
use crawlrs::search::response::{Response, ResponseItem};
use crawlrs::search::types::SearchEngineType;
use crawlrs::search::{SearchEngine, SearchError, SearchRequest};
use reqwest;
use serde_json;

async fn test_flaresolverr_status(client: &reqwest::Client) -> bool {
    println!("\n1. 检查FlareSolverr状态...");
    match client.get("http://localhost:8191").send().await {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                println!(
                    "✓ FlareSolverr状态: {}",
                    json["msg"].as_str().unwrap_or("未知")
                );
                println!("  版本: {}", json["version"].as_str().unwrap_or("未知"));
                true
            }
            Err(_) => {
                println!("✗ 解析FlareSolverr响应失败");
                false
            }
        },
        Err(_) => {
            println!("✗ 连接FlareSolverr失败");
            false
        }
    }
}

async fn test_flaresolverr_google_request(client: &reqwest::Client, query: &str) -> bool {
    println!("\n2. 使用FlareSolverr访问Google搜索: {}...", query);
    let flaresolverr_request = serde_json::json!({
        "cmd": "request.get",
        "url": format!("https://www.google.com/search?q={}", query.replace(' ', "+")),
        "maxTimeout": 60000,
        "headers": {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        }
    });

    match client
        .post("http://localhost:8191/v1")
        .json(&flaresolverr_request)
        .send()
        .await
    {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if json["status"].as_str() == Some("ok") {
                    println!("✓ FlareSolverr成功绕过CF验证");
                    if let Some(solution) = json["solution"].as_object() {
                        println!("  响应状态: {:?}", solution["status"]);
                        println!(
                            "  内容长度: {} 字符",
                            solution["response"].as_str().map(|s| s.len()).unwrap_or(0)
                        );
                    }
                    true
                } else {
                    println!("✗ FlareSolverr处理失败: {:?}", json["message"]);
                    false
                }
            }
            Err(_) => {
                println!("✗ 解析FlareSolverr响应失败");
                false
            }
        },
        Err(_) => {
            println!("✗ FlareSolverr请求失败");
            false
        }
    }
}

async fn test_google_with_google_engine(google_engine: &GoogleSearchEngine, query: &str) -> bool {
    println!("\n🔍 搜索关键词: {}", query);

    let request = SearchRequest::new(query)
        .with_limit(3)
        .with_lang("zh-CN")
        .with_country("CN");

    match google_engine.search(&request).await {
        Ok(results) => {
            println!("✓ 搜索成功！找到 {} 个结果", results.items.len());

            for (i, result) in results.items.iter().enumerate().take(2) {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
                if !result.description.is_empty() {
                    println!(
                        "     描述: {}",
                        &result.description[..100.min(result.description.len())]
                    );
                }
            }
            true
        }
        Err(e) => {
            println!("✗ 搜索失败: {:?}", e);
            false
        }
    }
}

async fn test_flaresolverr_google_engine(
    flaresolverr_engine: &FlareSolverrGoogleEngine,
    query: &str,
) -> bool {
    println!("\n🔍 使用FlareSolverr搜索关键词: {}", query);

    let request = SearchRequest::new(query)
        .with_limit(5)
        .with_lang("en")
        .with_country("US");

    match flaresolverr_engine.search(&request).await {
        Ok(results) => {
            println!("✓ 搜索成功！找到 {} 个结果", results.items.len());

            for (i, result) in results.items.iter().enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
                if !result.description.is_empty() {
                    println!(
                        "     描述: {}",
                        &result.description[..100.min(result.description.len())]
                    );
                }
            }
            true
        }
        Err(e) => {
            println!("✗ 搜索失败: {:?}", e);
            false
        }
    }
}

struct FlareSolverrGoogleEngine {
    flaresolverr_url: String,
    client: reqwest::Client,
}

impl FlareSolverrGoogleEngine {
    pub fn new(flaresolverr_url: String) -> Self {
        Self {
            flaresolverr_url,
            client: reqwest::Client::builder()
                .timeout(CRAWL_TASK_TIMEOUT)
                .build()
                .expect("Failed to build reqwest client"),
        }
    }

    async fn flaresolverr_request(&self, url: &str) -> Result<String, String> {
        let request = serde_json::json!({
            "cmd": "request.get",
            "url": url,
            "maxTimeout": 60000,
            "headers": {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            }
        });

        let response = self
            .client
            .post(format!("{}/v1", self.flaresolverr_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json_response: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        if json_response["status"] == "ok" {
            Ok(json_response["solution"]["response"]
                .as_str()
                .ok_or_else(|| "No response content".to_string())?
                .to_string())
        } else {
            Err(json_response["message"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string())
        }
    }
}

#[async_trait::async_trait]
impl SearchEngine for FlareSolverrGoogleEngine {
    fn name(&self) -> &'static str {
        "FlareSolverrGoogle"
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let url = format!(
            "https://www.google.com/search?q={}&num={}",
            request.query.replace(' ', "+"),
            request.limit
        );

        let html = self
            .flaresolverr_request(&url)
            .await
            .map_err(SearchError::Engine)?;

        let items = self.parse_results(&html);
        Ok(Response {
            items,
            total_results: None,
            engine: SearchEngineType::Google,
        })
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Google
    }

    fn health(&self) -> crawlrs::search::types::EngineHealth {
        crawlrs::search::types::EngineHealth::Unknown
    }
}

impl FlareSolverrGoogleEngine {
    fn parse_results(&self, html: &str) -> Vec<ResponseItem> {
        use scraper::{Html, Selector};
        let document = Html::parse_document(html);
        let title_selector = Selector::parse("h3").expect("Failed to parse title selector");
        let link_selector = Selector::parse("a").expect("Failed to parse link selector");
        let snippet_selector =
            Selector::parse(".VwiC3b").expect("Failed to parse snippet selector");

        let mut results = Vec::new();
        let titles: Vec<_> = document.select(&title_selector).collect();

        for title in titles {
            if let Some(link) = title.select(&link_selector).next() {
                if let Some(href) = link.value().attr("href") {
                    if href.starts_with("/url?q=") {
                        let url = href.trim_start_matches("/url?q=").to_string();
                        let url = url.split('&').next().unwrap_or(href).to_string();

                        let description = title
                            .select(&snippet_selector)
                            .next()
                            .map(|el| el.text().collect::<String>())
                            .unwrap_or_default();

                        results.push(ResponseItem {
                            title: title.text().collect::<String>(),
                            url,
                            description,
                            engine: SearchEngineType::Google,
                        });
                    }
                }
            }
        }

        results
    }
}

#[tokio::test]
#[ignore]
#[ignore] // Ignoring this test because it requires FlareSolverr service at localhost:8191
async fn test_flaresolverr_connection() {
    println!("=== 测试FlareSolverr服务 ===");

    let client = reqwest::Client::new();
    let result1 = test_flaresolverr_status(&client).await;
    let result2 = test_flaresolverr_google_request(&client, "鸿蒙星光大赏").await;

    assert!(result1, "FlareSolverr状态检查失败");
    assert!(result2, "FlareSolverr Google请求失败");

    println!("🎉 FlareSolverr连接测试通过！");
}

#[tokio::test]
#[ignore]
#[ignore]
async fn test_flaresolverr_google_search() {
    println!("=== 测试FlareSolverr Google搜索 ===");

    let engine = FlareSolverrGoogleEngine::new("http://localhost:8191".to_string());

    let result = test_flaresolverr_google_engine(&engine, "rust programming").await;
    assert!(result, "FlareSolverr Google搜索测试失败");

    println!("🎉 FlareSolverr Google搜索测试通过！");
}

#[tokio::test]
#[ignore]
#[ignore]
async fn test_google_with_remote_chrome() {
    println!("=== 使用远程Chrome测试Google搜索 ===");

    let ws_url = get_chrome_ws_url();
    println!("使用远程Chrome: {}", ws_url);
    set_chrome_ws_url(&ws_url);

    let google_engine = create_google_engine();

    let result = test_google_with_google_engine(&google_engine, "鸿蒙星光大赏").await;
    assert!(result, "使用远程Chrome的Google搜索测试失败");

    println!("🎉 使用远程Chrome的Google搜索测试通过！");
}

#[tokio::test]
#[ignore]
#[ignore] // Ignoring this test because it requires remote Chrome
async fn test_google_with_timeout() {
    println!("=== 使用远程Chrome测试Google搜索（增加超时时间） ===");

    let ws_url = get_chrome_ws_url();
    println!("使用远程Chrome: {}", ws_url);
    set_chrome_ws_url(&ws_url);

    let google_engine = create_google_engine();

    let test_queries = vec!["鸿蒙星光大赏", "HarmonyOS", "华为"];

    for query in test_queries {
        let result = test_google_with_google_engine(&google_engine, query).await;
        assert!(result, "搜索关键词 '{}' 测试失败", query);

        tokio::time::sleep(QUICK_TEST_TIMEOUT).await;
    }

    println!("🎉 使用远程Chrome的Google搜索（超时测试）通过！");
}

/// 测试Google搜索引擎多种查询
///
/// 注意：此测试需要Chrome DevTools Protocol可用（ws://localhost:9222/devtools/browser/default）。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_google_multiple_queries -- --include-ignored
#[ignore]
#[tokio::test]
#[ignore]
async fn test_google_multiple_queries() {
    println!("=== 测试Google搜索引擎多种查询 ===");

    set_chrome_ws_url("ws://localhost:9222/devtools/browser/default");

    let google_engine = create_google_engine();

    let test_cases = vec![
        ("rust programming", Some("en"), Some("US")),
        ("鸿蒙操作系统", Some("zh-CN"), Some("CN")),
        ("华为手机", Some("zh-CN"), Some("CN")),
    ];

    for (query, lang, country) in test_cases {
        println!(
            "\n🔍 搜索关键词: {} (lang={:?}, country={:?})",
            query, lang, country
        );

        let mut request = SearchRequest::new(query).with_limit(5);
        if let Some(l) = lang {
            request = request.with_lang(l);
        }
        if let Some(c) = country {
            request = request.with_country(c);
        }

        match google_engine.search(&request).await {
            Ok(results) => {
                println!("✓ 搜索成功！找到 {} 个结果", results.items.len());
                for (i, result) in results.items.iter().enumerate().take(3) {
                    println!("  {}. {}", i + 1, result.title);
                }
            }
            Err(e) => {
                println!("✗ 搜索失败: {:?}", e);
                panic!("搜索关键词 '{}' 测试失败: {:?}", query, e);
            }
        }

        tokio::time::sleep(QUICK_TEST_TIMEOUT).await;
    }

    println!("🎉 Google搜索引擎多种查询测试通过！");
}
