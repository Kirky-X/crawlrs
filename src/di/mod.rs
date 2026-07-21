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
//! - `modules` — trait-kit module definitions (Settings, Database, Http, Cache, etc.)
//! - `axum_state` — Axum integration; `CrawlRsState::from_kit` is the
//!   canonical entry point after building the `AsyncKit`

// Core DI modules
pub mod axum_state;
pub mod modules;

// Re-exports for convenience
pub use axum_state::{CrawlRsState, CrawlRsStateExt};
pub use modules::ModuleBuildError;
