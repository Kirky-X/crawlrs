// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理认证示例
//!
//! 演示如何在 crawlrs 中使用需要认证的代理服务器：
//! - URL 内嵌认证（user:pass@host）
//! - 代理认证失败处理
//! - 代理健康检查与冷却机制
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example proxy_auth
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 代理认证示例");
    info!("=====================================\n");

    // 1. 代理认证格式
    info!("1️⃣  代理认证格式");
    info!("-----------------------------");
    info!("  crawlrs 支持在代理 URL 中内嵌认证信息:");
    info!("");
    info!("  http://username:password@proxy.example.com:8080");
    info!("  https://username:password@proxy.example.com:8080");
    info!("  socks5://username:password@proxy.example.com:1080");
    info!("");
    info!("  配置方式:");
    info!("  [proxy]");
    info!("  urls = [");
    info!("    \"http://user1:pass1@proxy1:8080\",");
    info!("    \"http://user2:pass2@proxy2:8080\"");
    info!("  ]");
    info!("");

    // 2. 安全脱敏
    info!("2️⃣  日志脱敏");
    info!("-----------------------------");
    info!("  crawlrs 在日志输出代理 URL 时自动脱敏 userinfo:");
    info!("");
    info!("  原始 URL: http://admin:secret123@proxy.example.com:8080");
    info!("  日志输出: http://[REDACTED]@proxy.example.com:8080");
    info!("");
    info!("  这防止了 CWE-532（日志中暴露敏感信息）");
    info!("");

    // 3. 认证失败处理
    info!("3️⃣  认证失败处理");
    info!("-----------------------------");
    info!("  代理认证失败时的行为:");
    info!("");
    info!("  1. 引擎返回错误（HTTP 407 Proxy Authentication Required）");
    info!("  2. ProxyPool 自动标记该代理为不健康");
    info!("  3. 代理进入冷却期（cooldown_seconds）");
    info!("  4. 后续请求自动切换到下一个可用代理");
    info!("  5. 冷却期过后自动恢复重试");
    info!("");

    // 4. 最佳实践
    info!("4️⃣  代理认证最佳实践");
    info!("-----------------------------");
    info!("  ✅ 推荐:");
    info!("     - 使用环境变量存储代理凭据");
    info!("     - 定期轮换代理密码");
    info!("     - 使用 HTTPS 代理加密传输凭据");
    info!("     - 监控代理认证失败率");
    info!("");
    info!("  ❌ 避免:");
    info!("     - 在代码中硬编码代理凭据");
    info!("     - 在日志或错误消息中暴露凭据");
    info!("     - 多个服务共用同一代理账号");
    info!("");
    info!("  💡 环境变量方式:");
    info!("     PROXY_URL=http://$PROXY_USER:$PROXY_PASS@proxy:8080");

    info!("\n=====================================");
    info!("✨ 代理认证示例完成");
}
