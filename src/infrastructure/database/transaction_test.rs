// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Transaction tests
//!
//! This module contains tests for the TransactionManager.
//! Note: Integration tests require a running PostgreSQL database.
//! Unit tests are provided for configuration validation.

#[cfg(test)]
mod tests {
    use crate::infrastructure::database::transaction::{
        TransactionAccess, TransactionConfig, TransactionError, TransactionIsolation,
    };

    /// Test transaction config default values
    #[test]
    fn test_transaction_config_default() {
        let config = TransactionConfig::default();
        assert!(matches!(
            config.isolation_level,
            TransactionIsolation::ReadCommitted
        ));
        assert!(matches!(config.access_mode, TransactionAccess::ReadWrite));
        assert!(config.enable_savepoints);
        assert_eq!(config.role, "admin");
    }

    /// Test transaction config with custom values
    #[test]
    fn test_transaction_config_custom() {
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
            role: "readonly".to_string(),
        };
        assert!(matches!(
            config.isolation_level,
            TransactionIsolation::Serializable
        ));
        assert!(matches!(config.access_mode, TransactionAccess::ReadOnly));
        assert!(!config.enable_savepoints);
        assert_eq!(config.role, "readonly");
    }

    /// Test transaction isolation variants
    #[test]
    fn test_transaction_isolation_variants() {
        // Ensure all variants can be created
        let _ = TransactionIsolation::ReadUncommitted;
        let _ = TransactionIsolation::ReadCommitted;
        let _ = TransactionIsolation::RepeatableRead;
        let _ = TransactionIsolation::Serializable;
    }

    /// Test transaction access mode variants
    #[test]
    fn test_transaction_access_mode_variants() {
        // Ensure all variants can be created
        let _ = TransactionAccess::ReadWrite;
        let _ = TransactionAccess::ReadOnly;
    }

    /// Test transaction error variants
    #[test]
    fn test_transaction_error_display() {
        let err = TransactionError::BeginFailed("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = TransactionError::CommitFailed("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = TransactionError::RollbackFailed("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = TransactionError::NoActiveTransaction;
        assert!(err.to_string().contains("No active transaction"));

        let err = TransactionError::TransactionAlreadyActive;
        assert!(err.to_string().contains("already active"));

        let err = TransactionError::InvalidSavepointName("test".to_string());
        assert!(err.to_string().contains("Invalid"));

        let err = TransactionError::SavepointNotFound("test".to_string());
        assert!(err.to_string().contains("not found"));

        let err = TransactionError::SavepointFailed {
            name: "sp1".to_string(),
            message: "error".to_string(),
        };
        assert!(err.to_string().contains("sp1"));
        assert!(err.to_string().contains("error"));

        let err = TransactionError::DatabaseError("db error".to_string());
        assert!(err.to_string().contains("db error"));
    }

    // Integration tests are skipped by default. To run them:
    // 1. Start a PostgreSQL database
    // 2. Set SKIP_DATABASE_TESTS="" or remove the check
    // 3. Update the database URL in the test setup

    /// Test transaction manager with real database (requires PostgreSQL)
    /// Run with: SKIP_DATABASE_TESTS="" cargo test test_real_db_transaction
    #[tokio::test]
    #[ignore] // Requires PostgreSQL - run manually
    async fn test_real_db_transaction() {
        use crate::infrastructure::database::transaction::TransactionManager;
        use std::sync::Arc;

        // This test requires a PostgreSQL database
        // Set DATABASE_URL environment variable or update the URL below
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/crawlrs_test".to_string());

        let settings = crate::config::DatabaseSettings {
            url: database_url,
            max_connections: Some(5),
            min_connections: Some(1),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };

        let pool = crate::infrastructure::database::dbnexus_connection::create_pool(&settings)
            .await
            .expect("Failed to create pool");
        let tx_manager = TransactionManager::new(Arc::new(pool));

        // Begin transaction
        tx_manager.begin().await.expect("Failed to begin transaction");
        assert!(tx_manager.is_active(), "Transaction should be active");

        // Commit transaction
        tx_manager.commit().await.expect("Failed to commit transaction");
        assert!(!tx_manager.is_active(), "Transaction should not be active after commit");
    }

    /// Test nested transaction with savepoints (requires PostgreSQL)
    #[tokio::test]
    #[ignore] // Requires PostgreSQL - run manually
    async fn test_real_db_nested_transaction() {
        use crate::infrastructure::database::transaction::TransactionManager;
        use std::sync::Arc;

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/crawlrs_test".to_string());

        let settings = crate::config::DatabaseSettings {
            url: database_url,
            max_connections: Some(5),
            min_connections: Some(1),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };

        let pool = crate::infrastructure::database::dbnexus_connection::create_pool(&settings)
            .await
            .expect("Failed to create pool");
        let tx_manager = TransactionManager::new(Arc::new(pool));

        // Begin outer transaction
        tx_manager.begin().await.expect("Failed to begin transaction");

        // Create savepoint
        let sp_result = tx_manager.savepoint("sp1").await;
        assert!(sp_result.is_ok(), "Failed to create savepoint");
        assert_eq!(tx_manager.savepoint_count(), 1);

        // Rollback to savepoint
        let rollback_result = tx_manager.rollback_to_savepoint("sp1").await;
        assert!(rollback_result.is_ok(), "Failed to rollback to savepoint");

        // Release savepoint
        let release_result = tx_manager.release_savepoint("sp1").await;
        assert!(release_result.is_ok(), "Failed to release savepoint");

        // Commit
        tx_manager.commit().await.expect("Failed to commit");
    }
}
