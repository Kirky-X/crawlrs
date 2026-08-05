// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础限流配置示例
//!
//! 演示 crawlrs 的限流系统配置：
//! - 限流策略（TokenBucket / LeakyBucket / FixedWindow / SlidingWindow）
//! - 速率配置（每秒/每分钟/每小时）
//! - 通过配置文件和环境变量设置限流
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_rate_limit
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 基础限流配置示例");
    info!("=====================================\n");

    // 1. 限流策略
    info!("1️⃣  限流策略");
    info!("-----------------------------");
    info!("  crawlrs 支持 4 种限流算法:");
    info!("");
    info!("  TokenBucket   — 令牌桶（默认）");
    info!("    允许突发流量，桶满时可一次消费多个令牌");
    info!("    适用: 一般 API 限流");
    info!("");
    info!("  LeakyBucket   — 漏桶");
    info!("    平滑输出，严格控制请求速率");
    info!("    适用: 保护下游服务");
    info!("");
    info!("  FixedWindow   — 固定窗口");
    info!("    按固定时间窗口计数（如每分钟 100 次）");
    info!("    适用: 简单的速率限制");
    info!("");
    info!("  SlidingWindow — 滑动窗口");
    info!("    平滑的窗口计数，避免窗口边界突发");
    info!("    适用: 精确的速率控制");
    info!("");

    // 2. 配置方式
    info!("2️⃣  限流配置（config/default.toml）");
    info!("-----------------------------");
    info!("  [rate_limiting]");
    info!("  strategy = \"token_bucket\"");
    info!("  requests_per_second = 10");
    info!("  requests_per_minute = 100");
    info!("  requests_per_hour = 1000");
    info!("  bucket_capacity = 100");
    info!("  enabled = true");
    info!("");

    // 3. 团队级限流
    info!("3️⃣  团队级限流");
    info!("-----------------------------");
    info!("  crawlrs 支持按团队独立限流:");
    info!("  - 每个团队有独立的速率计数器");
    info!("  - 通过 API Key 关联团队身份");
    info!("  - 超限返回 HTTP 429 Too Many Requests");
    info!("");

    // 4. 环境变量覆盖
    info!("4️⃣  环境变量覆盖");
    info!("-----------------------------");
    info!("  CRAWLRS__RATE_LIMITING__STRATEGY=sliding_window");
    info!("  CRAWLRS__RATE_LIMITING__REQUESTS_PER_SECOND=20");
    info!("  CRAWLRS__RATE_LIMITING__REQUESTS_PER_MINUTE=200");
    info!("  CRAWLRS__RATE_LIMITING__ENABLED=true");
    info!("");
    info!("  💡 限流配置一致性检查:");
    info!("     requests_per_second <= requests_per_minute / 60");
    info!("     requests_per_minute <= requests_per_hour / 60");
    info!("     不一致时启动会报 ValidationError");

    info!("\n=====================================");
    info!("✨ 基础限流配置示例完成");
}
