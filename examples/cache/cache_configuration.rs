// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 缓存配置示例
//!
//! 演示 crawlrs 的缓存系统配置，包括：
//! - 内存缓存（oxcache）容量和 TTL 设置
//! - 按类型配置（搜索缓存、DNS 缓存、正则缓存）
//! - 缓存模式（Enabled / Bypass / WriteOnly / ReadOnly / Disabled）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example cache_configuration
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 缓存配置示例");
    info!("=====================================\n");

    // 1. 基础缓存配置
    info!("1️⃣  基础缓存配置（config/default.toml）");
    info!("-----------------------------");
    info!("  [cache]");
    info!("  enabled = true");
    info!("");
    info!("  [cache.memory]");
    info!("  capacity = 10000       # 最大缓存条目数");
    info!("  ttl_seconds = 300      # 默认 TTL（5 分钟）");
    info!("");
    info!("  [cache.types.search]   # 搜索结果缓存");
    info!("  ttl_seconds = 60");
    info!("  max_size = 100");
    info!("");
    info!("  [cache.types.dns]      # DNS 解析缓存");
    info!("  ttl_seconds = 300");
    info!("  max_size = 100");
    info!("");
    info!("  [cache.types.regex]    # 正则编译缓存");
    info!("  ttl_seconds = 600");
    info!("  max_size = 50");
    info!("");

    // 2. 缓存模式说明
    info!("2️⃣  缓存模式（CacheMode）");
    info!("-----------------------------");
    info!("  每次请求可通过 cache_mode 字段控制缓存行为:");
    info!("");
    info!("  Enabled    — 正常读写缓存（默认）");
    info!("  Bypass     — 跳过缓存，直接请求（强制刷新）");
    info!("  WriteOnly  — 只写不读（预热缓存）");
    info!("  ReadOnly   — 只读不写（调试模式）");
    info!("  Disabled   — 完全禁用缓存");
    info!("");
    info!("  在 scrape 请求中指定:");
    info!("  POST /v1/scrape");
    info!("  {{");
    info!("    \"url\": \"https://example.com\",");
    info!("    \"options\": {{");
    info!("      \"cache_mode\": \"bypass\"");
    info!("    }}");
    info!("  }}");
    info!("");

    // 3. 环境变量覆盖
    info!("3️⃣  环境变量覆盖");
    info!("-----------------------------");
    info!("  通过环境变量覆盖缓存配置:");
    info!("");
    info!("  CRAWLRS__CACHE__ENABLED=false          # 禁用缓存");
    info!("  CRAWLRS__CACHE__MEMORY__CAPACITY=5000   # 修改容量");
    info!("  CRAWLRS__CACHE__MEMORY__TTL_SECONDS=600 # 修改 TTL");
    info!("");
    info!("  💡 提示:");
    info!("     - 开发环境可减小 capacity 节省内存");
    info!("     - 生产环境建议增大 capacity 提高命中率");
    info!("     - DNS 缓存 TTL 不宜过长，避免 DNS 变更不及时");

    info!("\n=====================================");
    info!("✨ 缓存配置示例完成");
}
