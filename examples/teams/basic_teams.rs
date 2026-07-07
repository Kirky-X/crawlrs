// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础团队管理示例
//!
//! 演示如何创建和管理团队。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin basic_teams
//! ```
//!
//! ## 核心功能
//!
//! - 团队创建和配置
//! - 团队成员管理
//! - 团队积分配额

use log::{info, warn};
use uuid::Uuid;

// 模拟团队结构（实际使用时请通过依赖注入获取真实服务）
#[derive(Debug, Clone)]
struct Team {
    pub id: Uuid,
    pub name: String,
    pub api_keys: Vec<String>,
    pub credits: i64,
    pub max_concurrent: u32,
    pub rpm_limit: u32,
}

impl Team {
    pub fn new(name: &str, credits: i64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            api_keys: Vec::new(),
            credits,
            max_concurrent: 10,
            rpm_limit: 60,
        }
    }

    pub fn add_api_key(&mut self, key: &str) {
        self.api_keys.push(key.to_string());
        info!("Added API key to team {}: {}", self.name, &key[..8]);
    }

    pub fn consume_credits(&mut self, amount: i64) -> bool {
        if self.credits >= amount {
            self.credits -= amount;
            info!("Consumed {} credits. Remaining: {}", amount, self.credits);
            true
        } else {
            warn!("Insufficient credits! Required: {}, Available: {}", amount, self.credits);
            false
        }
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("=== 基础团队管理示例 ===\n");

    // 创建团队
    let mut team = Team::new("Acme Corp", 10000);
    info!("Created team: {} (ID: {})", team.name, team.id);

    // 管理 API Keys
    info!("\n--- API Key 管理 ---");
    team.add_api_key("crawlrs_sk_abc123def456");
    team.add_api_key("crawlrs_sk_xyz789ghi012");

    // 积分管理
    info!("\n--- 积分管理 ---");
    info!("Initial credits: {}", team.credits);

    // 模拟消耗积分
    team.consume_credits(100);
    team.consume_credits(50);
    team.consume_credits(25);

    // 模拟积分不足
    team.consume_credits(20000);

    // 团队配置
    info!("\n--- 团队配置 ---");
    info!("Max concurrent requests: {}", team.max_concurrent);
    info!("RPM limit: {}", team.rpm_limit);
    info!("API keys count: {}", team.api_keys.len());

    info!("\n=== 团队管理示例完成 ===");
}
