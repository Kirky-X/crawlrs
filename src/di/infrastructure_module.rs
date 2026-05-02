// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure module for Shaku dependency injection.
//!
//! This module provides Shaku components for infrastructure layer dependencies
//! including database connection pool, Redis client, and repository implementations.
//!
//! # Module Structure
//!
//! The infrastructure module is organized into the following sub-modules:
//!
//! - [`database_module`] - Database components (Settings, HttpClient, DatabasePool, TransactionManager)
//! - [`cache_module`] - Cache components (RedisClient, OxCache)
//! - [`repository_module`] - Repository components with instance caching
//! - [`infrastructure_service_module`] - Infrastructure services (WebhookSender)
//!
//! # Performance Optimization
//!
//! All repository components use `OnceLock` for instance caching, avoiding repeated
//! instantiation on every method call. This provides significant performance improvements
//! for frequently accessed repositories.

// Re-export all components from sub-modules for backward compatibility
pub use super::cache_module::{
    OxCacheComponent, OxCacheTrait, RedisClientComponent, RedisClientTrait,
};
pub use super::database_module::{
    DatabasePoolComponent, DatabasePoolTrait, HttpClientComponent, HttpClientTrait,
    SettingsComponent, SettingsTrait, TransactionManagerComponent, TransactionManagerTrait,
};
pub use super::infrastructure_service_module::{WebhookSenderComponent, WebhookSenderTrait};
pub use super::repository_module::{
    AuditLogRepositoryComponent, AuthScopeRepositoryComponent, CrawlRepositoryComponent,
    CreditsRepositoryComponent, GeoRestrictionRepositoryComponent, ScrapeResultRepositoryComponent,
    StorageRepositoryComponent, TaskQueueComponent, TaskRepositoryComponent,
    TasksBacklogRepositoryComponent, WebhookEventRepositoryComponent, WebhookRepositoryComponent,
};

// Infrastructure module components - for Shaku DI
