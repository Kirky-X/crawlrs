// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 自定义引擎集成示例
//!
//! 演示如何集成自定义爬取引擎。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin custom_engines
//! ```

use log::info;
use std::time::Duration;

// 模拟的错误类型
#[derive(Debug, thiserror::Error)]
enum CustomEngineError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Timeout")]
    Timeout,
}

// 模拟的爬取请求
#[derive(Debug, Clone)]
struct ScrapeRequest {
    url: String,
    timeout_ms: u64,
}

// 模拟的爬取响应
#[derive(Debug)]
struct ScrapeResponse {
    url: String,
    status_code: u16,
    content: String,
    success: bool,
}

// 自定义 HTTP 引擎
#[derive(Debug, Clone)]
struct CustomHttpEngine {
    name: String,
    timeout: Duration,
}

impl CustomHttpEngine {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn scrape(
        &self,
        request: &ScrapeRequest,
    ) -> Result<ScrapeResponse, CustomEngineError> {
        info!("Engine '{}' scraping: {}", self.name, request.url);

        // 验证 URL
        if !request.url.starts_with("http://") && !request.url.starts_with("https://") {
            return Err(CustomEngineError::InvalidUrl(request.url.clone()));
        }

        // 模拟异步爬取延迟
        tokio::time::sleep(Duration::from_millis(50)).await;

        // 构建响应
        let response = ScrapeResponse {
            url: request.url.clone(),
            status_code: 200,
            content: format!(
                r#"<!DOCTYPE html>
<html>
<head><title>{}</title></head>
<body>
<h1>Custom Engine Result</h1>
<p>URL: {}</p>
<p>Engine: {}</p>
</body>
</html>"#,
                request.url, request.url, self.name
            ),
            success: true,
        };

        Ok(response)
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("=== 自定义引擎集成示例 ===\n");

    // 创建引擎
    let engine = CustomHttpEngine::new("MyCustomHttp");
    info!("Created engine: {}", engine.name());

    // 创建请求
    let request = ScrapeRequest {
        url: "https://example.com/page1".to_string(),
        timeout_ms: 30000,
    };

    // 使用引擎
    info!("\n--- 使用自定义引擎 ---");
    match engine.scrape(&request).await {
        Ok(response) => {
            info!("✅ Scraped successfully!");
            info!("   Status: {}", response.status_code);
            info!("   Content length: {} bytes", response.content.len());
        }
        Err(e) => {
            info!("❌ Scraping failed: {:?}", e);
        }
    }

    // 测试错误处理
    info!("\n--- 测试错误处理 ---");
    let bad_request = ScrapeRequest {
        url: "ftp://invalid-protocol.com".to_string(),
        timeout_ms: 5000,
    };

    if let Err(e) = engine.scrape(&bad_request).await {
        info!("✅ Caught expected error: {}", e);
    }

    info!("\n=== 自定义引擎示例完成 ===");
}
