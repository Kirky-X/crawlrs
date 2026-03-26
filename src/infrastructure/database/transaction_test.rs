// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Transaction tests
//!
//! This module contains comprehensive tests for the TransactionManager,
//! including basic transaction operations, nested transactions with savepoints,
//! and error handling scenarios.

#[cfg(test)]
mod tests {
    use crate::infrastructure::database::transaction::{
        TransactionAccess, TransactionConfig, TransactionError, TransactionIsolation,
        TransactionManager,
    };
    use sea_orm::{Database, DatabaseConnection};
    use std::sync::Arc;

    /// Helper to create an in-memory SQLite database for testing
    async fn create_test_db() -> DatabaseConnection {
        Database::connect("sqlite::memory:")
            .await
            .expect("Failed to create test database")
    }

    /// Test basic transaction begin and commit
    #[tokio::test]
    async fn test_transaction_begin_commit() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin transaction
        let result = tx_manager.begin().await;
        assert!(result.is_ok(), "Failed to begin transaction");
        assert!(tx_manager.is_active(), "Transaction should be active after begin");

        // Commit transaction
        let result = tx_manager.commit().await;
        assert!(result.is_ok(), "Failed to commit transaction");
        assert!(!tx_manager.is_active(), "Transaction should not be active after commit");
    }

    /// Test basic transaction begin and rollback
    #[tokio::test]
    async fn test_transaction_begin_rollback() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin transaction
        tx_manager.begin().await.expect("Failed to begin transaction");
        assert!(tx_manager.is_active(), "Transaction should be active after begin");

        // Rollback transaction
        let result = tx_manager.rollback().await;
        assert!(result.is_ok(), "Failed to rollback transaction");
        assert!(!tx_manager.is_active(), "Transaction should not be active after rollback");
    }

    /// Test nested transaction with savepoint
    #[tokio::test]
    async fn test_nested_transaction_savepoint() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin outer transaction
        tx_manager.begin().await.expect("Failed to begin outer transaction");

        // Create first savepoint
        let sp1_result = tx_manager.savepoint("sp1").await;
        assert!(sp1_result.is_ok(), "Failed to create savepoint sp1");
        assert_eq!(tx_manager.savepoint_count(), 1, "Should have 1 savepoint");

        // Create second savepoint
        let sp2_result = tx_manager.savepoint("sp2").await;
        assert!(sp2_result.is_ok(), "Failed to create savepoint sp2");
        assert_eq!(tx_manager.savepoint_count(), 2, "Should have 2 savepoints");

        // Rollback to first savepoint
        let rollback_result = tx_manager.rollback_to_savepoint("sp1").await;
        assert!(rollback_result.is_ok(), "Failed to rollback to savepoint sp1");
        assert_eq!(tx_manager.savepoint_count(), 1, "Should have 1 savepoint after rollback");

        // Release first savepoint
        let release_result = tx_manager.release_savepoint("sp1").await;
        assert!(release_result.is_ok(), "Failed to release savepoint sp1");
        assert_eq!(tx_manager.savepoint_count(), 0, "Should have 0 savepoints after release");

        // Commit outer transaction
        let commit_result = tx_manager.commit().await;
        assert!(commit_result.is_ok(), "Failed to commit outer transaction");
    }

    /// Test savepoint error handling
    #[tokio::test]
    async fn test_savepoint_errors() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Try to create savepoint without active transaction
        let result = tx_manager.savepoint("sp1").await;
        assert!(
            matches!(result, Err(TransactionError::NoActiveTransaction)),
            "Should return NoActiveTransaction error"
        );

        // Begin transaction
        tx_manager.begin().await.unwrap();

        // Try to create savepoint with empty name
        let result = tx_manager.savepoint("").await;
        assert!(
            matches!(result, Err(TransactionError::InvalidSavepointName(_))),
            "Should return InvalidSavepointName error for empty name"
        );

        // Try to create savepoint with invalid characters
        let result = tx_manager.savepoint("sp-with-dash").await;
        assert!(
            matches!(result, Err(TransactionError::InvalidSavepointName(_))),
            "Should return InvalidSavepointName error for invalid characters"
        );

        // Create valid savepoint
        tx_manager.savepoint("sp1").await.unwrap();

        // Try to create duplicate savepoint
        let result = tx_manager.savepoint("sp1").await;
        assert!(
            matches!(result, Err(TransactionError::SavepointFailed { .. })),
            "Should return SavepointFailed error for duplicate savepoint"
        );

        // Try to rollback to non-existent savepoint
        let result = tx_manager.rollback_to_savepoint("nonexistent").await;
        assert!(
            matches!(result, Err(TransactionError::SavepointNotFound(_))),
            "Should return SavepointNotFound error"
        );

        // Cleanup
        tx_manager.rollback().await.unwrap();
    }

    /// Test transaction with configuration
    #[tokio::test]
    async fn test_transaction_with_config() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: true,
        };

        // Begin transaction with config
        let result = tx_manager.begin_with_config(config).await;
        assert!(result.is_ok(), "Failed to begin transaction with config");

        // Commit
        let result = tx_manager.commit().await;
        assert!(result.is_ok(), "Failed to commit transaction");
    }

    /// Test execute_in_transaction helper with success
    #[tokio::test]
    async fn test_execute_in_transaction_success() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Execute successful operation
        let result = tx_manager
            .execute_in_transaction(|_tx| async move { Ok::<i32, anyhow::Error>(42) })
            .await;

        assert!(result.is_ok(), "Transaction should succeed");
        assert_eq!(result.unwrap(), 42);
    }

    /// Test execute_in_transaction helper with failure
    #[tokio::test]
    async fn test_execute_in_transaction_failure() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Execute failing operation
        let result = tx_manager
            .execute_in_transaction(|_tx| async move {
                Err::<i32, anyhow::Error>(anyhow::anyhow!("Test error"))
            })
            .await;

        assert!(result.is_err(), "Transaction should fail");
        assert!(!tx_manager.is_active(), "Transaction should be rolled back");
    }

    /// Test execute_in_savepoint helper with success
    #[tokio::test]
    async fn test_execute_in_savepoint_success() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin outer transaction
        tx_manager.begin().await.unwrap();

        // Execute successful nested operation
        let result = tx_manager
            .execute_in_savepoint("nested1", |_tx| async move { Ok::<i32, anyhow::Error>(42) })
            .await;

        assert!(result.is_ok(), "Savepoint operation should succeed");
        assert_eq!(result.unwrap(), 42);

        // Commit outer transaction
        tx_manager.commit().await.unwrap();
    }

    /// Test execute_in_savepoint helper with failure
    #[tokio::test]
    async fn test_execute_in_savepoint_failure() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin outer transaction
        tx_manager.begin().await.unwrap();

        // Execute failing nested operation
        let result = tx_manager
            .execute_in_savepoint("nested2", |_tx| async move {
                Err::<i32, anyhow::Error>(anyhow::anyhow!("Nested error"))
            })
            .await;

        assert!(result.is_err(), "Savepoint operation should fail");

        // Outer transaction should still be active
        assert!(tx_manager.is_active(), "Outer transaction should still be active");

        // Commit outer transaction
        tx_manager.commit().await.unwrap();
    }

    /// Test double begin error
    #[tokio::test]
    async fn test_double_begin_error() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Begin first transaction
        tx_manager.begin().await.unwrap();

        // Try to begin second transaction
        let result = tx_manager.begin().await;
        assert!(
            matches!(result, Err(TransactionError::TransactionAlreadyActive)),
            "Should not allow double begin"
        );

        // Cleanup
        tx_manager.rollback().await.unwrap();
    }

    /// Test commit without active transaction error
    #[tokio::test]
    async fn test_commit_without_active_transaction() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Try to commit without active transaction
        let result = tx_manager.commit().await;
        assert!(
            matches!(result, Err(TransactionError::NoActiveTransaction)),
            "Should not allow commit without active transaction"
        );
    }

    /// Test rollback without active transaction error
    #[tokio::test]
    async fn test_rollback_without_active_transaction() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Try to rollback without active transaction
        let result = tx_manager.rollback().await;
        assert!(
            matches!(result, Err(TransactionError::NoActiveTransaction)),
            "Should not allow rollback without active transaction"
        );
    }

    /// Test has_transaction method
    #[tokio::test]
    async fn test_has_transaction() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let db = create_test_db().await;
        let tx_manager = TransactionManager::new(Arc::new(db));

        // No transaction initially
        assert!(!tx_manager.has_transaction(), "Should have no transaction initially");

        // Begin transaction
        tx_manager.begin().await.unwrap();

        // Should have transaction now
        assert!(tx_manager.has_transaction(), "Should have transaction after begin");

        // Commit
        tx_manager.commit().await.unwrap();

        // No transaction after commit
        assert!(!tx_manager.has_transaction(), "Should have no transaction after commit");
    }

    /// Test isolation level conversion
    #[test]
    fn test_isolation_level_conversion() {
        use sea_orm::IsolationLevel;

        assert!(matches!(
            IsolationLevel::from(TransactionIsolation::ReadUncommitted),
            IsolationLevel::ReadUncommitted
        ));
        assert!(matches!(
            IsolationLevel::from(TransactionIsolation::ReadCommitted),
            IsolationLevel::ReadCommitted
        ));
        assert!(matches!(
            IsolationLevel::from(TransactionIsolation::RepeatableRead),
            IsolationLevel::RepeatableRead
        ));
        assert!(matches!(
            IsolationLevel::from(TransactionIsolation::Serializable),
            IsolationLevel::Serializable
        ));
    }

    /// Test access mode conversion
    #[test]
    fn test_access_mode_conversion() {
        use sea_orm::AccessMode;

        assert!(matches!(
            AccessMode::from(TransactionAccess::ReadWrite),
            AccessMode::ReadWrite
        ));
        assert!(matches!(
            AccessMode::from(TransactionAccess::ReadOnly),
            AccessMode::ReadOnly
        ));
    }

    /// Test transaction config default
    #[test]
    fn test_transaction_config_default() {
        let config = TransactionConfig::default();

        assert!(matches!(
            config.isolation_level,
            TransactionIsolation::ReadCommitted
        ));
        assert!(matches!(config.access_mode, TransactionAccess::ReadWrite));
        assert!(config.enable_savepoints);
    }

    /// Test savepoint name validation
    #[test]
    fn test_savepoint_name_validation() {
        let db = futures::executor::block_on(create_test_db());
        let tx_manager = TransactionManager::new(Arc::new(db));

        // Valid names
        assert!(tx_manager.validate_savepoint_name("sp1").is_ok());
        assert!(tx_manager.validate_savepoint_name("savepoint_1").is_ok());
        assert!(tx_manager.validate_savepoint_name("SAVEPOINT").is_ok());
        assert!(tx_manager.validate_savepoint_name("_underscore").is_ok());

        // Invalid names
        assert!(tx_manager.validate_savepoint_name("").is_err());
        assert!(tx_manager.validate_savepoint_name("sp-1").is_err());
        assert!(tx_manager.validate_savepoint_name("sp 1").is_err());
        assert!(tx_manager.validate_savepoint_name(&"a".repeat(64)).is_err());
    }

    /// Test transaction manager creation
    #[test]
    fn test_transaction_manager_creation() {
        let db = futures::executor::block_on(create_test_db());
        let tx_manager = TransactionManager::new(Arc::new(db));

        assert!(!tx_manager.is_active());
        assert_eq!(tx_manager.savepoint_count(), 0);
    }

    /// Test transaction manager with config
    #[test]
    fn test_transaction_manager_with_config() {
        let db = futures::executor::block_on(create_test_db());
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
        };

        let tx_manager = TransactionManager::with_config(Arc::new(db), config);

        assert!(!tx_manager.is_active());
        assert_eq!(tx_manager.savepoint_count(), 0);
    }
}
