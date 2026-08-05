// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 团队隔离示例
//!
//! 演示 crawlrs 多租户架构下的团队资源隔离机制：
//! - 每个团队拥有独立的 API Key、配额和并发控制
//! - 通过 `team_id` 实现数据隔离
//! - 团队级并发信号量控制
//! - 地理围栏（Geo Restriction）配置
//!
//! # 前提条件
//!
//! 需要启用 `teams` feature。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example team_isolation
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 团队隔离示例");
    info!("=====================================\n");

    let base_url = "http://localhost:8899";

    // 1. 多租户架构概览
    info!("1️⃣  多租户架构概览");
    info!("-----------------------------");
    info!("  crawlrs 通过 team_id 实现多租户隔离:");
    info!("");
    info!("  ┌─────────────┐   ┌─────────────┐");
    info!("  │  Team A      │   │  Team B      │");
    info!("  │  API Keys    │   │  API Keys    │");
    info!("  │  Credits     │   │  Credits     │");
    info!("  │  Rate Limits │   │  Rate Limits │");
    info!("  │  Concurrency │   │  Concurrency │");
    info!("  └─────────────┘   └─────────────┘");
    info!("        │                   │");
    info!("        └───────┬───────────┘");
    info!("                │");
    info!("        ┌───────┴───────┐");
    info!("        │  crawlrs API  │");
    info!("        │  PostgreSQL   │");
    info!("        │  Redis        │");
    info!("        └───────────────┘");
    info!("");

    // 2. 团队级 API Key
    info!("2️⃣  团队级 API Key");
    info!("-----------------------------");
    info!("  创建属于 Team A 的 API Key:");
    info!("  POST {}/v1/admin/api-keys", base_url);
    info!("  {{");
    info!("    \"team_id\": \"<team-a-uuid>\",");
    info!("    \"name\": \"team-a-collector\",");
    info!("    \"scope\": {{ \"read\": true, \"write\": true }}");
    info!("  }}");
    info!("");
    info!("  使用 Team A 的 Key 发起请求时:");
    info!("    - Auth 中间件自动解析 team_id");
    info!("    - 所有操作限定在 Team A 的数据范围内");
    info!("    - 无法访问 Team B 的爬取结果或任务");
    info!("");

    // 3. 团队并发控制
    info!("3️⃣  团队并发控制");
    info!("-----------------------------");
    info!("  crawlrs 使用 TeamSemaphore 实现团队级并发控制:");
    info!("");
    info!("  - 每个团队有独立的并发信号量");
    info!("  - 超过团队并发上限时请求排队等待");
    info!("  - 避免单个团队耗尽系统资源");
    info!("");
    info!("  配置方式（config/default.toml）:");
    info!("  [teams]");
    info!("  max_concurrent_requests = 10  # 每团队最大并发");
    info!("");

    // 4. 地理围栏
    info!("4️⃣  地理围栏（Geo Restriction）");
    info!("-----------------------------");
    info!("  团队可配置地理围栏，限制可访问的地域范围:");
    info!("");
    info!("  查看团队地理围栏:");
    info!("  GET {}/v1/teams/geo-restrictions", base_url);
    info!("  Authorization: Bearer <team-api-key>");
    info!("");
    info!("  更新地理围栏:");
    info!("  PUT {}/v1/teams/geo-restrictions", base_url);
    info!("  {{");
    info!("    \"allowed_countries\": [\"US\", \"CN\", \"JP\"],");
    info!("    \"blocked_countries\": [\"KP\"]");
    info!("  }}");
    info!("");
    info!("  💡 地理围栏在 extract 路由中自动生效，");
    info!("     请求目标位于被封锁地区时返回 403。");
    info!("");

    // 5. 团队配额管理
    info!("5️⃣  团队配额（Credits）管理");
    info!("-----------------------------");
    info!("  每个团队有独立的 credits 余额:");
    info!("  - 每次爬取/搜索操作消耗 credits");
    info!("  - credits 耗尽时拒绝新请求");
    info!("  - 管理员可通过工具充值 credits");
    info!("");
    info!("  💡 详见 credits_management 示例");

    info!("\n=====================================");
    info!("✨ 团队隔离示例完成");
}
