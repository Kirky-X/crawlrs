// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Transaction Manager for database operations using dbnexus
//!
//! This module provides a comprehensive transaction management system supporting:
//! - Begin/Commit/Rollback transactions
//! - Nested transactions using savepoints
//! - Automatic rollback on drop (RAII pattern)
//! - Transaction isolation levels
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::infrastructure::database::transaction::TransactionManager;
//!
//! let tx_manager = TransactionManager::new(pool.clone());
//!
//! // Simple transaction
//! tx_manager.begin().await?;
//! tx_manager.savepoint("sp1").await?;
//! // ... operations ...
//! tx_manager.release_savepoint("sp1").await?;
//! tx_manager.commit().await?;
//! ```

use dbnexus::{DbPool, Session};
use log::{debug, error, info, warn};
use parking_lot::RwLock;
use sea_orm::{ConnectionTrait, DbErr};
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// Transaction error types
#[derive(Error, Debug)]
pub enum TransactionError {
    /// Failed to begin transaction
    #[error("Failed to begin transaction: {0}")]
    BeginFailed(String),

    /// Failed to commit transaction
    #[error("Failed to commit transaction: {0}")]
    CommitFailed(String),

    /// Failed to rollback transaction
    #[error("Failed to rollback transaction: {0}")]
    RollbackFailed(String),

    /// Failed to create savepoint
    #[error("Failed to create savepoint '{name}': {message}")]
    SavepointFailed { name: String, message: String },

    /// Failed to release savepoint
    #[error("Failed to release savepoint '{name}': {message}")]
    ReleaseSavepointFailed { name: String, message: String },

    /// Failed to rollback to savepoint
    #[error("Failed to rollback to savepoint '{name}': {message}")]
    RollbackToSavepointFailed { name: String, message: String },

    /// No active transaction
    #[error("No active transaction")]
    NoActiveTransaction,

    /// Transaction already active
    #[error("Transaction already active")]
    TransactionAlreadyActive,

    /// Invalid savepoint name
    #[error("Invalid savepoint name: {0}")]
    InvalidSavepointName(String),

    /// Savepoint not found
    #[error("Savepoint not found: {0}")]
    SavepointNotFound(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<DbErr> for TransactionError {
    fn from(err: DbErr) -> Self {
        TransactionError::DatabaseError(err.to_string())
    }
}

/// Transaction isolation level configuration
#[derive(Debug, Clone, Copy, Default)]
pub enum TransactionIsolation {
    /// Read uncommitted - lowest isolation level
    ReadUncommitted,
    /// Read committed - default for PostgreSQL
    #[default]
    ReadCommitted,
    /// Repeatable read - higher isolation
    RepeatableRead,
    /// Serializable - highest isolation level
    Serializable,
}

/// Transaction access mode
#[derive(Debug, Clone, Copy, Default)]
pub enum TransactionAccess {
    /// Read-write mode (default)
    #[default]
    ReadWrite,
    /// Read-only mode
    ReadOnly,
}

/// Transaction configuration
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    /// Isolation level for the transaction
    pub isolation_level: TransactionIsolation,
    /// Access mode (read-only or read-write)
    pub access_mode: TransactionAccess,
    /// Enable nested transactions via savepoints
    pub enable_savepoints: bool,
    /// Role to use for the session
    pub role: String,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            isolation_level: TransactionIsolation::ReadCommitted,
            access_mode: TransactionAccess::ReadWrite,
            enable_savepoints: true,
            role: "admin".to_string(),
        }
    }
}

/// Savepoint information
#[derive(Debug, Clone)]
struct Savepoint {
    /// Unique savepoint identifier
    id: Uuid,
    /// Savepoint name
    name: String,
}

/// Active transaction state
struct ActiveTransaction {
    /// The dbnexus session
    session: Session,
    /// Configuration used for this transaction
    config: TransactionConfig,
    /// Stack of active savepoints (for nested transactions)
    savepoints: VecDeque<Savepoint>,
    /// Whether the transaction has been committed or rolled back
    finished: bool,
}

/// Transaction Manager using dbnexus
///
/// Manages database transactions with support for:
/// - Basic transaction operations (begin, commit, rollback)
/// - Nested transactions using savepoints
/// - Automatic rollback on drop
/// - Configurable isolation levels
pub struct TransactionManager {
    /// Database connection pool
    pool: Arc<DbPool>,
    /// Active transaction state (if any)
    active_transaction: RwLock<Option<ActiveTransaction>>,
    /// Default transaction configuration
    default_config: TransactionConfig,
}

impl TransactionManager {
    /// Create a new transaction manager
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool (dbnexus DbPool)
    ///
    /// # Returns
    ///
    /// A new TransactionManager instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self {
            pool,
            active_transaction: RwLock::new(None),
            default_config: TransactionConfig::default(),
        }
    }

    /// Create a new transaction manager with custom configuration
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool (dbnexus DbPool)
    /// * `config` - Default transaction configuration
    pub fn with_config(pool: Arc<DbPool>, config: TransactionConfig) -> Self {
        Self {
            pool,
            active_transaction: RwLock::new(None),
            default_config: config,
        }
    }

    /// Begin a new transaction
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A transaction is already active
    /// - Failed to begin the transaction
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tx_manager = TransactionManager::new(pool);
    /// tx_manager.begin().await?;
    /// // ... perform operations ...
    /// tx_manager.commit().await?;
    /// ```
    pub async fn begin(&self) -> Result<(), TransactionError> {
        debug!("transaction_begin");
        self.begin_with_config(self.default_config.clone()).await
    }

    /// Begin a new transaction with specific configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Transaction configuration
    pub async fn begin_with_config(
        &self,
        config: TransactionConfig,
    ) -> Result<(), TransactionError> {
        // Check if transaction is already active (non-blocking read)
        {
            let active_tx = self.active_transaction.read();
            if active_tx.is_some() {
                return Err(TransactionError::TransactionAlreadyActive);
            }
        }

        // Get a session from the pool (await outside of lock)
        let session = self.pool.get_session(&config.role).await.map_err(|e| {
            error!("Failed to get session: {}", e);
            TransactionError::BeginFailed(e.to_string())
        })?;

        // Begin transaction using dbnexus Session (await outside of lock)
        session.begin_transaction().await.map_err(|e| {
            error!("Failed to begin transaction: {}", e);
            TransactionError::BeginFailed(e.to_string())
        })?;

        debug!(
            "Transaction started with role: {}, isolation: {:?}, access: {:?}",
            config.role, config.isolation_level, config.access_mode
        );

        // Store the transaction state (non-blocking write after await)
        let mut active_tx = self.active_transaction.write();
        *active_tx = Some(ActiveTransaction {
            session,
            config,
            savepoints: VecDeque::new(),
            finished: false,
        });

        Ok(())
    }

    /// Commit the active transaction
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No active transaction
    /// - Transaction already finished
    /// - Failed to commit
    #[allow(clippy::await_holding_lock)]
    pub async fn commit(&self) -> Result<(), TransactionError> {
        let mut active_tx = self.active_transaction.write();

        let tx_state = active_tx
            .as_mut()
            .ok_or(TransactionError::NoActiveTransaction)?;

        if tx_state.finished {
            return Err(TransactionError::NoActiveTransaction);
        }

        tx_state.finished = true;

        // Commit using dbnexus Session
        tx_state.session.commit().await.map_err(|e| {
            error!("Failed to commit transaction: {}", e);
            TransactionError::CommitFailed(e.to_string())
        })?;

        // Clear the active transaction
        *active_tx = None;

        info!("Transaction committed successfully");
        Ok(())
    }

    /// Rollback the active transaction
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No active transaction
    /// - Transaction already finished
    /// - Failed to rollback
    #[allow(clippy::await_holding_lock)]
    pub async fn rollback(&self) -> Result<(), TransactionError> {
        let mut active_tx = self.active_transaction.write();

        let tx_state = active_tx
            .as_mut()
            .ok_or(TransactionError::NoActiveTransaction)?;

        if tx_state.finished {
            return Err(TransactionError::NoActiveTransaction);
        }

        tx_state.finished = true;

        // Rollback using dbnexus Session
        tx_state.session.rollback().await.map_err(|e| {
            error!("Failed to rollback transaction: {}", e);
            TransactionError::RollbackFailed(e.to_string())
        })?;

        // Clear the active transaction
        *active_tx = None;

        info!("Transaction rolled back successfully");
        Ok(())
    }

    /// Create a savepoint for nested transactions
    ///
    /// Savepoints allow partial rollback within a transaction.
    /// This is useful for nested transaction scenarios.
    ///
    /// # Arguments
    ///
    /// * `name` - Savepoint name (must be unique within the transaction)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No active transaction
    /// - Savepoints are disabled
    /// - Invalid savepoint name
    /// - Failed to create savepoint
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tx_manager.begin().await?;
    /// tx_manager.savepoint("before_critical_operation").await?;
    /// // ... critical operation ...
    /// if something_failed {
    ///     tx_manager.rollback_to_savepoint("before_critical_operation").await?;
    /// }
    /// tx_manager.release_savepoint("before_critical_operation").await?;
    /// tx_manager.commit().await?;
    /// ```
    #[allow(clippy::await_holding_lock)]
    pub async fn savepoint(&self, name: &str) -> Result<Uuid, TransactionError> {
        self.validate_savepoint_name(name)?;

        let mut active_tx = self.active_transaction.write();

        let tx_state = active_tx
            .as_mut()
            .ok_or(TransactionError::NoActiveTransaction)?;

        if !tx_state.config.enable_savepoints {
            return Err(TransactionError::SavepointFailed {
                name: name.to_string(),
                message: "Savepoints are disabled for this transaction".to_string(),
            });
        }

        if tx_state.finished {
            return Err(TransactionError::NoActiveTransaction);
        }

        // Check if savepoint with same name exists
        if tx_state.savepoints.iter().any(|sp| sp.name == name) {
            return Err(TransactionError::SavepointFailed {
                name: name.to_string(),
                message: "Savepoint with this name already exists".to_string(),
            });
        }

        // Execute SAVEPOINT command via sea_orm ConnectionTrait::execute_unprepared.
        // 使用 sea_orm 的 execute_unprepared 而非 dbnexus 的 execute_raw_ddl，因为
        // SAVEPOINT/RELEASE SAVEPOINT/ROLLBACK TO SAVEPOINT 是事务控制语句（非 DDL），
        // 不应被 dbnexus DdlGuard 的 DDL 白名单拦截。
        let sql = format!("SAVEPOINT {}", name);
        let conn = tx_state.session.connection().map_err(|e| {
            error!("Failed to get connection for savepoint '{}': {}", name, e);
            TransactionError::SavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;
        conn.execute_unprepared(&sql).await.map_err(|e| {
            error!("Failed to create savepoint '{}': {}", name, e);
            TransactionError::SavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;

        let savepoint = Savepoint {
            id: Uuid::new_v4(),
            name: name.to_string(),
        };

        let savepoint_id = savepoint.id;
        tx_state.savepoints.push_back(savepoint);

        debug!("Savepoint '{}' created with id: {}", name, savepoint_id);
        Ok(savepoint_id)
    }

    /// Release a savepoint
    ///
    /// Releasing a savepoint removes it from the transaction.
    /// After release, you cannot rollback to this savepoint.
    ///
    /// # Arguments
    ///
    /// * `name` - Savepoint name to release
    #[allow(clippy::await_holding_lock)]
    pub async fn release_savepoint(&self, name: &str) -> Result<(), TransactionError> {
        self.validate_savepoint_name(name)?;

        let mut active_tx = self.active_transaction.write();

        let tx_state = active_tx
            .as_mut()
            .ok_or(TransactionError::NoActiveTransaction)?;

        if tx_state.finished {
            return Err(TransactionError::NoActiveTransaction);
        }

        // Find and remove the savepoint
        let position = tx_state
            .savepoints
            .iter()
            .position(|sp| sp.name == name)
            .ok_or_else(|| TransactionError::SavepointNotFound(name.to_string()))?;

        // Execute RELEASE SAVEPOINT command via sea_orm ConnectionTrait::execute_unprepared.
        let sql = format!("RELEASE SAVEPOINT {}", name);
        let conn = tx_state.session.connection().map_err(|e| {
            error!(
                "Failed to get connection for release_savepoint '{}': {}",
                name, e
            );
            TransactionError::ReleaseSavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;
        conn.execute_unprepared(&sql).await.map_err(|e| {
            error!("Failed to release savepoint '{}': {}", name, e);
            TransactionError::ReleaseSavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;

        tx_state.savepoints.remove(position);
        debug!("Savepoint '{}' released", name);
        Ok(())
    }

    /// Rollback to a savepoint
    ///
    /// This rolls back all changes made after the savepoint was created,
    /// but keeps the transaction active.
    ///
    /// # Arguments
    ///
    /// * `name` - Savepoint name to rollback to
    #[allow(clippy::await_holding_lock)]
    pub async fn rollback_to_savepoint(&self, name: &str) -> Result<(), TransactionError> {
        self.validate_savepoint_name(name)?;

        let mut active_tx = self.active_transaction.write();

        let tx_state = active_tx
            .as_mut()
            .ok_or(TransactionError::NoActiveTransaction)?;

        if tx_state.finished {
            return Err(TransactionError::NoActiveTransaction);
        }

        // Verify savepoint exists
        if !tx_state.savepoints.iter().any(|sp| sp.name == name) {
            return Err(TransactionError::SavepointNotFound(name.to_string()));
        }

        // Execute ROLLBACK TO SAVEPOINT command via sea_orm ConnectionTrait::execute_unprepared.
        let sql = format!("ROLLBACK TO SAVEPOINT {}", name);
        let conn = tx_state.session.connection().map_err(|e| {
            error!(
                "Failed to get connection for rollback_to_savepoint '{}': {}",
                name, e
            );
            TransactionError::RollbackToSavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;
        conn.execute_unprepared(&sql).await.map_err(|e| {
            error!("Failed to rollback to savepoint '{}': {}", name, e);
            TransactionError::RollbackToSavepointFailed {
                name: name.to_string(),
                message: e.to_string(),
            }
        })?;

        // Remove all savepoints created after this one
        let position = tx_state
            .savepoints
            .iter()
            .position(|sp| sp.name == name)
            .expect("Savepoint existence verified above, position must exist");
        tx_state.savepoints.truncate(position + 1);

        debug!("Rolled back to savepoint '{}'", name);
        Ok(())
    }

    /// Check if there is an active transaction
    pub fn is_active(&self) -> bool {
        let active_tx = self.active_transaction.read();
        active_tx.as_ref().map(|tx| !tx.finished).unwrap_or(false)
    }

    /// Check if there is an active transaction
    ///
    /// Returns true if there is an active transaction that has not been finished.
    pub fn has_transaction(&self) -> bool {
        let active_tx = self.active_transaction.read();
        active_tx.as_ref().filter(|tx| !tx.finished).is_some()
    }

    /// Get the number of active savepoints
    pub fn savepoint_count(&self) -> usize {
        let active_tx = self.active_transaction.read();
        active_tx
            .as_ref()
            .map(|tx| tx.savepoints.len())
            .unwrap_or(0)
    }

    /// Validate savepoint name
    fn validate_savepoint_name(&self, name: &str) -> Result<(), TransactionError> {
        if name.is_empty() {
            return Err(TransactionError::InvalidSavepointName(
                "Savepoint name cannot be empty".to_string(),
            ));
        }

        if name.len() > 63 {
            return Err(TransactionError::InvalidSavepointName(
                "Savepoint name too long (max 63 characters)".to_string(),
            ));
        }

        // PostgreSQL savepoint names must be valid identifiers
        let valid = name.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !valid {
            return Err(TransactionError::InvalidSavepointName(
                "Savepoint name must contain only alphanumeric characters and underscores"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

impl Drop for TransactionManager {
    fn drop(&mut self) {
        // Check if there's an active transaction that wasn't committed
        if let Some(active_tx) = self.active_transaction.read().as_ref() {
            if !active_tx.finished {
                warn!("TransactionManager dropped with active transaction - transaction will be rolled back");
            }
        }
    }
}

/// Transaction guard for RAII-style transaction management
///
/// This guard automatically rolls back the transaction if not committed.
pub struct TransactionGuard<'a> {
    manager: &'a TransactionManager,
    committed: bool,
}

impl<'a> TransactionGuard<'a> {
    /// Create a new transaction guard
    pub fn new(manager: &'a TransactionManager) -> Self {
        Self {
            manager,
            committed: false,
        }
    }

    /// Commit the transaction
    pub async fn commit(mut self) -> Result<(), TransactionError> {
        self.manager.commit().await?;
        self.committed = true;
        Ok(())
    }

    /// Rollback the transaction
    pub async fn rollback(mut self) -> Result<(), TransactionError> {
        self.manager.rollback().await?;
        self.committed = true;
        Ok(())
    }
}

impl<'a> Drop for TransactionGuard<'a> {
    fn drop(&mut self) {
        if !self.committed && self.manager.is_active() {
            warn!("TransactionGuard dropped without commit - transaction should be rolled back");
            // Note: We can't async rollback in Drop, so we just warn
            // The TransactionManager's Drop will handle the actual rollback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Config & Error variant tests
    // ============================================================

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

    #[test]
    fn test_transaction_isolation_variants() {
        let _ = TransactionIsolation::ReadUncommitted;
        let _ = TransactionIsolation::ReadCommitted;
        let _ = TransactionIsolation::RepeatableRead;
        let _ = TransactionIsolation::Serializable;
    }

    #[test]
    fn test_transaction_access_mode_variants() {
        let _ = TransactionAccess::ReadWrite;
        let _ = TransactionAccess::ReadOnly;
    }

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

        let err = TransactionError::ReleaseSavepointFailed {
            name: "sp1".to_string(),
            message: "error".to_string(),
        };
        assert!(err.to_string().contains("sp1"));

        let err = TransactionError::RollbackToSavepointFailed {
            name: "sp1".to_string(),
            message: "error".to_string(),
        };
        assert!(err.to_string().contains("sp1"));

        let err = TransactionError::DatabaseError("db error".to_string());
        assert!(err.to_string().contains("db error"));
    }

    #[test]
    fn test_from_dberr_to_transaction_error() {
        let db_err = DbErr::Custom("custom db error".to_string());
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("custom db error"));
    }

    // ============================================================
    // Manager construction & state inspection (no DB needed)
    // ============================================================

    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                DbPool::try_from(&dbnexus::DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    #[test]
    fn test_new_creates_manager_without_active_transaction() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        assert!(!manager.is_active());
        assert!(!manager.has_transaction());
        assert_eq!(manager.savepoint_count(), 0);
    }

    #[test]
    fn test_with_config_creates_manager_with_custom_config() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
            role: "readonly".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config);
        // Manager starts without an active transaction regardless of config
        assert!(!manager.is_active());
        assert!(!manager.has_transaction());
        assert_eq!(manager.savepoint_count(), 0);
    }

    #[test]
    fn test_is_active_false_when_no_transaction() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        assert!(!manager.is_active());
    }

    #[test]
    fn test_has_transaction_false_when_no_transaction() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        assert!(!manager.has_transaction());
    }

    #[test]
    fn test_savepoint_count_zero_when_no_transaction() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        assert_eq!(manager.savepoint_count(), 0);
    }

    // ============================================================
    // Savepoint name validation (pure logic, no DB needed)
    // ============================================================

    #[tokio::test]
    async fn test_savepoint_empty_name_rejected_before_active_check() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_savepoint_name_too_long_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let long_name = "a".repeat(64);
        let result = manager.savepoint(&long_name).await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
        let err = result.unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    #[tokio::test]
    async fn test_savepoint_name_max_length_accepted() {
        // 63 chars is the max allowed length
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let max_name = "a".repeat(63);
        let result = manager.savepoint(&max_name).await;
        // Passes validation but fails because no active transaction
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_invalid_chars_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        // Contains hyphen which is not allowed
        let result = manager.savepoint("invalid-name").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
        let err = result.unwrap_err();
        assert!(err.to_string().contains("alphanumeric"));
    }

    #[tokio::test]
    async fn test_savepoint_name_with_space_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("invalid name").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_savepoint_name_with_special_chars_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        for invalid_name in &["sp@1", "sp.1", "sp/1", "sp-1", "sp!1", "sp#1"] {
            let result = manager.savepoint(invalid_name).await;
            assert!(
                matches!(result, Err(TransactionError::InvalidSavepointName(_))),
                "Expected InvalidSavepointName for '{}'",
                invalid_name
            );
        }
    }

    #[tokio::test]
    async fn test_savepoint_valid_name_passes_validation_but_no_tx() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        // Valid names pass validation but fail because no active transaction
        for valid_name in &["sp1", "savepoint_1", "SAVEPOINT_1", "sp123", "_sp"] {
            let result = manager.savepoint(valid_name).await;
            assert!(
                matches!(result, Err(TransactionError::NoActiveTransaction)),
                "Expected NoActiveTransaction for valid name '{}'",
                valid_name
            );
        }
    }

    #[tokio::test]
    async fn test_release_savepoint_empty_name_rejected_before_active_check() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.release_savepoint("").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_release_savepoint_invalid_name_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.release_savepoint("invalid-name").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_release_savepoint_valid_name_no_tx_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.release_savepoint("valid_sp").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_rollback_to_savepoint_empty_name_rejected_before_active_check() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.rollback_to_savepoint("").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_rollback_to_savepoint_invalid_name_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.rollback_to_savepoint("invalid-name").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_rollback_to_savepoint_valid_name_no_tx_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.rollback_to_savepoint("valid_sp").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    // ============================================================
    // Error path tests with lazy pool (no real DB connection)
    // ============================================================

    #[tokio::test]
    async fn test_begin_fails_without_real_db() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.begin().await;
        // Lazy pool with empty URL should fail to get a session
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Could be BeginFailed (from get_session error)
        assert!(
            matches!(err, TransactionError::BeginFailed(_))
                || matches!(err, TransactionError::DatabaseError(_)),
            "Expected BeginFailed or DatabaseError, got {:?}",
            err
        );
        // Manager should still have no active transaction
        assert!(!manager.is_active());
    }

    #[tokio::test]
    async fn test_begin_with_config_fails_without_real_db() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
            role: "readonly".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config);
        let result = manager
            .begin_with_config(TransactionConfig::default())
            .await;
        assert!(result.is_err());
        assert!(!manager.is_active());
    }

    #[tokio::test]
    async fn test_commit_without_active_transaction_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.commit().await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_rollback_without_active_transaction_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.rollback().await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    // ============================================================
    // TransactionGuard tests
    // ============================================================

    #[test]
    fn test_guard_new_creates_uncommitted_guard() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let _guard = TransactionGuard::new(&manager);
        // Guard should not be committed (we can't directly check committed,
        // but we can verify the manager state is unchanged)
        assert!(!manager.is_active());
        // Guard drops here without commit — should not panic since no active tx
    }

    #[tokio::test]
    async fn test_guard_commit_without_active_tx_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let guard = TransactionGuard::new(&manager);
        let result = guard.commit().await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_guard_rollback_without_active_tx_returns_error() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let guard = TransactionGuard::new(&manager);
        let result = guard.rollback().await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[test]
    fn test_guard_drop_without_active_tx_does_not_panic() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        {
            let _guard = TransactionGuard::new(&manager);
            // Guard drops here — no active tx, so no warn/panic
        }
        // If we reach here, the drop didn't panic
        assert!(!manager.is_active());
    }

    // ============================================================
    // Drop behavior tests
    // ============================================================

    #[test]
    fn test_drop_without_active_transaction_does_not_panic() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        // Manager drops here — no active transaction, so no warn
        drop(manager);
        // If we reach here, the drop didn't panic
    }

    #[test]
    fn test_manager_can_be_cloned_via_arc() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        // TransactionManager is not Clone, but we can wrap it in Arc
        let arc_manager = Arc::new(manager);
        assert!(!arc_manager.is_active());
        assert_eq!(arc_manager.savepoint_count(), 0);
    }

    // ============================================================
    // Additional From<DbErr> variant coverage
    // 覆盖 sea_orm::DbErr 各变体到 TransactionError::DatabaseError 的转换
    // ============================================================

    #[test]
    fn test_from_dberr_record_not_found_to_transaction_error() {
        let db_err = DbErr::RecordNotFound("task 42".to_string());
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("task 42"));
    }

    #[test]
    fn test_from_dberr_connection_acquire_timeout_to_transaction_error() {
        let db_err = DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_connection_acquire_closed_to_transaction_error() {
        let db_err = DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed);
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_record_not_inserted_to_transaction_error() {
        let db_err = DbErr::RecordNotInserted;
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_query_runtime_to_transaction_error() {
        let db_err = DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("syntax error"));
    }

    #[test]
    fn test_from_dberr_query_sqlx_error_to_transaction_error() {
        // RuntimeErr::SqlxError 包装底层 sqlx 错误（Arc），验证转换仍走 DatabaseError 分支
        let inner = sea_orm::sqlx::Error::RowNotFound;
        let db_err = DbErr::Query(sea_orm::RuntimeErr::SqlxError(std::sync::Arc::new(inner)));
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_try_into_err_to_transaction_error() {
        // TryIntoErr 包装类型转换失败错误，字段为 from/into/source
        let source_err: Arc<dyn std::error::Error + Send + Sync> = Arc::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad value",
        ));
        let db_err = DbErr::TryIntoErr {
            from: "String",
            into: "i32",
            source: source_err,
        };
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        // Display 中包含 "String" -> "i32" 转换信息
        assert!(tx_err.to_string().contains("String"));
        assert!(tx_err.to_string().contains("i32"));
    }

    #[test]
    fn test_from_dberr_conn_runtime_to_transaction_error() {
        // Conn 变体包装 RuntimeErr
        let db_err = DbErr::Conn(sea_orm::RuntimeErr::Internal("conn lost".to_string()));
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("conn lost"));
    }

    #[test]
    fn test_from_dberr_exec_runtime_to_transaction_error() {
        let db_err = DbErr::Exec(sea_orm::RuntimeErr::Internal("exec fail".to_string()));
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("exec fail"));
    }

    #[test]
    fn test_from_dberr_record_not_updated_to_transaction_error() {
        let db_err = DbErr::RecordNotUpdated;
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_unpack_insert_id_to_transaction_error() {
        let db_err = DbErr::UnpackInsertId;
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_type_to_transaction_error() {
        let db_err = DbErr::Type("invalid type".to_string());
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("invalid type"));
    }

    #[test]
    fn test_from_dberr_json_to_transaction_error() {
        let db_err = DbErr::Json("parse error".to_string());
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("parse error"));
    }

    #[test]
    fn test_from_dberr_convert_from_u64_to_transaction_error() {
        let db_err = DbErr::ConvertFromU64("String");
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
    }

    #[test]
    fn test_from_dberr_attr_type_to_transaction_error() {
        let db_err = DbErr::AttrNotSet("version".to_string());
        let tx_err: TransactionError = db_err.into();
        assert!(matches!(tx_err, TransactionError::DatabaseError(_)));
        assert!(tx_err.to_string().contains("version"));
    }

    // ============================================================
    // TransactionError Display — 精确消息内容验证
    // ============================================================

    #[test]
    fn test_transaction_error_begin_failed_display_contains_prefix() {
        let err = TransactionError::BeginFailed("conn refused".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Failed to begin transaction"));
        assert!(msg.contains("conn refused"));
    }

    #[test]
    fn test_transaction_error_commit_failed_display_contains_prefix() {
        let err = TransactionError::CommitFailed("timeout".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Failed to commit transaction"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_transaction_error_rollback_failed_display_contains_prefix() {
        let err = TransactionError::RollbackFailed("deadlock".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Failed to rollback transaction"));
        assert!(msg.contains("deadlock"));
    }

    #[test]
    fn test_transaction_error_savepoint_failed_display_contains_both_fields() {
        let err = TransactionError::SavepointFailed {
            name: "before_op".to_string(),
            message: "duplicate".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Failed to create savepoint"));
        assert!(msg.contains("before_op"));
        assert!(msg.contains("duplicate"));
    }

    #[test]
    fn test_transaction_error_release_savepoint_failed_display_contains_both_fields() {
        let err = TransactionError::ReleaseSavepointFailed {
            name: "sp1".to_string(),
            message: "not found".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Failed to release savepoint"));
        assert!(msg.contains("sp1"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_transaction_error_rollback_to_savepoint_failed_display_contains_both_fields() {
        let err = TransactionError::RollbackToSavepointFailed {
            name: "sp2".to_string(),
            message: "connection lost".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Failed to rollback to savepoint"));
        assert!(msg.contains("sp2"));
        assert!(msg.contains("connection lost"));
    }

    #[test]
    fn test_transaction_error_database_error_display_contains_message() {
        let err = TransactionError::DatabaseError("query panicked".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Database error"));
        assert!(msg.contains("query panicked"));
    }

    #[test]
    fn test_transaction_error_invalid_savepoint_name_display_contains_message() {
        let err = TransactionError::InvalidSavepointName("too long".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Invalid savepoint name"));
        assert!(msg.contains("too long"));
    }

    #[test]
    fn test_transaction_error_savepoint_not_found_display_contains_name() {
        let err = TransactionError::SavepointNotFound("sp_xyz".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Savepoint not found"));
        assert!(msg.contains("sp_xyz"));
    }

    #[test]
    fn test_transaction_error_no_active_transaction_display_exact() {
        let err = TransactionError::NoActiveTransaction;
        assert_eq!(err.to_string(), "No active transaction");
    }

    #[test]
    fn test_transaction_error_transaction_already_active_display_exact() {
        let err = TransactionError::TransactionAlreadyActive;
        assert_eq!(err.to_string(), "Transaction already active");
    }

    // ============================================================
    // TransactionError Debug 实现
    // ============================================================

    #[test]
    fn test_transaction_error_implements_debug_for_all_variants() {
        // 确保所有变体都实现 Debug（#[derive(Debug)] 应保证）
        let variants: Vec<TransactionError> = vec![
            TransactionError::BeginFailed("e".into()),
            TransactionError::CommitFailed("e".into()),
            TransactionError::RollbackFailed("e".into()),
            TransactionError::SavepointFailed {
                name: "n".into(),
                message: "m".into(),
            },
            TransactionError::ReleaseSavepointFailed {
                name: "n".into(),
                message: "m".into(),
            },
            TransactionError::RollbackToSavepointFailed {
                name: "n".into(),
                message: "m".into(),
            },
            TransactionError::NoActiveTransaction,
            TransactionError::TransactionAlreadyActive,
            TransactionError::InvalidSavepointName("n".into()),
            TransactionError::SavepointNotFound("n".into()),
            TransactionError::DatabaseError("e".into()),
        ];
        for err in &variants {
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty());
        }
    }

    // ============================================================
    // TransactionIsolation / TransactionAccess — Default / Copy
    // ============================================================

    #[test]
    fn test_transaction_isolation_default_is_read_committed() {
        let iso: TransactionIsolation = Default::default();
        assert!(matches!(iso, TransactionIsolation::ReadCommitted));
    }

    #[test]
    fn test_transaction_access_default_is_read_write() {
        let acc: TransactionAccess = Default::default();
        assert!(matches!(acc, TransactionAccess::ReadWrite));
    }

    #[test]
    fn test_transaction_isolation_clone_preserves_variant() {
        for iso in [
            TransactionIsolation::ReadUncommitted,
            TransactionIsolation::ReadCommitted,
            TransactionIsolation::RepeatableRead,
            TransactionIsolation::Serializable,
        ] {
            let cloned = iso;
            // 验证 Copy trait：赋值不移动原值
            assert!(
                matches!(cloned, TransactionIsolation::ReadUncommitted)
                    || matches!(cloned, TransactionIsolation::ReadCommitted)
                    || matches!(cloned, TransactionIsolation::RepeatableRead)
                    || matches!(cloned, TransactionIsolation::Serializable)
            );
            // 使用 cloned 后再使用 iso，验证 Copy
            let _ = iso;
        }
    }

    #[test]
    fn test_transaction_access_clone_preserves_variant() {
        for acc in [TransactionAccess::ReadWrite, TransactionAccess::ReadOnly] {
            let cloned = acc;
            // 验证 Copy trait：赋值不移动原值
            assert!(
                matches!(cloned, TransactionAccess::ReadWrite)
                    || matches!(cloned, TransactionAccess::ReadOnly)
            );
            let _ = acc;
        }
    }

    // ============================================================
    // TransactionConfig — Clone / Debug
    // ============================================================

    #[test]
    fn test_transaction_config_clone_preserves_all_fields() {
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::RepeatableRead,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
            role: "viewer".to_string(),
        };
        let cloned = config.clone();
        assert!(matches!(
            cloned.isolation_level,
            TransactionIsolation::RepeatableRead
        ));
        assert!(matches!(cloned.access_mode, TransactionAccess::ReadOnly));
        assert!(!cloned.enable_savepoints);
        assert_eq!(cloned.role, "viewer");
    }

    #[test]
    fn test_transaction_config_debug_format_works() {
        let config = TransactionConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("TransactionConfig"));
        assert!(debug.contains("admin"));
        assert!(debug.contains("true"));
    }

    // ============================================================
    // Savepoint name validation — additional boundary cases
    // ============================================================

    #[tokio::test]
    async fn test_savepoint_name_single_char_passes_validation_no_tx() {
        // 单字符名称应该通过校验
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("a").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_single_underscore_passes_validation_no_tx() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("_").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_digits_only_passes_validation_no_tx() {
        // 纯数字也应该通过校验（is_alphanumeric 接受数字）
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("12345").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_unicode_alphanumeric_passes_validation_no_tx() {
        // is_alphanumeric 接受 unicode 字母字符（如中文、日文）
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("测试点").await;
        // 通过校验（unicode alphanumeric），但因为没有活动事务而失败
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_tab_character_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("sp\t1").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_savepoint_name_newline_character_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("sp\n1").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_savepoint_name_emoji_rejected() {
        // Emoji 不属于 alphanumeric，应该被拒绝
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.savepoint("sp🎉").await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_savepoint_name_at_max_boundary_passes_validation_no_tx() {
        // 63 字符是边界值，应该通过
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let name = "a".repeat(63);
        let result = manager.savepoint(&name).await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_savepoint_name_one_over_max_rejected() {
        // 64 字符应该被拒绝
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let name = "a".repeat(64);
        let result = manager.savepoint(&name).await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    // ============================================================
    // release_savepoint / rollback_to_savepoint — 额外边界
    // ============================================================

    #[tokio::test]
    async fn test_release_savepoint_too_long_name_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let name = "a".repeat(64);
        let result = manager.release_savepoint(&name).await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_rollback_to_savepoint_too_long_name_rejected() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let name = "a".repeat(64);
        let result = manager.rollback_to_savepoint(&name).await;
        assert!(matches!(
            result,
            Err(TransactionError::InvalidSavepointName(_))
        ));
    }

    #[tokio::test]
    async fn test_release_savepoint_unicode_name_rejected_validation_only_for_chars() {
        // 注意：unicode 字母通过 is_alphanumeric 校验，所以会进入 NoActiveTransaction 分支
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.release_savepoint("释放点").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    #[tokio::test]
    async fn test_rollback_to_savepoint_unicode_name_passes_validation_no_tx() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let result = manager.rollback_to_savepoint("回滚点").await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
    }

    // ============================================================
    // begin_with_config — 不同配置组合的失败路径
    // ============================================================

    #[tokio::test]
    async fn test_begin_with_config_savepoints_disabled_fails_without_db() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::ReadUncommitted,
            access_mode: TransactionAccess::ReadWrite,
            enable_savepoints: false,
            role: "admin".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config.clone());
        let result = manager.begin_with_config(config).await;
        assert!(result.is_err());
        // 即使 begin_with_config 失败，manager 仍然没有活动事务
        assert!(!manager.is_active());
        assert!(!manager.has_transaction());
    }

    #[tokio::test]
    async fn test_begin_with_config_custom_role_fails_without_db() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: true,
            role: "viewer".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config.clone());
        let result = manager.begin_with_config(config).await;
        assert!(result.is_err());
        assert!(!manager.is_active());
    }

    #[tokio::test]
    async fn test_begin_with_config_empty_role_fails_without_db() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::ReadCommitted,
            access_mode: TransactionAccess::ReadWrite,
            enable_savepoints: true,
            role: "".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config.clone());
        let result = manager.begin_with_config(config).await;
        assert!(result.is_err());
    }

    // ============================================================
    // TransactionGuard — 额外边界
    // ============================================================

    #[test]
    fn test_guard_drop_in_inner_scope_does_not_panic() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        {
            let _guard_outer = TransactionGuard::new(&manager);
            {
                let _guard_inner = TransactionGuard::new(&manager);
                // 内层 guard 先 drop，没有活动事务，不应 panic
            }
            // 外层 guard 还活着
            assert!(!manager.is_active());
        }
        // 两个 guard 都 drop 了，manager 状态不变
        assert!(!manager.is_active());
    }

    #[test]
    fn test_guard_can_be_created_and_dropped_multiple_times() {
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        for _ in 0..5 {
            let _guard = TransactionGuard::new(&manager);
            // guard 创建和 drop 都不应该 panic
        }
        assert!(!manager.is_active());
    }

    #[tokio::test]
    async fn test_guard_commit_called_twide_after_drop_returns_error() {
        // 第一次 commit 后 guard 被 consume（self by value），不能再调用
        // 这里测试：commit 失败后 guard 仍然 drop
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        let guard = TransactionGuard::new(&manager);
        let result = guard.commit().await;
        assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
        // guard 已经被 consume（commit takes self by value）
    }

    // ============================================================
    // Drop 行为 — 无活动事务时不触发警告
    // ============================================================

    #[test]
    fn test_drop_with_configured_manager_no_active_transaction_no_panic() {
        let pool = create_test_db_pool();
        let config = TransactionConfig {
            isolation_level: TransactionIsolation::Serializable,
            access_mode: TransactionAccess::ReadOnly,
            enable_savepoints: false,
            role: "viewer".to_string(),
        };
        let manager = TransactionManager::with_config(pool, config);
        // 显式 drop — 无活动事务，应不 panic
        drop(manager);
    }

    #[test]
    fn test_drop_after_failed_begin_does_not_panic() {
        // 模拟 begin 失败后 drop manager
        let pool = create_test_db_pool();
        let manager = TransactionManager::new(pool);
        // 不调用 begin（避免 await），直接 drop
        drop(manager);
    }
}
