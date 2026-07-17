// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Integration tests for `TransactionManager` against a real PostgreSQL
//! instance (via testcontainers / `TEST_DATABASE_URL`).
//!
//! These tests complement the unit tests in
//! `src/infrastructure/database/transaction.rs` by exercising the
//! happy path and error paths that require a live database connection:
//!
//! - `begin()` / `begin_with_config()` actually open a transaction
//! - `commit()` / `rollback()` actually terminate the transaction
//! - `savepoint()` / `release_savepoint()` / `rollback_to_savepoint()`
//!   execute real `SAVEPOINT` SQL
//! - Nested savepoints work end-to-end
//! - `Drop` (RAII) does not panic with an active transaction
//! - Different `TransactionConfig` (isolation / access mode / role) are
//!   accepted by the manager
//! - Error paths that need an active transaction (duplicate savepoint,
//!   `TransactionAlreadyActive`, `SavepointNotFound` after release, …)
//!
//! Style follows `tests/integration/repositories/task_repository_test.rs`:
//! `#[tokio::test]` + `create_test_app_no_worker()`.

use super::helpers::create_test_app_no_worker;
use crawlrs::infrastructure::database::transaction::{
    TransactionAccess, TransactionConfig, TransactionError, TransactionGuard, TransactionIsolation,
    TransactionManager,
};

// ============================================================
// begin() — happy path
// ============================================================

#[tokio::test]
async fn test_begin_succeeds_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());

    manager.begin().await.expect("begin should succeed");

    assert!(manager.is_active());
    assert!(manager.has_transaction());

    // Cleanup — release the session back to the pool
    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_begin_with_config_succeeds_with_real_db() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadCommitted,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin_with_config(TransactionConfig::default())
        .await
        .expect("begin_with_config should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("cleanup rollback");
}

// ============================================================
// commit() — happy path
// ============================================================

#[tokio::test]
async fn test_commit_succeeds_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());

    manager.commit().await.expect("commit should succeed");

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

#[tokio::test]
async fn test_begin_commit_complete_cycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    // Initial state
    assert!(!manager.is_active());
    assert_eq!(manager.savepoint_count(), 0);

    // Begin
    manager.begin().await.expect("begin failed");
    assert!(manager.is_active());
    assert!(manager.has_transaction());

    // Commit
    manager.commit().await.expect("commit failed");
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);
}

// ============================================================
// rollback() — happy path
// ============================================================

#[tokio::test]
async fn test_rollback_succeeds_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("rollback should succeed");

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

#[tokio::test]
async fn test_begin_rollback_complete_cycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    // Begin
    manager.begin().await.expect("begin failed");
    assert!(manager.is_active());

    // Rollback
    manager.rollback().await.expect("rollback failed");
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

// ============================================================
// savepoint() — happy path
// ============================================================

#[tokio::test]
async fn test_savepoint_creation_succeeds() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    assert_eq!(manager.savepoint_count(), 0);

    let sp_id = manager
        .savepoint("sp1")
        .await
        .expect("savepoint should succeed");
    assert!(!sp_id.is_nil(), "savepoint id must not be nil");
    assert_eq!(manager.savepoint_count(), 1);

    manager.commit().await.expect("cleanup commit");
}

#[tokio::test]
async fn test_multiple_savepoints_creation() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");

    let sp1 = manager.savepoint("sp1").await.expect("sp1 should succeed");
    let sp2 = manager.savepoint("sp2").await.expect("sp2 should succeed");
    let sp3 = manager.savepoint("sp3").await.expect("sp3 should succeed");

    // Each savepoint gets a unique UUID
    assert_ne!(sp1, sp2);
    assert_ne!(sp2, sp3);
    assert_ne!(sp1, sp3);
    assert_eq!(manager.savepoint_count(), 3);

    manager.commit().await.expect("cleanup commit");
}

#[tokio::test]
async fn test_savepoint_valid_names_pass() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");

    // Various valid savepoint names
    for name in &["sp1", "savepoint_1", "SAVEPOINT_1", "sp123", "_sp", "a", "_"] {
        let result = manager.savepoint(name).await;
        assert!(
            result.is_ok(),
            "Expected savepoint '{}' to succeed, got: {:?}",
            name,
            result.err()
        );
    }
    assert_eq!(manager.savepoint_count(), 7);

    manager.rollback().await.expect("cleanup rollback");
}

// ============================================================
// release_savepoint() — happy path
// ============================================================

#[tokio::test]
async fn test_release_savepoint_succeeds() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    manager
        .savepoint("sp1")
        .await
        .expect("savepoint should succeed");
    assert_eq!(manager.savepoint_count(), 1);

    manager
        .release_savepoint("sp1")
        .await
        .expect("release should succeed");
    assert_eq!(manager.savepoint_count(), 0);

    manager.commit().await.expect("cleanup commit");
}

#[tokio::test]
async fn test_release_multiple_savepoints_non_lifo() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    manager.savepoint("sp1").await.expect("sp1");
    manager.savepoint("sp2").await.expect("sp2");
    manager.savepoint("sp3").await.expect("sp3");
    assert_eq!(manager.savepoint_count(), 3);

    // Release in non-LIFO order — should still work (PostgreSQL allows it)
    manager
        .release_savepoint("sp2")
        .await
        .expect("release sp2 (non-LIFO)");
    assert_eq!(manager.savepoint_count(), 2);

    manager
        .release_savepoint("sp1")
        .await
        .expect("release sp1 (non-LIFO)");
    assert_eq!(manager.savepoint_count(), 1);

    manager
        .release_savepoint("sp3")
        .await
        .expect("release sp3 (non-LIFO)");
    assert_eq!(manager.savepoint_count(), 0);

    manager.commit().await.expect("cleanup commit");
}

// ============================================================
// rollback_to_savepoint() — happy path
// ============================================================

#[tokio::test]
async fn test_rollback_to_savepoint_succeeds() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    manager
        .savepoint("sp1")
        .await
        .expect("savepoint should succeed");

    manager
        .rollback_to_savepoint("sp1")
        .await
        .expect("rollback_to_savepoint should succeed");

    // Per source: `truncate(position + 1)` — the savepoint itself is retained
    assert_eq!(manager.savepoint_count(), 1);

    manager.commit().await.expect("cleanup commit");
}

#[tokio::test]
async fn test_rollback_to_savepoint_removes_later_savepoints() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    manager.savepoint("sp1").await.expect("sp1");
    manager.savepoint("sp2").await.expect("sp2");
    manager.savepoint("sp3").await.expect("sp3");
    assert_eq!(manager.savepoint_count(), 3);

    // Rollback to sp1 should remove sp2 and sp3, keep sp1
    manager
        .rollback_to_savepoint("sp1")
        .await
        .expect("rollback_to sp1");
    assert_eq!(manager.savepoint_count(), 1);

    // sp2 and sp3 are gone — should fail with SavepointNotFound
    let err = manager.rollback_to_savepoint("sp2").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound for sp2 after rollback_to sp1, got: {:?}",
        err
    );

    let err = manager.release_savepoint("sp3").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound for sp3 after rollback_to sp1, got: {:?}",
        err
    );

    // sp1 is still present — can rollback to it again
    manager
        .rollback_to_savepoint("sp1")
        .await
        .expect("rollback_to sp1 again");

    manager.commit().await.expect("cleanup commit");
}

// ============================================================
// Nested savepoints — full cycle
// ============================================================

#[tokio::test]
async fn test_nested_savepoints_full_cycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    // Level 1
    let sp1 = manager.savepoint("level1").await.expect("level1");
    assert_eq!(manager.savepoint_count(), 1);

    // Level 2
    let sp2 = manager.savepoint("level2").await.expect("level2");
    assert_eq!(manager.savepoint_count(), 2);

    // Level 3
    let sp3 = manager.savepoint("level3").await.expect("level3");
    assert_eq!(manager.savepoint_count(), 3);

    assert_ne!(sp1, sp2);
    assert_ne!(sp2, sp3);

    // Rollback to level 2 — level 3 should be removed
    manager
        .rollback_to_savepoint("level2")
        .await
        .expect("rollback to level2");
    assert_eq!(manager.savepoint_count(), 2);

    // level3 is gone
    let err = manager.release_savepoint("level3").await.unwrap_err();
    assert!(matches!(err, TransactionError::SavepointNotFound(_)));

    // Release level 2
    manager
        .release_savepoint("level2")
        .await
        .expect("release level2");
    assert_eq!(manager.savepoint_count(), 1);

    // Rollback to level 1
    manager
        .rollback_to_savepoint("level1")
        .await
        .expect("rollback to level1");
    assert_eq!(manager.savepoint_count(), 1);

    // Release level 1
    manager
        .release_savepoint("level1")
        .await
        .expect("release level1");
    assert_eq!(manager.savepoint_count(), 0);

    manager.commit().await.expect("commit");
}

// ============================================================
// Drop behavior (RAII pattern)
// ============================================================

#[tokio::test]
async fn test_drop_without_commit_does_not_panic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());

    // Drop without commit/rollback — Drop emits a warn! but must not panic.
    // The underlying Session is dropped, rolling back the transaction.
    drop(manager);

    // If we reach here, drop didn't panic
}

#[tokio::test]
async fn test_drop_after_commit_does_not_panic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.commit().await.expect("commit");
    assert!(!manager.is_active());

    drop(manager); // No active transaction — no warn, no panic
}

#[tokio::test]
async fn test_drop_after_rollback_does_not_panic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.rollback().await.expect("rollback");
    assert!(!manager.is_active());

    drop(manager);
}

#[tokio::test]
async fn test_drop_with_active_savepoints_does_not_panic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("sp1");
    manager.savepoint("sp2").await.expect("sp2");
    assert_eq!(manager.savepoint_count(), 2);

    // Drop with active savepoints — should not panic
    drop(manager);
}

// ============================================================
// Different TransactionConfig — isolation levels
// ============================================================

#[tokio::test]
async fn test_begin_with_read_committed_isolation() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadCommitted,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin with read committed should succeed");
    assert!(manager.is_active());

    manager.commit().await.expect("cleanup");
}

#[tokio::test]
async fn test_begin_with_read_uncommitted_isolation() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadUncommitted,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin with read uncommitted should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_begin_with_repeatable_read_isolation() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::RepeatableRead,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin with repeatable read should succeed");
    assert!(manager.is_active());

    manager.commit().await.expect("cleanup");
}

#[tokio::test]
async fn test_begin_with_serializable_isolation() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::Serializable,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin with serializable should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("cleanup");
}

// ============================================================
// Different TransactionConfig — access modes
// ============================================================

#[tokio::test]
async fn test_begin_with_read_write_access_mode() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadCommitted,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin read-write should succeed");
    assert!(manager.is_active());

    manager.commit().await.expect("cleanup");
}

#[tokio::test]
async fn test_begin_with_read_only_access_mode() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadCommitted,
        access_mode: TransactionAccess::ReadOnly,
        enable_savepoints: true,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    // PostgreSQL supports READ ONLY transactions; begin should succeed
    manager
        .begin()
        .await
        .expect("begin read-only should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("cleanup");
}

// ============================================================
// Different TransactionConfig — savepoints disabled
// ============================================================

#[tokio::test]
async fn test_begin_with_savepoints_disabled_blocks_savepoint() {
    let app = create_test_app_no_worker().await;
    let config = TransactionConfig {
        isolation_level: TransactionIsolation::ReadCommitted,
        access_mode: TransactionAccess::ReadWrite,
        enable_savepoints: false,
        role: "admin".to_string(),
    };
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);

    manager
        .begin()
        .await
        .expect("begin with savepoints disabled should succeed");
    assert!(manager.is_active());

    // savepoint() must fail because savepoints are disabled in config
    let err = manager.savepoint("sp1").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointFailed { .. }),
        "Expected SavepointFailed when savepoints disabled, got: {:?}",
        err
    );
    let msg = err.to_string();
    assert!(
        msg.contains("disabled"),
        "Error message should mention 'disabled', got: {}",
        msg
    );

    // release_savepoint / rollback_to_savepoint still validate name first,
    // then check active tx — they should fail with NoActiveTransaction-like
    // path only if no savepoints exist. With savepoints disabled, none can
    // exist, so they hit SavepointNotFound.
    let err = manager.release_savepoint("sp1").await.unwrap_err();
    assert!(matches!(err, TransactionError::SavepointNotFound(_)));

    manager.rollback().await.expect("cleanup");
}

// ============================================================
// Error paths — require active transaction state
// ============================================================

#[tokio::test]
async fn test_begin_when_already_active_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager
        .begin()
        .await
        .expect("first begin should succeed");
    assert!(manager.is_active());

    // Second begin must fail — transaction already active
    let err = manager.begin().await.unwrap_err();
    assert!(
        matches!(err, TransactionError::TransactionAlreadyActive),
        "Expected TransactionAlreadyActive, got: {:?}",
        err
    );

    // State unchanged — still active
    assert!(manager.is_active());
    assert!(manager.has_transaction());

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_commit_after_commit_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.commit().await.expect("first commit");
    assert!(!manager.is_active());

    // Second commit must fail — no active transaction
    let err = manager.commit().await.unwrap_err();
    assert!(
        matches!(err, TransactionError::NoActiveTransaction),
        "Expected NoActiveTransaction, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_rollback_after_rollback_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.rollback().await.expect("first rollback");
    assert!(!manager.is_active());

    let err = manager.rollback().await.unwrap_err();
    assert!(
        matches!(err, TransactionError::NoActiveTransaction),
        "Expected NoActiveTransaction, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_commit_after_rollback_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.rollback().await.expect("rollback");

    let err = manager.commit().await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_rollback_after_commit_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.commit().await.expect("commit");

    let err = manager.rollback().await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_savepoint_duplicate_name_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("first sp1");

    // Second savepoint with same name must fail
    let err = manager.savepoint("sp1").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointFailed { .. }),
        "Expected SavepointFailed for duplicate name, got: {:?}",
        err
    );
    let msg = err.to_string();
    assert!(
        msg.contains("already exists"),
        "Error message should mention 'already exists', got: {}",
        msg
    );

    // State unchanged — only one savepoint
    assert_eq!(manager.savepoint_count(), 1);

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_release_savepoint_not_found_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let err = manager
        .release_savepoint("nonexistent")
        .await
        .unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound, got: {:?}",
        err
    );
    assert!(
        err.to_string().contains("nonexistent"),
        "Error should contain savepoint name"
    );

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_rollback_to_savepoint_not_found_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let err = manager
        .rollback_to_savepoint("nonexistent")
        .await
        .unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound, got: {:?}",
        err
    );
    assert!(err.to_string().contains("nonexistent"));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_release_savepoint_after_release_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("create sp1");
    manager
        .release_savepoint("sp1")
        .await
        .expect("release sp1");

    // Releasing again must fail — savepoint no longer exists
    let err = manager.release_savepoint("sp1").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound after release, got: {:?}",
        err
    );

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_rollback_to_savepoint_after_release_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("create sp1");
    manager
        .release_savepoint("sp1")
        .await
        .expect("release sp1");

    // Rolling back to a released savepoint must fail
    let err = manager
        .rollback_to_savepoint("sp1")
        .await
        .unwrap_err();
    assert!(
        matches!(err, TransactionError::SavepointNotFound(_)),
        "Expected SavepointNotFound after release, got: {:?}",
        err
    );

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_savepoint_after_commit_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.commit().await.expect("commit");

    // No active transaction — savepoint must fail
    let err = manager.savepoint("sp1").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::NoActiveTransaction),
        "Expected NoActiveTransaction, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_savepoint_after_rollback_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.rollback().await.expect("rollback");

    let err = manager.savepoint("sp1").await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_release_savepoint_after_commit_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("create sp1");
    manager.commit().await.expect("commit");

    let err = manager.release_savepoint("sp1").await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_rollback_to_savepoint_after_commit_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    manager.savepoint("sp1").await.expect("create sp1");
    manager.commit().await.expect("commit");

    let err = manager
        .rollback_to_savepoint("sp1")
        .await
        .unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_savepoint_empty_name_with_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    // Validation happens before active-tx check — empty name rejected
    let err = manager.savepoint("").await.unwrap_err();
    assert!(
        matches!(err, TransactionError::InvalidSavepointName(_)),
        "Expected InvalidSavepointName, got: {:?}",
        err
    );
    assert!(err.to_string().contains("empty"));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_savepoint_invalid_name_with_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    // Hyphen is not allowed
    let err = manager.savepoint("invalid-name").await.unwrap_err();
    assert!(matches!(err, TransactionError::InvalidSavepointName(_)));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_savepoint_name_too_long_with_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let long_name = "a".repeat(64);
    let err = manager.savepoint(&long_name).await.unwrap_err();
    assert!(
        matches!(err, TransactionError::InvalidSavepointName(_)),
        "Expected InvalidSavepointName for 64-char name, got: {:?}",
        err
    );
    assert!(err.to_string().contains("too long"));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_release_savepoint_invalid_name_with_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let err = manager
        .release_savepoint("invalid-name")
        .await
        .unwrap_err();
    assert!(matches!(err, TransactionError::InvalidSavepointName(_)));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_rollback_to_savepoint_invalid_name_with_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let err = manager
        .rollback_to_savepoint("invalid-name")
        .await
        .unwrap_err();
    assert!(matches!(err, TransactionError::InvalidSavepointName(_)));

    manager.rollback().await.expect("cleanup");
}

// ============================================================
// State inspection — transitions across the lifecycle
// ============================================================

#[tokio::test]
async fn test_is_active_state_transitions() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert!(!manager.is_active());

    manager.begin().await.expect("begin");
    assert!(manager.is_active());

    manager.commit().await.expect("commit");
    assert!(!manager.is_active());

    manager.begin().await.expect("begin 2");
    assert!(manager.is_active());

    manager.rollback().await.expect("rollback 2");
    assert!(!manager.is_active());
}

#[tokio::test]
async fn test_has_transaction_state_transitions() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert!(!manager.has_transaction());

    manager.begin().await.expect("begin");
    assert!(manager.has_transaction());

    manager.rollback().await.expect("rollback");
    assert!(!manager.has_transaction());
}

#[tokio::test]
async fn test_savepoint_count_transitions() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert_eq!(manager.savepoint_count(), 0);

    manager.begin().await.expect("begin");
    assert_eq!(manager.savepoint_count(), 0);

    manager.savepoint("sp1").await.expect("sp1");
    assert_eq!(manager.savepoint_count(), 1);

    manager.savepoint("sp2").await.expect("sp2");
    assert_eq!(manager.savepoint_count(), 2);

    // Release removes one
    manager
        .release_savepoint("sp1")
        .await
        .expect("release sp1");
    assert_eq!(manager.savepoint_count(), 1);

    // rollback_to_savepoint keeps the target savepoint
    manager
        .rollback_to_savepoint("sp2")
        .await
        .expect("rollback to sp2");
    assert_eq!(manager.savepoint_count(), 1);

    manager.commit().await.expect("commit");
    assert_eq!(manager.savepoint_count(), 0);
}

// ============================================================
// TransactionGuard — with real DB
// ============================================================

#[tokio::test]
async fn test_guard_commit_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    assert!(manager.is_active());

    let guard = TransactionGuard::new(&manager);
    guard.commit().await.expect("guard commit should succeed");

    // After guard commit, transaction is finished
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

#[tokio::test]
async fn test_guard_rollback_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    assert!(manager.is_active());

    let guard = TransactionGuard::new(&manager);
    guard
        .rollback()
        .await
        .expect("guard rollback should succeed");

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

#[tokio::test]
async fn test_guard_commit_without_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    // No active transaction — guard commit must fail
    let guard = TransactionGuard::new(&manager);
    let err = guard.commit().await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

#[tokio::test]
async fn test_guard_rollback_without_active_tx_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let guard = TransactionGuard::new(&manager);
    let err = guard.rollback().await.unwrap_err();
    assert!(matches!(err, TransactionError::NoActiveTransaction));
}

// ============================================================
// Combined: savepoint + rollback_to + commit (real SQL)
// ============================================================

#[tokio::test]
async fn test_savepoint_rollback_to_then_commit_full_cycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");

    let _ = manager.savepoint("before_op").await.expect("savepoint");

    // Simulate "operation" by creating another savepoint and rolling back to the first
    let _ = manager.savepoint("after_op").await.expect("savepoint 2");
    assert_eq!(manager.savepoint_count(), 2);

    manager
        .rollback_to_savepoint("before_op")
        .await
        .expect("rollback to before_op");
    // after_op is removed; before_op is retained
    assert_eq!(manager.savepoint_count(), 1);

    // Can still create a new savepoint with a different name
    let _ = manager.savepoint("retry_op").await.expect("retry savepoint");
    assert_eq!(manager.savepoint_count(), 2);

    manager
        .release_savepoint("before_op")
        .await
        .expect("release before_op");
    assert_eq!(manager.savepoint_count(), 1);

    manager
        .release_savepoint("retry_op")
        .await
        .expect("release retry_op");
    assert_eq!(manager.savepoint_count(), 0);

    manager.commit().await.expect("commit");
    assert!(!manager.is_active());
}

// ============================================================
// TransactionError Display — via real operations (integration sanity)
// ============================================================

#[tokio::test]
async fn test_transaction_already_active_error_display() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    let err = manager.begin().await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("already active"),
        "Expected 'already active' in message, got: {}",
        msg
    );

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_savepoint_not_found_error_display() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin");
    let err = manager
        .release_savepoint("missing_sp")
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Savepoint not found"));
    assert!(msg.contains("missing_sp"));

    manager.rollback().await.expect("cleanup");
}

#[tokio::test]
async fn test_no_active_transaction_error_display() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let err = manager.commit().await.unwrap_err();
    assert_eq!(err.to_string(), "No active transaction");
}
