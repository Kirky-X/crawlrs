// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 错误处理模式示例
//!
//! 演示 crawlrs 引擎层各种错误类型及推荐的错误处理策略：
//! - `EngineError::Timeout` — 请求超时，可重试
//! - `EngineError::SsrfProtection` — SSRF 防护触发，不应重试
//! - `EngineError::AntiBotDetected` — 反爬虫检测，需切换策略
//! - `EngineError::AllEnginesFailed` — 所有引擎失败，需降级
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example error_handling
//! ```

use crawlrs::engines::engine_client::{EngineClient, EngineError, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 错误处理模式示例");
    info!("=====================================\n");

    let client = EngineClient::new();

    // 1. 基本错误匹配
    info!("1️⃣  基本错误处理");
    info!("-----------------------------");

    let request = ScrapeRequest::new("https://this-domain-does-not-exist.invalid")
        .timeout(Duration::from_secs(5));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("  爬取成功: HTTP {}", response.status_code);
        }
        Err(e) => handle_engine_error(&e),
    }

    info!("");

    // 2. 超时错误演示
    info!("2️⃣  超时错误处理");
    info!("-----------------------------");

    // 极短超时触发 Timeout
    let request =
        ScrapeRequest::new("https://httpbin.org/delay/10").timeout(Duration::from_millis(100));

    match client.scrape(&request).await {
        Ok(response) => {
            info!("  爬取成功: HTTP {}", response.status_code);
        }
        Err(EngineError::Timeout(duration)) => {
            info!("  ⏰ 请求超时: {:?}", duration);
            info!("  💡 策略: 增加超时时间后重试");
        }
        Err(e) => {
            info!("  其他错误: {:?}", e);
        }
    }

    info!("");

    // 3. 无效 URL 错误
    info!("3️⃣  无效 URL 处理");
    info!("-----------------------------");

    let request = ScrapeRequest::new("not-a-valid-url");

    match client.scrape(&request).await {
        Ok(_) => info!("  意外成功"),
        Err(EngineError::InvalidUrl(url)) => {
            info!("  🚫 无效 URL: {}", url);
            info!("  💡 策略: 在提交前使用 URL 验证");
        }
        Err(e) => info!("  其他错误: {:?}", e),
    }

    info!("");

    // 4. 带重试的错误恢复模式
    info!("4️⃣  重试模式演示");
    info!("-----------------------------");

    let max_retries = 3;
    let url = "https://httpbin.org/status/500";
    let mut last_error = None;

    for attempt in 1..=max_retries {
        let request = ScrapeRequest::new(url).timeout(Duration::from_secs(10));

        match client.scrape(&request).await {
            Ok(response) => {
                info!(
                    "  ✅ 第 {} 次尝试成功: HTTP {}",
                    attempt, response.status_code
                );
                last_error = None;
                break;
            }
            Err(e) => {
                info!("  ❌ 第 {} 次尝试失败: {:?}", attempt, e);
                last_error = Some(e);

                if attempt < max_retries {
                    let backoff = Duration::from_millis(500 * attempt as u64);
                    info!("  🔄 等待 {:?} 后重试...", backoff);
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    if let Some(e) = last_error {
        info!("  ⚠️  所有 {} 次重试均失败，最终错误: {:?}", max_retries, e);
    }

    info!("\n=====================================");
    info!("✨ 错误处理模式示例完成");
    info!("");
    info!("💡 最佳实践:");
    info!("   - Timeout / AntiBotDetected / EngineMrtExceeded 属于可重试错误");
    info!("   - InvalidUrl / SsrfProtection 不应重试，应在调用前修正");
    info!("   - AllEnginesFailed 表示系统级故障，考虑降级或报警");
    info!("   - 使用指数退避避免重试风暴");
}

/// 根据 EngineError 类型给出不同的处理策略
fn handle_engine_error(error: &EngineError) {
    match error {
        EngineError::Timeout(duration) => {
            info!("  ⏰ 超时: {:?}", duration);
            info!("  💡 增加超时或检查目标服务器状态");
        }
        EngineError::InvalidUrl(url) => {
            info!("  🚫 无效 URL: {}", url);
            info!("  💡 在提交前验证 URL 格式");
        }
        EngineError::SsrfProtection(reason) => {
            info!("  🛡️ SSRF 防护: {}", reason);
            info!("  💡 目标 URL 触发了 SSRF 安全策略，不应重试");
        }
        EngineError::AntiBotDetected(reason) => {
            info!("  🤖 反爬虫检测: {}", reason);
            info!("  💡 切换 User-Agent / 启用代理 / 使用浏览器引擎");
        }
        EngineError::AllEnginesFailed(reason) => {
            info!("  💥 所有引擎失败: {}", reason);
            info!("  💡 检查网络连接或引擎健康状态");
        }
        EngineError::NoEnginesAvailable => {
            info!("  ⚠️ 无可用引擎");
            info!("  💡 检查引擎配置");
        }
        EngineError::BrowserError(msg) => {
            info!("  🌐 浏览器错误: {}", msg);
            info!("  💡 检查浏览器实例是否正常运行");
        }
        EngineError::EngineMrtExceeded { engine, mrt } => {
            info!("  ⏰ 引擎 {} 超过 MRT {:?}", engine, mrt);
            info!("  💡 路由层会自动 fallback 到下一引擎");
        }
        other => {
            info!("  ❓ 其他错误: {:?}", other);
        }
    }
}
