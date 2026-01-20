// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Dependency Injection module using Shaku library.
//!
//! This module provides a standardized dependency injection framework for managing
//! component dependencies across the crawlrs application.
//!
//! # Module Structure
//!
//! - [`app_module`](app_module::AppModule) - Root application module
//! - [`infrastructure_module`](infrastructure_module::InfrastructureModule) - Infrastructure components
//! - [`search_module`](search_module::SearchModule) - Search components
//! - [`service_module`](service_module::ServiceModule) - Service components
//! - [`test_module`](test_module::TestModule) - Test components with mocks
//! - [`axum_state`](axum_state::AppState) - Axum integration
//! - [`state_manager`](state_manager::DependencyStateManager) - Dependency state management
//!
//! # Usage
//!
//! ```rust
//! use shaku::HasComponent;
//! use crate::di::app_module::AppModule;
//!
//! let module = AppModule::builder().build();
//! let component: &dyn SomeInterface = module.resolve_ref();
//! ```

pub mod app_module;
pub mod axum_state;
pub mod infrastructure_module;
pub mod search_module;
pub mod service_module;
pub mod state_manager;

pub use app_module::AppModule;
pub use axum_state::{AppState, AppStateExt};
pub use state_manager::DependencyStateManager;
