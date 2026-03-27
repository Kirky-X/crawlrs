// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Database module for Shaku dependency injection.
//!
//! This module provides Shaku components for database layer dependencies
//! including database connection pool and transaction management.

use std::sync::Arc;

use shaku::{Component, HasComponent, Interface, Module, ModuleBuildContext};

use crate::config::Settings;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;
use crate::infrastructure::database::transaction::TransactionManager;

// =============================================================================
// Settings Component
// =============================================================================

/// Trait for Settings component
pub trait SettingsTrait: Interface + Send + Sync {
    fn get(&self) -> Arc<Settings>;
}

/// Settings component for Shaku DI
pub struct SettingsComponent {
    settings: Arc<Settings>,
}

impl<M: Module> Component<M> for SettingsComponent {
    type Interface = dyn SettingsTrait;
    type Parameters = Arc<Settings>;

    fn build(_: &mut ModuleBuildContext<M>, settings: Self::Parameters) -> Box<Self::Interface> {
        Box::new(Self { settings })
    }
}

impl SettingsComponent {
    /// Create a new SettingsComponent with explicit dependencies
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
}

impl SettingsTrait for SettingsComponent {
    fn get(&self) -> Arc<Settings> {
        self.settings.clone()
    }
}

// =============================================================================
// HTTP Client Component
// =============================================================================

/// Trait for HttpClient component
pub trait HttpClientTrait: Interface + Send + Sync {
    fn get(&self) -> Arc<reqwest::Client>;
}

/// HttpClient component for Shaku DI
pub struct HttpClientComponent {
    client: Arc<reqwest::Client>,
}

impl<M: Module> Component<M> for HttpClientComponent {
    type Interface = dyn HttpClientTrait;
    type Parameters = Arc<reqwest::Client>;

    fn build(_: &mut ModuleBuildContext<M>, client: Self::Parameters) -> Box<Self::Interface> {
        Box::new(Self { client })
    }
}

impl HttpClientTrait for HttpClientComponent {
    fn get(&self) -> Arc<reqwest::Client> {
        self.client.clone()
    }
}

// =============================================================================
// Database Pool Component
// =============================================================================

/// Trait for Database component
pub trait DatabasePoolTrait: Interface + Send + Sync {
    fn get_pool(&self) -> Arc<DatabasePool>;
}

/// Database component for Shaku DI
pub struct DatabasePoolComponent {
    /// The actual database pool
    pool: Arc<DatabasePool>,
}

impl<M: Module + HasComponent<dyn SettingsTrait>> Component<M> for DatabasePoolComponent {
    type Interface = dyn DatabasePoolTrait;
    type Parameters = ();

    fn build(_context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        // Note: This component requires explicit pool creation via bootstrap
        // The pool should be created and passed to the module during initialization
        panic!("DatabasePoolComponent must be initialized with an explicit pool. Use DatabasePoolComponent::from(pool) instead.")
    }
}

impl From<Arc<DatabasePool>> for DatabasePoolComponent {
    fn from(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

impl DatabasePoolTrait for DatabasePoolComponent {
    fn get_pool(&self) -> Arc<DatabasePool> {
        Arc::clone(&self.pool)
    }
}

// =============================================================================
// Transaction Manager Component
// =============================================================================

/// Trait for TransactionManager component
pub trait TransactionManagerTrait: Interface + Send + Sync {
    /// Get the transaction manager
    fn get_manager(&self) -> Arc<TransactionManager>;
}

/// TransactionManager component for Shaku DI
///
/// This component provides transaction management capabilities including:
/// - Begin/Commit/Rollback transactions
/// - Nested transactions using savepoints
/// - Configurable isolation levels
pub struct TransactionManagerComponent {
    manager: Arc<TransactionManager>,
}

impl<M: Module + HasComponent<dyn DatabasePoolTrait>> Component<M> for TransactionManagerComponent {
    type Interface = dyn TransactionManagerTrait;
    type Parameters = ();

    fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        let pool_component: Arc<dyn DatabasePoolTrait> = M::build_component(context);
        let pool = pool_component.get_pool();

        // Create TransactionManager with the dbnexus DbPool
        // DatabasePool wraps the dbnexus DbPool internally
        let db_pool = pool.inner().clone();
        let manager = Arc::new(TransactionManager::new(db_pool));

        Box::new(Self { manager })
    }
}

impl TransactionManagerComponent {
    /// Create a new TransactionManagerComponent with explicit dependencies
    pub fn new(manager: Arc<TransactionManager>) -> Self {
        Self { manager }
    }

    /// Create with dbnexus DbPool
    pub fn with_db_pool(db_pool: Arc<dbnexus::DbPool>) -> Self {
        let manager = Arc::new(TransactionManager::new(db_pool));
        Self { manager }
    }
}

impl TransactionManagerTrait for TransactionManagerComponent {
    fn get_manager(&self) -> Arc<TransactionManager> {
        self.manager.clone()
    }
}

// Database module components - for Shaku DI
