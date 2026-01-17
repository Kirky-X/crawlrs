// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 积分实体
///
/// 表示团队的积分余额，用于跟踪资源使用情况
///
/// # 安全提示
///
/// `balance` 字段包含敏感的财务信息，仅对 crate 可见。
/// 外部模块应使用 `balance()` 方法读取余额。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credits {
    pub id: Uuid,
    pub team_id: Uuid,
    /// 积分余额 (敏感信息)
    pub(crate) balance: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Credits {
    /// 获取积分余额
    ///
    /// # 安全提示
    ///
    /// 此方法返回积分余额，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn balance(&self) -> i64 {
        self.balance
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditsTransaction {
    pub id: Uuid,
    pub team_id: Uuid,
    pub amount: i64, // Positive for credits added, negative for credits used
    pub transaction_type: CreditsTransactionType,
    pub description: String,
    pub reference_id: Option<Uuid>, // Reference to task, crawl, etc.
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CreditsTransactionType {
    Search,
    Scrape,
    Extract,
    Crawl,
    ManualAdjustment,
    Subscription,
    Refund,
}

impl std::fmt::Display for CreditsTransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreditsTransactionType::Search => write!(f, "search"),
            CreditsTransactionType::Scrape => write!(f, "scrape"),
            CreditsTransactionType::Extract => write!(f, "extract"),
            CreditsTransactionType::Crawl => write!(f, "crawl"),
            CreditsTransactionType::ManualAdjustment => write!(f, "manual_adjustment"),
            CreditsTransactionType::Subscription => write!(f, "subscription"),
            CreditsTransactionType::Refund => write!(f, "refund"),
        }
    }
}
