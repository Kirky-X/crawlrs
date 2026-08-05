// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! TTL 管理示例
//!
//! 演示 crawlrs 缓存系统中 TTL（Time-To-Live）的管理策略：
//! - 不同缓存类型使用不同 TTL
//! - TTL 对数据新鲜度的影响
//! - 通过配置和请求级别控制 TTL
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example ttl_management
//! ```

use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 TTL 管理示例");
    info!("=====================================\n");

    // 1. 各缓存类型的推荐 TTL
    info!("1️⃣  各缓存类型的推荐 TTL");
    info!("-----------------------------");
    info!("");

    let cache_ttls = vec![
        ("搜索结果缓存", Duration::from_secs(60), "搜索时效性要求高"),
        ("DNS 解析缓存", Duration::from_secs(300), "DNS 记录通常 TTL 300s+"),
        ("正则编译缓存", Duration::from_secs(600), "正则不常变化，可长期缓存"),
        ("页面内容缓存", Duration::from_secs(300), "页面内容 5 分钟内视为有效"),
    ];

    for (name, ttl, reason) in &cache_ttls {
        info!("  {} — TTL: {:?} — {}", name, ttl, reason);
    }
    info!("");

    // 2. TTL 配置方式
    info!("2️⃣  TTL 配置方式");
    info!("-----------------------------");
    info!("");
    info!("  方式一：全局配置（config/default.toml）");
    info!("  [cache.types.search]");
    info!("  ttl_seconds = 60");
    info!("");
    info!("  方式二：环境变量覆盖");
    info!("  CRAWLRS__CACHE__TYPES__SEARCH__TTL_SECONDS=120");
    info!("");

    // 3. TTL 与缓存模式配合
    info!("3️⃣  TTL 与缓存模式配合");
    info!("-----------------------------");
    info!("");
    info!("  场景: 需要获取最新数据（绕过缓存）");
    info!("  POST /v1/scrape");
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"options\": {{ \"cache_mode\": \"bypass\" }}");
    info!("  }}");
    info!("");
    info!("  场景: 预热缓存（写入但不读取）");
    info!("  POST /v1/scrape");
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"options\": {{ \"cache_mode\": \"write_only\" }}");
    info!("  }}");
    info!("");

    // 4. TTL 最佳实践
    info!("4️⃣  TTL 最佳实践");
    info!("-----------------------------");
    info!("");
    info!("  ✅ 推荐:");
    info!("     - 搜索缓存: 60-120s（平衡新鲜度和性能）");
    info!("     - DNS 缓存: 300s（与标准 DNS TTL 对齐）");
    info!("     - 静态资源: 600s+（变化频率低）");
    info!("");
    info!("  ❌ 避免:");
    info!("     - TTL 过短（< 10s）— 缓存形同虚设");
    info!("     - TTL 过长（> 3600s）— 数据严重过期");
    info!("     - 所有类型使用同一 TTL — 不同数据有不同时效需求");
    info!("");
    info!("  💡 监控建议:");
    info!("     - 观察缓存命中率（/metrics 端点）");
    info!("     - 命中率 < 50% 考虑增大 capacity 或调整 TTL");
    info!("     - 命中率 > 95% 且数据过时考虑缩短 TTL");

    info!("\n=====================================");
    info!("✨ TTL 管理示例完成");
}
