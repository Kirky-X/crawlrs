// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! HTTP 代理配置示例
//!
//! 演示如何在 crawlrs 中配置和使用 HTTP 代理：
//! - 通过配置文件设置代理池
//! - 通过请求级别指定代理
//! - 代理策略（RoundRobin / Sticky）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example http_proxy
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 HTTP 代理配置示例");
    info!("=====================================\n");

    // 1. 请求级别代理
    info!("1️⃣  请求级别代理配置");
    info!("-----------------------------");
    info!("  通过 ScrapeOptions 为单次请求指定代理:");
    info!("");
    info!("  let request = ScrapeRequest::new(\"https://example.com\")");
    info!("    .with_options(");
    info!("      ScrapeOptions::builder()");
    info!("        .proxy(\"http://user:pass@proxy.example.com:8080\")");
    info!("        .timeout(Duration::from_secs(30))");
    info!("        .build()");
    info!("    );");
    info!("");
    info!("  或通过 API 请求:");
    info!("  POST /v1/scrape");
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"options\": {{");
    info!("      \"proxy\": \"http://user:pass@proxy.example.com:8080\"");
    info!("    }}");
    info!("  }}");
    info!("");

    // 2. 全局代理池配置
    info!("2️⃣  全局代理池配置（config/default.toml）");
    info!("-----------------------------");
    info!("  [proxy]");
    info!("  urls = [");
    info!("    \"http://proxy1:8080\",");
    info!("    \"http://proxy2:8080\",");
    info!("    \"http://user:pass@proxy3:8080\"");
    info!("  ]");
    info!("  strategy = \"round_robin\"  # 或 \"sticky\"");
    info!("  sticky_ttl_seconds = 300");
    info!("  cooldown_seconds = 60");
    info!("");

    // 3. 代理策略说明
    info!("3️⃣  代理策略");
    info!("-----------------------------");
    info!("  RoundRobin — 轮询分配，每次请求使用不同代理");
    info!("    适用: 分散负载，避免单代理被封");
    info!("");
    info!("  Sticky     — 粘性会话，同一 session_id 固定使用同一代理");
    info!("    适用: 需要维持登录状态的场景");
    info!("    配置: 在请求中设置 session_id");
    info!("");

    // 4. 使用 EngineClient 演示
    info!("4️⃣  EngineClient 代理使用");
    info!("-----------------------------");

    let client = EngineClient::new();
    let options = ScrapeOptions::builder()
        .proxy("http://proxy.example.com:8080")
        .timeout(Duration::from_secs(10))
        .build();
    let request = ScrapeRequest::new("https://httpbin.org/ip").with_options(options);

    match client.scrape(&request).await {
        Ok(response) => {
            info!("  ✅ 通过代理爬取成功: HTTP {}", response.status_code);
            let preview = &response.content[..response.content.len().min(200)];
            info!("  响应: {}", preview);
        }
        Err(e) => {
            info!("  ⚠️  代理不可达（示例环境）: {:?}", e);
            info!("  💡 确保代理 URL 正确且可访问");
        }
    }

    info!("\n=====================================");
    info!("✨ HTTP 代理配置示例完成");
    info!("");
    info!("💡 安全提示:");
    info!("   - 代理 URL 中的凭据在日志中会自动脱敏");
    info!("   - 生产环境建议使用 HTTPS 代理");
    info!("   - 定期验证代理池健康状态");
}
