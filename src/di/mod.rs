// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Dependency Injection module using trait-kit library.
//!
//! This module provides a standardized dependency injection framework for managing
//! component dependencies across the crawlrs application.
//!
//! # Module Structure
//!
//! - [`infrastructure_module`](infrastructure_module) - Infrastructure components
//!   - [`database_module`](database_module) - Database components
//!   - [`cache_module`](cache_module) - Cache components
//!   - [`repository_module`](repository_module) - Repository components with caching
//!   - [`infrastructure_service_module`](infrastructure_service_module) - Infrastructure services
//! - [`search_module`](search_module::SearchModule) - Search components
//! - [`service_module`](service_module::ServiceModule) - Service components
//! - [`engines_module`](engines_module) - Engine components
//! - [`axum_state`](axum_state::AppState) - Axum integration
//! - [`state_manager`](state_manager::DependencyStateManager) - Dependency state management
//!
//! # Performance Optimization
//!
//! Repository components use `OnceLock` for instance caching, providing:
//! - Lazy initialization of underlying implementations
//! - Singleton pattern without repeated instantiation
//! - Thread-safe caching with minimal overhead

// Core DI modules
pub mod axum_state;
pub mod engines_module;
pub mod modules;
pub mod search_module;
pub mod service_module;

// Infrastructure sub-modules (organized separately for maintainability)
pub mod cache_module;
pub mod database_module;
pub mod infrastructure_module;
pub mod infrastructure_service_module;
pub mod repository_module;

// Re-exports for convenience
pub use axum_state::{AppState, AppStateExt};
pub use modules::ModuleBuildError;
