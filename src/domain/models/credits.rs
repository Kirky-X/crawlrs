// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credits {
    pub id: Uuid,
    pub team_id: Uuid,
    pub balance: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
