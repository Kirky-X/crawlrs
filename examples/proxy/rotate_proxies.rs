// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理轮换示例
//!
//! 演示 crawlrs 的 ProxyPool 代理轮换机制：
//! - RoundRobin 轮询分配
//! - Sticky 粘性会话
//! - 健康检查与冷却恢复
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example rotate_proxies
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 代理轮换示例");
    info!("=====================================\n");

    // 1. 代理池创建
    info!("1️⃣  代理池创建");
    info!("-----------------------------");
    info!("  ProxyPool 支持从 URL 列表快速创建:");
    info!("");
    info!("  let pool = ProxyPool::from_urls(");
    info!("    vec![");
    info!("      \"http://proxy1:8080\".to_string(),");
    info!("      \"http://proxy2:8080\".to_string(),");
    info!("      \"http://proxy3:8080\".to_string(),");
    info!("    ],");
    info!("    Duration::from_secs(300),  // sticky TTL");
    info!("    Duration::from_secs(60),   // 失败冷却时长");
    info!("  );");
    info!("");

    // 2. RoundRobin 轮换
    info!("2️⃣  RoundRobin 轮换");
    info!("-----------------------------");
    info!("  每次调用 pool.next(category) 按顺序取下一个可用代理:");
    info!("");
    info!("  请求 1 → proxy1:8080");
    info!("  请求 2 → proxy2:8080");
    info!("  请求 3 → proxy3:8080");
    info!("  请求 4 → proxy1:8080  (循环)");
    info!("");
    info!("  如果某个代理进入冷却:");
    info!("  请求 1 → proxy1:8080");
    info!("  请求 2 → proxy2:8080");
    info!("  请求 3 → proxy2:8080  (proxy3 冷却中，跳过)");
    info!("  请求 4 → proxy1:8080");
    info!("");

    // 3. Sticky 粘性会话
    info!("3️⃣  Sticky 粘性会话");
    info!("-----------------------------");
    info!("  同一 session_id 在 TTL 内固定使用同一代理:");
    info!("");
    info!("  pool.sticky(\"session-abc\") → proxy1:8080");
    info!("  pool.sticky(\"session-abc\") → proxy1:8080  (同一代理)");
    info!("  pool.sticky(\"session-xyz\") → proxy2:8080  (不同 session)");
    info!("");
    info!("  在 scrape 请求中使用:");
    info!("  ScrapeOptions::builder()");
    info!("    .session_id(\"my-session-id\")");
    info!("    .build()");
    info!("");

    // 4. 健康检查与冷却
    info!("4️⃣  健康检查与冷却恢复");
    info!("-----------------------------");
    info!("  失败处理:");
    info!("    pool.mark_failure(\"http://proxy1:8080\");");
    info!("    → proxy1 进入冷却期（默认 60s）");
    info!("    → 冷却期间 next()/sticky() 跳过该代理");
    info!("");
    info!("  成功恢复:");
    info!("    pool.mark_success(\"http://proxy1:8080\");");
    info!("    → proxy1 立即恢复健康");
    info!("");
    info!("  自动恢复:");
    info!("    → 冷却期结束后代理自动恢复可用");
    info!("    → 全部代理冷却时返回 None（不 panic）");
    info!("");
    info!("  💡 配置建议:");
    info!("     - cooldown_seconds: 60-120s（给代理足够恢复时间）");
    info!("     - sticky_ttl: 300s（5 分钟内维持会话一致性）");
    info!("     - 代理池至少 3 个以上代理保证可用性");

    info!("\n=====================================");
    info!("✨ 代理轮换示例完成");
}
