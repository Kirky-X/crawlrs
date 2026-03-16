// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits domain model - pure domain entity without ORM annotations
//!
//! This module contains the pure domain model for Credits,
//! following Domain-Driven Design principles.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Credits domain model
///
/// Represents a team's credit balance for resource usage tracking.
/// This is a pure domain model without any ORM annotations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Credits {
    /// Unique identifier
    pub id: Uuid,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// Current credit balance (sensitive data)
    balance: i64,
    /// When the record was created
    pub created_at: DateTime<Utc>,
    /// When the record was last updated
    pub updated_at: DateTime<Utc>,
}

impl Credits {
    /// Create a new credits record
    pub fn new(id: Uuid, team_id: Uuid, initial_balance: i64) -> Self {
        let now = Utc::now();
        Self {
            id,
            team_id,
            balance: initial_balance,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a credits record with custom timestamps (for mappers)
    pub fn with_timestamps(
        id: Uuid,
        team_id: Uuid,
        balance: i64,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            team_id,
            balance,
            created_at,
            updated_at,
        }
    }

    /// Get the current balance
    ///
    /// # Security Note
    ///
    /// This method returns the credit balance. Callers should handle
    /// this data carefully and avoid logging or exposing to users.
    pub fn balance(&self) -> i64 {
        self.balance
    }

    /// Check if the team has enough credits
    pub fn has_sufficient_balance(&self, amount: i64) -> bool {
        self.balance >= amount
    }

    /// Deduct credits from the balance
    ///
    /// Returns an error if insufficient balance
    pub fn deduct(&mut self, amount: i64) -> Result<(), CreditsError> {
        if self.balance < amount {
            return Err(CreditsError::InsufficientBalance {
                current: self.balance,
                requested: amount,
            });
        }
        self.balance -= amount;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Add credits to the balance
    pub fn add(&mut self, amount: i64) {
        self.balance += amount;
        self.updated_at = Utc::now();
    }

    /// Set balance directly (for administrative purposes)
    pub fn set_balance(&mut self, new_balance: i64) {
        self.balance = new_balance;
        self.updated_at = Utc::now();
    }
}

/// Credits transaction domain model
///
/// Represents a single credit transaction record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreditsTransaction {
    /// Unique identifier
    pub id: Uuid,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// Transaction amount (positive for additions, negative for deductions)
    pub amount: i64,
    /// Type of transaction
    pub transaction_type: CreditsTransactionType,
    /// Human-readable description
    pub description: String,
    /// Reference to related entity (task, crawl, etc.)
    pub reference_id: Option<Uuid>,
    /// When the transaction occurred
    pub created_at: DateTime<Utc>,
}

impl CreditsTransaction {
    /// Create a new credits transaction
    pub fn new(
        id: Uuid,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Self {
        Self {
            id,
            team_id,
            amount,
            transaction_type,
            description,
            reference_id,
            created_at: Utc::now(),
        }
    }

    /// Create a transaction with custom timestamp (for mappers)
    pub fn with_timestamp(
        id: Uuid,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            team_id,
            amount,
            transaction_type,
            description,
            reference_id,
            created_at,
        }
    }

    /// Check if this is a deduction transaction
    pub fn is_deduction(&self) -> bool {
        self.amount < 0
    }

    /// Check if this is an addition transaction
    pub fn is_addition(&self) -> bool {
        self.amount > 0
    }
}

/// Credits transaction type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditsTransactionType {
    /// Credits used for search operation
    Search,
    /// Credits used for scrape operation
    Scrape,
    /// Credits used for extract operation
    Extract,
    /// Credits used for crawl operation
    Crawl,
    /// Manual adjustment by admin
    ManualAdjustment,
    /// Credits from subscription
    Subscription,
    /// Credits refunded
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

impl std::str::FromStr for CreditsTransactionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "search" => Ok(CreditsTransactionType::Search),
            "scrape" => Ok(CreditsTransactionType::Scrape),
            "extract" => Ok(CreditsTransactionType::Extract),
            "crawl" => Ok(CreditsTransactionType::Crawl),
            "manual_adjustment" => Ok(CreditsTransactionType::ManualAdjustment),
            "subscription" => Ok(CreditsTransactionType::Subscription),
            "refund" => Ok(CreditsTransactionType::Refund),
            _ => Err(format!("Invalid transaction type: {}", s)),
        }
    }
}

/// Credits domain errors
#[derive(Debug, thiserror::Error)]
pub enum CreditsError {
    /// Insufficient balance for operation
    #[error("Insufficient balance: have {current}, need {requested}")]
    InsufficientBalance { current: i64, requested: i64 },

    /// Invalid amount
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),
}
