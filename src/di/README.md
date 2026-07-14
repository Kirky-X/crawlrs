// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! # Shaku Dependency Injection Module
//!
//! This module provides a standardized dependency injection framework for managing
//! component dependencies across the crawlrs application using the [Shaku](https://github.com/AzureMarker/shaku) library.
//!
//! ## Overview
//!
//! The DI module implements a clean architecture with the following layers:
//!
//! ```text
//! +------------------+     +------------------+     +------------------+
//! |    AppModule     |     |   ServiceModule  |     |  SearchModule   |
//! |   (Root Module)  | --> |  (Services)      |     |  (Search)       |
//! +------------------+     +------------------+     +------------------+
//!        |                       |                       |
//!        v                       v                       v
//! +------------------+     +------------------+     +------------------+
//! | Infrastructure   |     |   EngineModule   |     |                 |
//! | Module (DB/Redis)|     |  (Scraping)      |     |                 |
//! +------------------+     +------------------+     +------------------+
//! ```

//! ## Quick Start
//!
//! ### 1. Creating an Application Module
//!
//! ```rust
//! use crawlrs::di::AppModule;
//! use std::sync::Arc;
//! use crawlrs::config::settings::Settings;
//!
//! let settings = Arc::new(settings);
//! let module = AppModule::builder().build();
//!
//! // Resolve components
//! let task_repo: &dyn TaskRepository = module.resolve_ref();
//! ```
//!
//! ### 2. Using Components in Handlers
//!
//! ```rust
//! use crawlrs::di::ShakuAppState;
//! use crawlrs::di::ShakuStateExt;
//!
//! async fn my_handler(state: ShakuAppState) {
//!     let task_repo = state.task_repo();
//!     // Use task_repo...
//! }
//! ```
//!
//! ### 3. Testing with Mock Components
//!
//! ```rust
//! use crawlrs::di::test_module::{create_test_module, TestModule};
//!
//! let test_module = create_test_module();
//! let task_repo: &dyn TaskRepository = test_module.resolve_ref();
//! ```
//!
//! ## Module Structure
//!
//! | Module | Purpose | Components |
//! |--------|---------|------------|
//! | [`AppModule`](app_module::AppModule) | Root module combining all sub-modules | - |
//! | [`InfrastructureModule`](infrastructure_module::InfrastructureModule) | Database, Redis, Repositories | DatabasePool, RedisClient, TaskRepository, etc. |
//! | [`EngineModule`](engine_module::EngineModule) | Scraping engines | EngineClient, EngineRouter, ReqwestEngine |
//! | [`SearchModule`](search_module::SearchModule) | Search functionality | SearchClient, SearchAggregator |
//! | [`ServiceModule`](service_module::ServiceModule) | Business services | RateLimitingService, TeamService, WebhookService, RobotsChecker, TeamSemaphore |
//! | [`TestModule`](test_module::TestModule) | Test components | InMemoryTaskRepository, MockRateLimitingService, etc. |

//! ## Available Components
//!
//! ### Infrastructure Components
//!
//! | Component | Interface | Description |
//! |-----------|-----------|-------------|
//! | `DatabasePoolComponent` | `DatabasePool` | PostgreSQL connection pool |
//! | `RedisClientComponent` | `RedisClient` | Redis client for caching |
//! | `TaskRepositoryComponent` | `TaskRepository` | Task data access |
//! | `CreditsRepositoryComponent` | `CreditsRepository` | Credits data access |
//! | `CrawlRepositoryComponent` | `CrawlRepository` | Crawl data access |
//! | `ScrapeResultRepositoryComponent` | `ScrapeResultRepository` | Scrape result data access |
//! | `WebhookRepositoryComponent` | `WebhookRepository` | Webhook data access |
//! | `WebhookEventRepositoryComponent` | `WebhookEventRepository` | Webhook event data access |
//! | `TasksBacklogRepositoryComponent` | `TasksBacklogRepository` | Tasks backlog data access |
//! | `GeoRestrictionRepositoryComponent` | `GeoRestrictionRepository` | Geo restriction data access |
//! | `TaskQueueComponent` | `TaskQueue` | PostgreSQL-based task queue |
//!
//! ### Service Components
//!
//! | Component | Interface | Description |
//! |-----------|-----------|-------------|
//! | `RateLimitingServiceComponent` | `RateLimitingService` | Distributed rate limiting |
//! | `TeamServiceComponent` | `TeamService` | Team management and geo restrictions |
//! | `WebhookServiceComponent` | `WebhookService` | Webhook notifications |
//! | `CreateScrapeUseCaseComponent` | `CreateScrapeUseCase` | Scrape operation use case |
//! | `RobotsCheckerComponent` | `RobotsCheckerTrait` | robots.txt compliance checking |
//! | `TeamSemaphoreComponent` | - | Per-team concurrency control |

//! ## Dependency States
//!
//! The [`DependencyStateManager`](state_manager::DependencyStateManager) tracks component initialization states:
//!
//! - `NotInitialized` - Component not yet initialized
//! - `Initializing` - Component is initializing
//! - `Ready` - Component is ready for use
//! - `Failed(String)` - Component failed to initialize

//! ## Best Practices
//!
//! 1. **Define interfaces in domain layer** - Traits should be defined in the domain layer
//! 2. **Add Interface constraint** - Extend traits with `shaku::Interface`
//! 3. **Implement in infrastructure** - Actual implementations in infrastructure layer
//! 4. **Create Shaku components** - Wrap implementations in Shaku components
//! 5. **Register in modules** - Add components to appropriate modules
//!
//! ## Example: Creating a New Component
//!
//! ### Step 1: Define Interface
//!
//! ```rust
//! use shaku::Interface;
//!
//! pub trait MyService: Interface + Send + Sync {
//!     fn do_something(&self) -> Result<(), Error>;
//! }
//! ```
//!
//! ### Step 2: Implement Component
//!
//! ```rust
//! use shaku::Component;
//!
//! #[derive(Component)]
//! #[shaku(interface = MyService)]
//! pub struct MyServiceImpl {
//!     #[shaku(inject)]
//!     other_service: Arc<dyn OtherService>,
//! }
//!
//! impl MyService for MyServiceImpl {
//!     fn do_something(&self) -> Result<(), Error> {
//!         // Implementation...
//!     }
//! }
//! ```
//!
//! ### Step 3: Register in Module
//!
//! ```rust
//! shaku::module! {
//!     MyModule {
//!         components = [MyServiceImpl, OtherServiceImpl],
//!         providers = []
//!     }
//! }
//! ```
//!
//! ## Migration from Manual DI
//!
//! The existing bootstrap module still works. To migrate gradually:
//!
//! 1. Keep existing `bootstrap` module for production
//! 2. Create new DI modules alongside
//! 3. Migrate handlers to use `ShakuAppState`
//! 4. Switch entry point when ready
//!
//! ## Performance Considerations
//!
//! - Shaku performs dependency resolution at compile time
//! - Runtime overhead is minimal (just method calls)
//! - Use `Arc<dyn Trait>` for shared dependencies
//! - Components are singletons by default
//!
//! ## Testing
//!
//! Use `TestModule` for unit tests without external dependencies:
//!
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!
//!     #[tokio::test]
//!     async fn test_with_mocks() {
//!         let module = create_test_module();
//!         let task_repo = module.resolve::<dyn TaskRepository>().unwrap();
//!         // Test without database...
//!     }
//! }
//! ```
//!
//! ## Migration Guide: Extension<T> to ShakuAppState
//!
//! ### Before (Extension<T> Pattern)
//!
//! ```rust
//! use axum::{extract::Extension, Json};
//! use std::sync::Arc;
//!
//! pub async fn create_scrape(
//!     Extension(queue): Extension<Arc<dyn TaskQueue>>,
//!     Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
//!     Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
//!     Extension(auth_state): Extension<AuthState>,
//!     Json(payload): Json<ScrapeRequestDto>,
//! ) -> impl IntoResponse {
//!     // Use dependencies directly
//!     queue.enqueue(task).await?;
//! }
//! ```
//!
//! ### After (ShakuAppState Pattern)
//!
//! ```rust
//! use axum::{extract::State, Json};
//! use crate::di::{ShakuAppState, ShakuStateExt};
//!
//! pub async fn create_scrape_shaku(
//!     state: ShakuAppState,
//!     auth_state: AuthState,
//!     Json(payload): Json<ScrapeRequestDto>,
//! ) -> impl IntoResponse {
//!     // Access dependencies through ShakuStateExt trait
//!     let task_queue = state.task_queue();
//!     let rate_limiting_service = state.rate_limiting_service();
//!     let task_repository = state.task_repo();
//!
//!     // Use dependencies
//!     task_queue.enqueue(task).await?;
//! }
//! ```
//!
//! ### Key Changes
//!
//! | Aspect | Extension<T> Pattern | ShakuAppState Pattern |
//! |--------|---------------------|----------------------|
//! | Handler signature | 6+ parameters | 2-3 parameters |
//! | Adding dependencies | Add new Extension layer | Add accessor to trait |
//! | Type safety | Manual annotation | Automatic via trait |
//! | Code organization | Distributed | Centralized |
//! | Testing | Manual mocking | TestModule available |
//!
//! ### Router Configuration
//!
//! ```rust
//! use axum::{routing::post, Router};
//! use crate::di::{AppModule, ShakuAppState};
//!
//! fn create_router(module: &AppModule) -> Result<Router, shaku::Error> {
//!     let state = ShakuAppState::from_module(module)?;
//!
//!     Router::new()
//!         .route("/api/v1/scrape", post(create_scrape_shaku))
//!         .with_state(state)
//! }
//! ```
//!
//! ### Mixed Usage (Migration Path)
//!
//! During migration, you can support both patterns:
//!
//! ```rust
//! fn create_mixed_router(
//!     state: ShakuAppState,
//!     legacy_service: Arc<LegacyService>,
//! ) -> Router {
//!     Router::new()
//!         // New Shaku handlers
//!         .route("/api/v1/scrape", post(create_scrape_shaku))
//!         // Legacy handlers
//!         .route("/api/v1/legacy", post(legacy_handler))
//!         .layer(Extension(legacy_service))
//!         .with_state(state)
//! }
//! ```
//!
//! ## Comparison: Extension vs with_state
//!
//! ### Extension Layer Approach
//!
//! - Add each dependency as `Extension<T>` layer
//! - Handler receives dependencies directly as parameters
//! - Simple to understand for small apps
//! - Becomes unwieldy with many dependencies
//!
//! ### with_state Approach (Recommended)
//!
//! - Single `ShakuAppState` contains all dependencies
//! - Handlers use `ShakuStateExt` trait to access dependencies
//! - Cleaner function signatures
//! - Easier to add new dependencies
//! - Consistent pattern across all handlers
