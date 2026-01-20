// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application module for Shaku dependency injection.
//!
//! This is the root module that combines all sub-modules (infrastructure, engine, search)
//! and provides the main entry point for dependency resolution in the crawlrs application.

use std::sync::Arc;

use crate::config::settings::Settings;

/// Application module - the root module for Shaku DI
///
/// This module combines all sub-modules and provides the main entry point
/// for dependency resolution in the crawlrs application.
pub struct AppModule;

impl AppModule {
    /// Create an AppModule with default settings
    pub fn create(_settings: Arc<Settings>) -> Self {
        Self
    }
}
