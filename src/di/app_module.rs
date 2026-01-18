// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Application module for Shaku dependency injection.
//!
//! This is the root module that combines all sub-modules (infrastructure, engine, search)
//! and provides the main entry point for dependency resolution.

use shaku::{Component, HasComponent, Interface, Module};
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::di::engine_module::EngineModule;
use crate::di::infrastructure_module::InfrastructureModule;
use crate::di::search_module::SearchModule;
use crate::di::service_module::ServiceModule;

/// Component parameters for AppModule
#[derive(shaku::ComponentParameters)]
pub struct AppModuleParameters {
    /// Application settings
    pub settings: Arc<Settings>,
}

/// Application module - the root module for Shaku DI
///
/// This module combines all sub-modules and provides the main entry point
/// for dependency resolution in the crawlrs application.
///
/// # Module Dependencies
///
/// - InfrastructureModule (database, Redis, repositories)
/// - EngineModule (scraping engines)
/// - SearchModule (search engines)
shaku::module! {
    pub AppModule {
        components = [],
        providers = [],

        use InfrastructureModule {
            components = [
                crate::di::infrastructure_module::DatabasePoolComponent,
                crate::di::infrastructure_module::RedisClientComponent,
                crate::di::infrastructure_module::TaskRepositoryComponent,
                crate::di::infrastructure_module::CreditsRepositoryComponent,
                crate::di::infrastructure_module::CrawlRepositoryComponent,
                crate::di::infrastructure_module::ScrapeResultRepositoryComponent,
                crate::di::infrastructure_module::WebhookRepositoryComponent,
                crate::di::infrastructure_module::WebhookEventRepositoryComponent,
                crate::di::infrastructure_module::TasksBacklogRepositoryComponent,
                crate::di::infrastructure_module::GeoRestrictionRepositoryComponent,
                crate::di::infrastructure_module::StorageRepositoryComponent,
                crate::di::infrastructure_module::TaskQueueComponent,
            ],
            providers = []
        }

        use EngineModule {
            components = [
                crate::di::engine_module::EngineRouterComponent,
                crate::di::engine_module::EngineHealthMonitorComponent,
                crate::di::engine_module::EngineClientComponent,
                crate::di::engine_module::ReqwestEngineComponent,
            ],
            providers = []
        }

        use SearchModule {
            components = [
                crate::di::search_module::SearchAggregatorComponent,
                crate::di::search_module::SearchClientComponent,
            ],
            providers = []
        }

        use ServiceModule {
            components = [
                crate::di::service_module::RateLimitingServiceComponent,
                crate::di::service_module::TeamServiceComponent,
                crate::di::service_module::WebhookServiceComponent,
                crate::di::service_module::CreateScrapeUseCaseComponent,
                crate::di::service_module::RobotsCheckerComponent,
                crate::di::service_module::TeamSemaphoreComponent,
            ],
            providers = []
        }
    }
}

/// Helper function to create an AppModule with default settings
pub fn create_app_module(settings: Arc<Settings>) -> AppModule {
    AppModule::builder()
        .with_component_parameters::<crate::di::infrastructure_module::DatabasePoolComponent>(
            crate::di::infrastructure_module::DatabasePoolComponent::from_parameters(
                crate::di::infrastructure_module::InfrastructureModuleParameters {
                    settings: settings.clone(),
                },
            ),
        )
        .build()
}
