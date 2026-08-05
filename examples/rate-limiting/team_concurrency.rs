// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 团队并发控制示例
//!
//! 演示 crawlrs 的 `TeamSemaphore` 团队级并发控制机制：
//! - 每个团队独立的并发信号量
//! - 超限排队等待
//! - 与限流中间件的配合
//!
//! # 前提条件
//!
//! 需要启用 `teams` feature。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example team_concurrency
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 团队并发控制示例");
    info!("=====================================\n");

    // 1. 并发控制架构
    info!("1️⃣  团队并发控制架构");
    info!("-----------------------------");
    info!("  crawlrs 使用 TeamSemaphore 实现团队级并发控制:");
    info!("");
    info!("  请求 → Auth 中间件（解析 team_id）");
    info!("       → TeamSemaphore（检查团队并发）");
    info!("         → 有空位 → 立即执行");
    info!("         → 已满   → 排队等待（或返回 429）");
    info!("       → RateLimitMiddleware（检查速率）");
    info!("         → Handler 执行");
    info!("       → 释放信号量");
    info!("");

    // 2. 配置方式
    info!("2️⃣  并发控制配置");
    info!("-----------------------------");
    info!("  config/default.toml:");
    info!("");
    info!("  [teams]");
    info!("  max_concurrent_requests = 10  # 每团队最大并发请求");
    info!("");
    info!("  环境变量覆盖:");
    info!("  CRAWLRS__TEAMS__MAX_CONCURRENT_REQUESTS=20");
    info!("");

    // 3. 工作原理
    info!("3️⃣  工作原理");
    info!("-----------------------------");
    info!("  TeamSemaphore 内部使用 DashMap<team_id, Semaphore>:");
    info!("");
    info!("  - 每个 team_id 首次出现时创建新信号量");
    info!("  - 请求到达时 acquire permit");
    info!("  - 请求完成时 release permit");
    info!("  - 信号量保证同一团队不超过 max_concurrent_requests");
    info!("");
    info!("  并发场景:");
    info!("    Team A: 10 个并发请求 → 全部并行执行");
    info!("    Team A: 第 11 个请求 → 排队等待，直到有请求完成");
    info!("    Team B: 同时 5 个请求 → 独立执行（不受 Team A 影响）");
    info!("");

    // 4. 与限流的配合
    info!("4️⃣  并发控制 vs 速率限制");
    info!("-----------------------------");
    info!("  并发控制（TeamSemaphore）:");
    info!("    - 控制同时执行的请求数");
    info!("    - 保护系统资源（CPU/内存/连接数）");
    info!("    - 请求排队而非拒绝");
    info!("");
    info!("  速率限制（RateLimitMiddleware）:");
    info!("    - 控制单位时间内的请求数");
    info!("    - 保护下游服务不被过载");
    info!("    - 超限直接返回 429");
    info!("");
    info!("  两者互补:");
    info!("    并发控制保证资源不耗尽");
    info!("    速率限制保证流量不过载");
    info!("");
    info!("  💡 调优建议:");
    info!("     - 并发数 = 下游服务可承受的并行度");
    info!("     - 速率 = 下游服务的 QPS 上限");
    info!("     - 两者配合实现完整的流量保护");

    info!("\n=====================================");
    info!("✨ 团队并发控制示例完成");
}
