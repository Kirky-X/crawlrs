// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 作用域验证示例
//!
//! 演示 crawlrs 的 API Key 权限模型（`ApiKeyScope`）：
//! - `read` — 只读权限（查询爬取状态、获取结果）
//! - `write` — 写入权限（创建爬取/爬取任务）
//! - `admin` — 管理权限（签发/撤销 API Key）
//! - `search_limit` / `scrape_limit` — 请求数量限制
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example scope_validation
//! ```

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 作用域验证示例");
    info!("=====================================\n");

    // 1. 权限模型说明
    info!("1️⃣  API Key 权限模型");
    info!("-----------------------------");
    info!("  crawlrs 使用基于标志位的权限模型 (ApiKeyScope):");
    info!("");
    info!("  权限标志:");
    info!("    read   — 查询操作（GET 请求）");
    info!("    write  — 创建操作（POST 请求）");
    info!("    admin  — 管理操作（API Key 签发/撤销）");
    info!("");
    info!("  速率限制:");
    info!("    search_limit — 单次搜索最大结果数");
    info!("    scrape_limit — 单次爬取最大页面数");
    info!("");

    // 2. 预定义 Scope 示例
    info!("2️⃣  预定义 Scope 配置");
    info!("-----------------------------");
    info!("");
    info!("  只读 Scope (read_only):");
    info!("    read=true, write=false, admin=false");
    info!("    search_limit=100, scrape_limit=50");
    info!("    适用: 数据查看、监控面板");
    info!("");
    info!("  完全访问 Scope (full_access):");
    info!("    read=true, write=true, admin=true");
    info!("    search_limit=u32::MAX, scrape_limit=u32::MAX");
    info!("    适用: 管理员、系统级操作");
    info!("");
    info!("  自定义 Scope:");
    info!("    read=true, write=true, admin=false");
    info!("    search_limit=500, scrape_limit=200");
    info!("    适用: 普通业务应用");
    info!("");

    // 3. 权限检查流程
    info!("3️⃣  权限检查流程");
    info!("-----------------------------");
    info!("  请求 → Auth 中间件 → 解析 API Key → 加载 Scope → 权限校验");
    info!("");
    info!("  权限校验规则:");
    info!("    GET  /v1/scrape/{{id}}  → 需要 read 权限");
    info!("    POST /v1/scrape       → 需要 write 权限");
    info!("    POST /v1/crawl        → 需要 write 权限");
    info!("    POST /v1/admin/api-keys → 需要 admin 权限");
    info!("");
    info!("  权限不足时返回 403 Forbidden，附带缺失权限说明。");
    info!("");

    // 4. 创建自定义 Scope 的 API Key
    info!("4️⃣  创建自定义 Scope 的 API Key");
    info!("-----------------------------");
    info!("  POST /v1/admin/api-keys");
    info!("  {{");
    info!("    \"team_id\": \"<team-uuid>\",");
    info!("    \"name\": \"readonly-monitoring\",");
    info!("    \"scope\": {{");
    info!("      \"read\": true,");
    info!("      \"write\": false,");
    info!("      \"admin\": false,");
    info!("      \"search_limit\": 50,");
    info!("      \"scrape_limit\": 20");
    info!("    }}");
    info!("  }}");
    info!("");
    info!("  💡 最佳实践:");
    info!("     - 为每个集成创建最小权限的 API Key");
    info!("     - 监控类应用只需 read 权限");
    info!("     - 数据采集应用需要 write 权限但不需要 admin");
    info!("     - 定期审计 API Key 的权限范围");

    info!("\n=====================================");
    info!("✨ 作用域验证示例完成");
}
