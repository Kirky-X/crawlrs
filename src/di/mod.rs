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
//! - [`modules`](modules) — trait-kit module definitions (Settings, Database, Http, Cache, etc.)
//! - [`axum_state`](axum_state::CrawlRsState) — Axum integration; `CrawlRsState::from_kit` is the
//!   canonical entry point after building the `AsyncKit`
//! - [`infrastructure_module`](infrastructure_module) - Legacy infrastructure components
//!   - [`database_module`](database_module) - Database components
//!   - [`cache_module`](cache_module) - Cache components
//!   - [`repository_module`](repository_module) - Repository components with caching
//!   - [`infrastructure_service_module`](infrastructure_service_module) - Infrastructure services
//! - [`search_module`](search_module) - Search components
//! - [`service_module`](service_module) - Service components
//! - [`engines_module`](engines_module) - Engine components

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
pub use axum_state::{CrawlRsState, CrawlRsStateExt};
pub use modules::ModuleBuildError;
