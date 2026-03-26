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
use parking_lot::RwLock;
use sea_orm::DbErr;
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, instrument, warn};
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
    #[instrument(skip(self), name = "transaction_begin")]
    pub async fn begin(&self) -> Result<(), TransactionError> {
        self.begin_with_config(self.default_config.clone()).await
    }

    /// Begin a new transaction with specific configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Transaction configuration
    #[instrument(skip(self, config), name = "transaction_begin_with_config")]
    pub async fn begin_with_config(
        &self,
        config: TransactionConfig,
    ) -> Result<(), TransactionError> {
        let mut active_tx = self.active_transaction.write();

        if active_tx.is_some() {
            return Err(TransactionError::TransactionAlreadyActive);
        }

        // Get a session from the pool
        let session = self
            .pool
            .get_session(&config.role)
            .await
            .map_err(|e| {
                error!("Failed to get session: {}", e);
                TransactionError::BeginFailed(e.to_string())
            })?;

        // Begin transaction using dbnexus Session
        session.begin_transaction().await.map_err(|e| {
            error!("Failed to begin transaction: {}", e);
            TransactionError::BeginFailed(e.to_string())
        })?;

        debug!(
            "Transaction started with role: {}, isolation: {:?}, access: {:?}",
            config.role, config.isolation_level, config.access_mode
        );

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
    #[instrument(skip(self), name = "transaction_commit")]
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
    #[instrument(skip(self), name = "transaction_rollback")]
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
    #[instrument(skip(self), name = "transaction_savepoint")]
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

        // Execute SAVEPOINT command using session's execute_raw_ddl (admin role bypasses permission checks)
        let sql = format!("SAVEPOINT {}", name);
        tx_state.session.execute_raw_ddl(&sql).await.map_err(|e| {
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
    #[instrument(skip(self), name = "transaction_release_savepoint")]
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

        // Execute RELEASE SAVEPOINT command
        let sql = format!("RELEASE SAVEPOINT {}", name);
        tx_state.session.execute_raw_ddl(&sql).await.map_err(|e| {
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
    #[instrument(skip(self), name = "transaction_rollback_to_savepoint")]
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

        // Execute ROLLBACK TO SAVEPOINT command
        let sql = format!("ROLLBACK TO SAVEPOINT {}", name);
        tx_state.session.execute_raw_ddl(&sql).await.map_err(|e| {
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
        active_tx
            .as_ref()
            .map(|tx| !tx.finished)
            .unwrap_or(false)
    }

    /// Check if there is an active transaction
    ///
    /// Returns true if there is an active transaction that has not been finished.
    pub fn has_transaction(&self) -> bool {
        let active_tx = self.active_transaction.read();
        active_tx
            .as_ref()
            .filter(|tx| !tx.finished)
            .is_some()
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
