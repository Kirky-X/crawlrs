// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 自定义请求头示例
//!
//! 演示如何配置自定义HTTP请求头，包括：
//! - User-Agent
//! - Accept-Language
//! - 自定义Header
//! - Cookie设置
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example custom_headers
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::collections::HashMap;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始自定义请求头示例");
    info!("=====================================\n");

    let client = EngineClient::new();
    let url = "https://httpbin.org/headers";

    info!("🎯 目标: {}", url);
    info!("（httpbin.org/headers 会返回我们发送的所有请求头）");
    info!("");

    // 1. 基本请求（无自定义头）
    info!("1️⃣  基本请求（默认请求头）");
    info!("-----------------------------");
    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 默认请求头:");
            info!(
                "  {}",
                response
                    .content
                    .lines()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
        }
        Err(e) => info!("❌ 请求失败: {:?}", e),
    }
    info!("");

    // 2. 自定义User-Agent
    info!("2️⃣  自定义User-Agent");
    info!("-----------------------------");
    let mut headers1 = HashMap::new();
    headers1.insert(
        "User-Agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string(),
    );

    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(_) => {
            info!("✅ 自定义User-Agent已发送");
            info!("  响应包含我们设置的头信息");
        }
        Err(e) => info!("❌ 请求失败: {:?}", e),
    }
    info!("");

    // 3. 完整自定义头
    info!("3️⃣  完整自定义请求头配置");
    info!("-----------------------------");

    let custom_headers = vec![
        (
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        ),
        (
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        ),
        ("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7"),
        ("Accept-Encoding", "gzip, deflate, br"),
        ("Connection", "keep-alive"),
        ("Upgrade-Insecure-Requests", "1"),
        ("Cache-Control", "max-age=0"),
    ];

    info!("📋 配置的请求头:");
    for (name, value) in &custom_headers {
        info!("  {}: {}", name, value);
    }
    info!("");

    // 4. 模拟发送带自定义头的请求
    info!("4️⃣  执行带自定义头的请求");
    info!("-----------------------------");

    let mut headers = HashMap::new();
    for (name, value) in custom_headers {
        headers.insert(name.to_string(), value.to_string());
    }

    let request = ScrapeRequest::new(url).timeout(Duration::from_secs(30));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("✅ 请求成功发送");
            info!("  状态码: {}", response.status_code);
            info!("  响应长度: {} 字节", response.content.len());
        }
        Err(e) => info!("❌ 请求失败: {:?}", e),
    }
    info!("");

    // 5. 常见场景示例
    info!("5️⃣  常见场景的头配置");
    info!("-----------------------------");

    // 移动设备模拟
    info!("📱 移动设备模拟:");
    let mut mobile_headers = HashMap::new();
    mobile_headers.insert(
        "User-Agent".to_string(),
        "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X)".to_string(),
    );
    mobile_headers.insert(
        "Accept".to_string(),
        "text/html,application/xhtml+xml,application/xml;q=0.9".to_string(),
    );
    info!(
        "  User-Agent: {}",
        mobile_headers.get("User-Agent").unwrap()
    );
    info!("");

    // API请求
    info!("🔌 API请求:");
    let mut api_headers = HashMap::new();
    api_headers.insert("User-Agent".to_string(), "MyApp/1.0".to_string());
    api_headers.insert("Accept".to_string(), "application/json".to_string());
    api_headers.insert("Content-Type".to_string(), "application/json".to_string());
    info!(
        "  Content-Type: {}",
        api_headers.get("Content-Type").unwrap()
    );
    info!("");

    // 登录态请求
    info!("🔐 带认证的请求:");
    let mut auth_headers = HashMap::new();
    auth_headers.insert(
        "Cookie".to_string(),
        "session=abc123; user_id=456".to_string(),
    );
    auth_headers.insert("Authorization".to_string(), "Bearer token789".to_string());
    info!("  Cookie: session=***; user_id=***");
    info!("  Authorization: Bearer ***");

    info!("\n=====================================");
    info!("✨ 自定义请求头示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 使用有意义的User-Agent帮助服务器识别客户端");
    info!("   - 设置合适的Accept头以获取期望的响应格式");
    info!("   - 使用Accept-Language设置语言偏好");
    info!("   - 敏感信息（如Cookie）应妥善保管");
}
