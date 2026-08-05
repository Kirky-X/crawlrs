// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! oxcache 内存缓存集成示例
//!
//! 演示 crawlrs 内部使用的 oxcache 缓存系统：
//! - 创建不同类型的缓存实例（搜索缓存、DNS 缓存、正则缓存）
//! - 基本的 get/set/delete/exists 操作
//! - 缓存容量和淘汰策略
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example oxcache_cache
//! ```

use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 oxcache 内存缓存示例");
    info!("=====================================\n");

    // 1. 创建缓存实例
    info!("1️⃣  创建缓存实例");
    info!("-----------------------------");

    // crawlrs 内部使用 oxcache 创建缓存
    // 以下为概念演示，展示 crawlrs 的缓存创建方式
    let capacity: u64 = 1000;
    let ttl = Duration::from_secs(300);

    info!("  缓存容量: {}", capacity);
    info!("  默认 TTL: {:?}", ttl);
    info!("");

    // 2. 缓存类型
    info!("2️⃣  crawlrs 内置缓存类型");
    info!("-----------------------------");
    info!("");
    info!("  SearchCache — 搜索结果缓存");
    info!("    Key 格式: search:<query>:limit=<n>[:lang=<l>][:country=<c>]");
    info!("    Value: Vec<SearchResult>");
    info!("    用途: 避免重复搜索相同查询");
    info!("");
    info!("  DnsCache — DNS 解析缓存");
    info!("    Key 格式: dns:<host>:<port>");
    info!("    Value: DnsCacheEntry {{ ips: Vec<IpAddr>, remaining_ttl_secs }}");
    info!("    用途: 减少 DNS 查询，加速 SSRF 校验");
    info!("");
    info!("  RegexCache — 正则编译缓存");
    info!("    Key 格式: regex:<hash>");
    info!("    Value: String（正则模式字符串）");
    info!("    用途: 避免重复编译相同正则");
    info!("");

    // 3. 缓存操作演示
    info!("3️⃣  缓存操作（概念演示）");
    info!("-----------------------------");
    info!("");
    info!("  // 写入缓存");
    info!("  cache.set(\"search:rust:limit=10\", &results).await?;");
    info!("");
    info!("  // 读取缓存");
    info!("  let cached = cache.get(\"search:rust:limit=10\").await?;");
    info!("  match cached {{");
    info!("    Some(results) => info!(\"缓存命中! {{}} 条结果\", results.len()),");
    info!("    None => info!(\"缓存未命中，执行实际搜索\"),");
    info!("  }}");
    info!("");
    info!("  // 检查存在");
    info!("  if cache.exists(\"search:rust:limit=10\").await? {{ ... }}");
    info!("");
    info!("  // 删除缓存");
    info!("  cache.delete(\"search:rust:limit=10\").await?;");
    info!("");

    // 4. 淘汰策略
    info!("4️⃣  淘汰策略");
    info!("-----------------------------");
    info!("  oxcache 基于容量和 TTL 自动淘汰:");
    info!("");
    info!("  - 容量淘汰: 超过 capacity 时自动淘汰最久未使用的条目");
    info!("  - TTL 过期: 条目超过 TTL 后自动失效");
    info!("  - 无需手动清理，缓存自动管理生命周期");
    info!("");
    info!("  💡 性能提示:");
    info!("     - oxcache 使用分片锁，并发读写性能优秀");
    info!("     - 建议 capacity 设为预期峰值的 1.5 倍");
    info!("     - 不同类型的缓存应设置不同的 TTL");

    info!("\n=====================================");
    info!("✨ oxcache 内存缓存示例完成");
}
