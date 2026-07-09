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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ========== Credits::new tests ==========

    #[test]
    fn test_credits_new_sets_balance_and_timestamps() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();

        let before = Utc::now();
        let credits = Credits::new(id, team_id, 1000);
        let after = Utc::now();

        assert_eq!(credits.id, id);
        assert_eq!(credits.team_id, team_id);
        assert_eq!(credits.balance(), 1000, "balance should match initial_balance");
        assert!(
            credits.created_at >= before && credits.created_at <= after,
            "created_at should be now"
        );
        assert_eq!(credits.created_at, credits.updated_at);
    }

    #[test]
    fn test_credits_new_zero_balance() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 0);
        assert_eq!(credits.balance(), 0);
    }

    #[test]
    fn test_credits_new_negative_balance() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), -50);
        assert_eq!(credits.balance(), -50, "negative initial balance allowed");
    }

    // ========== Credits::with_timestamps tests ==========

    #[test]
    fn test_credits_with_timestamps_preserves_values() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let created = Utc::now();
        let updated = created + chrono::Duration::seconds(60);

        let credits = Credits::with_timestamps(id, team_id, 500, created, updated);

        assert_eq!(credits.id, id);
        assert_eq!(credits.team_id, team_id);
        assert_eq!(credits.balance(), 500);
        assert_eq!(credits.created_at, created);
        assert_eq!(credits.updated_at, updated);
    }

    // ========== has_sufficient_balance tests ==========

    #[test]
    fn test_has_sufficient_balance_true_when_enough() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 1000);
        assert!(credits.has_sufficient_balance(1000), "exact amount should be sufficient");
        assert!(credits.has_sufficient_balance(500), "less than balance is sufficient");
    }

    #[test]
    fn test_has_sufficient_balance_false_when_insufficient() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 100);
        assert!(!credits.has_sufficient_balance(101), "more than balance is insufficient");
    }

    #[test]
    fn test_has_sufficient_balance_zero_amount_always_true() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 0);
        assert!(credits.has_sufficient_balance(0), "zero amount against zero balance is sufficient");
    }

    // ========== deduct tests ==========

    #[test]
    fn test_deduct_success_reduces_balance() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 1000);
        let before = Utc::now();

        credits.deduct(300).expect("deduct should succeed with sufficient balance");

        assert_eq!(credits.balance(), 700, "balance should be reduced by deducted amount");
        assert!(credits.updated_at >= before, "updated_at should advance");
    }

    #[test]
    fn test_deduct_exact_balance_succeeds() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 500);
        credits.deduct(500).expect("deducting exact balance should succeed");
        assert_eq!(credits.balance(), 0, "balance should be 0 after full deduction");
    }

    #[test]
    fn test_deduct_insufficient_balance_returns_error() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 100);
        let err = credits
            .deduct(300)
            .expect_err("should error when insufficient balance");

        match err {
            CreditsError::InsufficientBalance { current, requested } => {
                assert_eq!(current, 100, "error should report current balance");
                assert_eq!(requested, 300, "error should report requested amount");
            }
            other => panic!("expected InsufficientBalance, got {:?}", other),
        }
        assert_eq!(credits.balance(), 100, "balance should be unchanged on error");
    }

    #[test]
    fn test_deduct_zero_amount_succeeds() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 100);
        credits.deduct(0).expect("deducting 0 should succeed");
        assert_eq!(credits.balance(), 100, "balance unchanged after deducting 0");
    }

    // ========== add tests ==========

    #[test]
    fn test_add_increases_balance() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 500);
        let before = Utc::now();

        credits.add(300);

        assert_eq!(credits.balance(), 800, "balance should increase by added amount");
        assert!(credits.updated_at >= before);
    }

    #[test]
    fn test_add_zero_unchanged_but_updates_timestamp() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 500);
        let old_updated = credits.updated_at;
        // Sleep tiny bit to ensure timestamp differs
        std::thread::sleep(std::time::Duration::from_millis(2));
        credits.add(0);
        assert_eq!(credits.balance(), 500);
        assert!(credits.updated_at > old_updated, "updated_at should advance even for add(0)");
    }

    // ========== set_balance tests ==========

    #[test]
    fn test_set_balance_overrides() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 1000);
        credits.set_balance(42);
        assert_eq!(credits.balance(), 42, "set_balance should override");
    }

    #[test]
    fn test_set_balance_negative_allowed() {
        let mut credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 100);
        credits.set_balance(-10);
        assert_eq!(credits.balance(), -10, "negative balance allowed via set_balance");
    }

    // ========== CreditsTransaction::new tests ==========

    #[test]
    fn test_credits_transaction_new_sets_fields() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let ref_id = Uuid::new_v4();

        let before = Utc::now();
        let txn = CreditsTransaction::new(
            id,
            team_id,
            -50,
            CreditsTransactionType::Scrape,
            "scrape cost".to_string(),
            Some(ref_id),
        );
        let after = Utc::now();

        assert_eq!(txn.id, id);
        assert_eq!(txn.team_id, team_id);
        assert_eq!(txn.amount, -50);
        assert_eq!(txn.transaction_type, CreditsTransactionType::Scrape);
        assert_eq!(txn.description, "scrape cost");
        assert_eq!(txn.reference_id, Some(ref_id));
        assert!(
            txn.created_at >= before && txn.created_at <= after,
            "created_at should be now"
        );
    }

    #[test]
    fn test_credits_transaction_new_without_reference() {
        let txn = CreditsTransaction::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            100,
            CreditsTransactionType::Subscription,
            "monthly".to_string(),
            None,
        );
        assert!(txn.reference_id.is_none());
    }

    // ========== CreditsTransaction::with_timestamp tests ==========

    #[test]
    fn test_credits_transaction_with_timestamp_preserves_ts() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let ts = Utc::now();

        let txn = CreditsTransaction::with_timestamp(
            id,
            team_id,
            25,
            CreditsTransactionType::Refund,
            "refund".to_string(),
            None,
            ts,
        );

        assert_eq!(txn.id, id);
        assert_eq!(txn.team_id, team_id);
        assert_eq!(txn.amount, 25);
        assert_eq!(txn.transaction_type, CreditsTransactionType::Refund);
        assert_eq!(txn.created_at, ts, "with_timestamp should preserve custom ts");
    }

    // ========== is_deduction / is_addition tests ==========

    #[test]
    fn test_is_deduction_true_for_negative_amount() {
        let txn = CreditsTransaction::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            -10,
            CreditsTransactionType::Scrape,
            "deduct".to_string(),
            None,
        );
        assert!(txn.is_deduction(), "negative amount is a deduction");
        assert!(!txn.is_addition(), "negative amount is not an addition");
    }

    #[test]
    fn test_is_addition_true_for_positive_amount() {
        let txn = CreditsTransaction::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            50,
            CreditsTransactionType::Subscription,
            "add".to_string(),
            None,
        );
        assert!(txn.is_addition(), "positive amount is an addition");
        assert!(!txn.is_deduction(), "positive amount is not a deduction");
    }

    #[test]
    fn test_is_deduction_and_is_addition_false_for_zero() {
        let txn = CreditsTransaction::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            0,
            CreditsTransactionType::ManualAdjustment,
            "zero".to_string(),
            None,
        );
        assert!(!txn.is_deduction(), "zero amount is not a deduction");
        assert!(!txn.is_addition(), "zero amount is not an addition");
    }

    // ========== CreditsTransactionType Display / FromStr tests ==========

    #[test]
    fn test_transaction_type_display_all_variants() {
        assert_eq!(CreditsTransactionType::Search.to_string(), "search");
        assert_eq!(CreditsTransactionType::Scrape.to_string(), "scrape");
        assert_eq!(CreditsTransactionType::Extract.to_string(), "extract");
        assert_eq!(CreditsTransactionType::Crawl.to_string(), "crawl");
        assert_eq!(
            CreditsTransactionType::ManualAdjustment.to_string(),
            "manual_adjustment"
        );
        assert_eq!(
            CreditsTransactionType::Subscription.to_string(),
            "subscription"
        );
        assert_eq!(CreditsTransactionType::Refund.to_string(), "refund");
    }

    #[test]
    fn test_transaction_type_from_str_valid() {
        assert_eq!(
            CreditsTransactionType::from_str("search").expect("valid"),
            CreditsTransactionType::Search
        );
        assert_eq!(
            CreditsTransactionType::from_str("scrape").expect("valid"),
            CreditsTransactionType::Scrape
        );
        assert_eq!(
            CreditsTransactionType::from_str("extract").expect("valid"),
            CreditsTransactionType::Extract
        );
        assert_eq!(
            CreditsTransactionType::from_str("crawl").expect("valid"),
            CreditsTransactionType::Crawl
        );
        assert_eq!(
            CreditsTransactionType::from_str("manual_adjustment").expect("valid"),
            CreditsTransactionType::ManualAdjustment
        );
        assert_eq!(
            CreditsTransactionType::from_str("subscription").expect("valid"),
            CreditsTransactionType::Subscription
        );
        assert_eq!(
            CreditsTransactionType::from_str("refund").expect("valid"),
            CreditsTransactionType::Refund
        );
    }

    #[test]
    fn test_transaction_type_from_str_invalid_returns_error() {
        let err = CreditsTransactionType::from_str("unknown_type")
            .expect_err("invalid type should error");
        assert!(
            err.contains("Invalid transaction type"),
            "error should describe invalid type: {}",
            err
        );
        assert!(err.contains("unknown_type"), "error should include the bad value");
    }

    #[test]
    fn test_transaction_type_serde_roundtrip() {
        for ty in [
            CreditsTransactionType::Search,
            CreditsTransactionType::Scrape,
            CreditsTransactionType::Extract,
            CreditsTransactionType::Crawl,
            CreditsTransactionType::ManualAdjustment,
            CreditsTransactionType::Subscription,
            CreditsTransactionType::Refund,
        ] {
            let json = serde_json::to_string(&ty).expect("serialize");
            let back: CreditsTransactionType =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(ty, back, "roundtrip should preserve: {}", json);
        }
    }

    // ========== CreditsError tests ==========

    #[test]
    fn test_credits_error_insufficient_balance_display() {
        let err = CreditsError::InsufficientBalance {
            current: 50,
            requested: 100,
        };
        let msg = err.to_string();
        assert!(msg.contains("Insufficient balance"), "msg: {}", msg);
        assert!(msg.contains("50"), "should show current: {}", msg);
        assert!(msg.contains("100"), "should show requested: {}", msg);
    }

    #[test]
    fn test_credits_error_invalid_amount_display() {
        let err = CreditsError::InvalidAmount("negative".to_string());
        assert!(err.to_string().contains("Invalid amount"));
        assert!(err.to_string().contains("negative"));
    }

    #[test]
    fn test_credits_error_database_error_display() {
        let err = CreditsError::DatabaseError("conn lost".to_string());
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("conn lost"));
    }

    // ========== Credits / CreditsTransaction serde roundtrip ==========

    #[test]
    fn test_credits_serde_roundtrip() {
        let credits = Credits::new(Uuid::new_v4(), Uuid::new_v4(), 750);
        let json = serde_json::to_string(&credits).expect("serialize");
        let back: Credits = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(credits, back, "serde roundtrip should preserve credits");
    }

    #[test]
    fn test_credits_transaction_serde_roundtrip() {
        let txn = CreditsTransaction::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            -25,
            CreditsTransactionType::Crawl,
            "crawl cost".to_string(),
            Some(Uuid::new_v4()),
        );
        let json = serde_json::to_string(&txn).expect("serialize");
        let back: CreditsTransaction = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(txn, back, "serde roundtrip should preserve transaction");
    }
}
