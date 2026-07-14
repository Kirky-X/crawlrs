// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 积分管理示例
//!
//! 演示如何管理团队积分配额和使用计费。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin credits_management
//! ```
//!
//! ## 核心功能
//!
//! - 积分充值和分配
//! - 使用计费（按请求/数据量）
//! - 配额监控和警告

use log::{info, warn};
use uuid::Uuid;

// 积分交易类型
#[derive(Debug, Clone)]
enum TransactionType {
    Credit,
    Debit,
}

// 积分交易记录
#[derive(Debug, Clone)]
struct CreditTransaction {
    id: Uuid,
    team_id: Uuid,
    amount: i64,
    transaction_type: TransactionType,
    description: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

// 团队积分管理器
#[derive(Debug)]
struct CreditsManager {
    team_id: Uuid,
    balance: i64,
    transactions: Vec<CreditTransaction>,
    daily_limit: i64,
    monthly_limit: i64,
}

impl CreditsManager {
    pub fn new(team_id: Uuid, initial_credits: i64, daily_limit: i64, monthly_limit: i64) -> Self {
        Self {
            team_id,
            balance: initial_credits,
            transactions: Vec::new(),
            daily_limit,
            monthly_limit,
        }
    }

    // 消费积分
    pub fn consume(&mut self, amount: i64, description: &str) -> Result<(), String> {
        if self.balance < amount {
            return Err(format!(
                "Insufficient credits. Required: {}, Available: {}",
                amount, self.balance
            ));
        }

        self.balance -= amount;
        self.transactions.push(CreditTransaction {
            id: Uuid::new_v4(),
            team_id: self.team_id,
            amount: -amount,
            transaction_type: TransactionType::Debit,
            description: description.to_string(),
            created_at: chrono::Utc::now(),
        });

        info!("Consumed {} credits. Remaining: {}", amount, self.balance);
        Ok(())
    }

    // 充值积分
    pub fn deposit(&mut self, amount: i64, description: &str) {
        self.balance += amount;
        self.transactions.push(CreditTransaction {
            id: Uuid::new_v4(),
            team_id: self.team_id,
            amount,
            transaction_type: TransactionType::Credit,
            description: description.to_string(),
            created_at: chrono::Utc::now(),
        });

        info!(
            "Deposited {} credits. New balance: {}",
            amount, self.balance
        );
    }

    // 获取余额
    pub fn get_balance(&self) -> i64 {
        self.balance
    }

    // 获取交易历史
    pub fn get_history(&self, limit: usize) -> &[CreditTransaction] {
        &self.transactions[..std::cmp::min(limit, self.transactions.len())]
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("=== 积分管理示例 ===\n");

    let team_id = Uuid::new_v4();
    let mut manager = CreditsManager::new(team_id, 10000, 1000, 30000);

    info!("Created credits manager for team: {}", team_id);
    info!("Initial balance: {}\n", manager.get_balance());

    // 模拟使用场景
    info!("--- 模拟使用场景 ---");

    // 抓取一个网页
    info!("1. 抓取网页 (1 credit)");
    let _ = manager.consume(1, "Web scrape: https://example.com");

    // 批量抓取
    info!("2. 批量抓取 100 个页面 (100 credits)");
    let _ = manager.consume(100, "Batch scrape: 100 pages");

    // 使用高级功能（消耗更多积分）
    info!("3. 使用 LLM 提取 (50 credits)");
    let _ = manager.consume(50, "LLM extraction: product data");

    // 尝试超出余额的消费
    info!("4. 尝试大额消费 (20000 credits)");
    if let Err(e) = manager.consume(20000, "Large batch operation") {
        warn!("Failed: {}", e);
    }

    // 充值
    info!("\n5. 充值 5000 积分");
    manager.deposit(5000, "Monthly top-up");

    // 显示交易历史
    info!("\n--- 交易历史 ---");
    for tx in manager.get_history(10) {
        let sign = match tx.transaction_type {
            TransactionType::Credit => "+",
            TransactionType::Debit => "-",
        };
        info!(
            "  [{}] {} {} - {}",
            tx.created_at.format("%H:%M:%S"),
            sign,
            tx.amount,
            tx.description
        );
    }

    info!("\nFinal balance: {}", manager.get_balance());
    info!("\n=== 积分管理示例完成 ===");
}
