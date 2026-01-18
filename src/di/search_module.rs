// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search module for Shaku dependency injection.
//!
//! This module provides Shaku components for search layer dependencies
//! including SearchClient, SearchAggregator, and individual search engine implementations.

use shaku::Component;
use std::sync::Arc;

use crate::domain::search::engine::SearchEngine;
use crate::search::aggregator::SearchAggregator;
use crate::search::client::SearchClient;

/// Component parameters for SearchModule
#[derive(shaku::ComponentParameters)]
pub struct SearchModuleParameters {
    /// Search engine settings
    pub settings: Arc<crate::config::settings::Settings>,
}

/// SearchAggregator component
#[derive(Component)]
#[shaku(interface = SearchAggregator)]
pub struct SearchAggregatorComponent {
    /// Vector of search engines
    #[shaku(inject)]
    engines: Vec<Arc<dyn SearchEngine>>,

    /// Timeout in milliseconds
    timeout_ms: u64,
}

impl SearchAggregatorComponent {
    pub fn new(engines: Vec<Arc<dyn SearchEngine>>, timeout_ms: u64) -> Self {
        Self {
            engines,
            timeout_ms,
        }
    }
}

impl SearchAggregator for SearchAggregatorComponent {
    // Implementation delegated to internal aggregator
}

/// SearchClient component
#[derive(Component)]
#[shaku(interface = SearchClient)]
pub struct SearchClientComponent {
    /// Search aggregator
    #[shaku(inject)]
    aggregator: Arc<dyn SearchAggregator>,
}

impl SearchClient for SearchClientComponent {
    // Implementation delegated to internal client
}

/// Search module for Shaku DI
///
/// This module provides all search components including:
/// - SearchClient (main interface for search operations)
/// - SearchAggregator (result aggregation and deduplication)
/// - Individual search engine implementations
shaku::module! {
    pub SearchModule {
        components = [
            SearchAggregatorComponent,
            SearchClientComponent,
        ],
        providers = []
    }
}
