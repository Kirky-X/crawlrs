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

impl HttpClientComponent {
    /// Create a new HttpClientComponent with explicit dependencies
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self { client }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个用于测试的 Settings 实例（所有子配置使用默认值）。
    fn make_test_settings() -> Arc<Settings> {
        use crate::config::settings::CorsSettings;
        use crate::config::{
            BingSearchSettings, CacheSettings, ConcurrencySettings, DatabaseSettings,
            EngineSettings, LLMSettings, LoggingSettings, ProxySettings, RateLimitingSettings,
            RedisSettings, SearchSettings, ServerSettings, TimeoutSettings, TrustedProxySettings,
            WebhookSettings, WorkerSettings,
        };

        Arc::new(Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
        })
    }

    // ========== SettingsComponent ==========

    #[test]
    fn test_settings_component_new_stores_settings() {
        let settings = make_test_settings();
        let component = SettingsComponent::new(settings.clone());
        // get() 应返回与传入相同的 Arc
        let retrieved = component.get();
        assert!(Arc::ptr_eq(&retrieved, &settings));
    }

    #[test]
    fn test_settings_component_get_returns_clone() {
        let settings = make_test_settings();
        let component = SettingsComponent::new(settings.clone());
        let first = component.get();
        let second = component.get();
        // 多次调用 get() 应返回指向同一 Settings 的 Arc
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &settings));
    }

    #[test]
    fn test_settings_component_as_trait_object() {
        let settings = make_test_settings();
        let component = SettingsComponent::new(settings.clone());
        // 通过 trait 对象访问，验证动态分发正常工作
        let trait_obj: &dyn SettingsTrait = &component;
        let retrieved = trait_obj.get();
        assert!(Arc::ptr_eq(&retrieved, &settings));
    }

    // ========== HttpClientComponent ==========

    #[test]
    fn test_http_client_component_new_stores_client() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let retrieved = component.get();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    #[test]
    fn test_http_client_component_get_returns_clone() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let first = component.get();
        let second = component.get();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &client));
    }

    #[test]
    fn test_http_client_component_as_trait_object() {
        let client = Arc::new(reqwest::Client::new());
        let component = HttpClientComponent::new(client.clone());
        let trait_obj: &dyn HttpClientTrait = &component;
        let retrieved = trait_obj.get();
        assert!(Arc::ptr_eq(&retrieved, &client));
    }

    // ========== DatabasePoolComponent / TransactionManagerComponent ==========
    // 注意：DatabasePoolComponent::from(pool) 需要 Arc<DatabasePool>，而 DatabasePool
    // 内部封装 dbnexus::DbPool，必须连接真实数据库才能构造。
    // TransactionManagerComponent::new/with_db_pool 同样依赖 DbPool。
    // 这些构造器无法在无数据库环境的单元测试中测试，故跳过。
}
