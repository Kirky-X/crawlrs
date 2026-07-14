// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! # Dependency Injection Module (trait-kit)
//!
//! Provides standardized dependency injection for the crawlrs application using
//! the [trait-kit](https://crates.io/crates/trait-kit) library.
//!
//! ## Architecture
//!
//! ```text
//! AsyncKit<Unbuilt>
//!   │  kit.set_config(Arc<Settings>)
//!   │  kit.register::<M>() for each module
//!   ▼
//! AsyncKit<Ready>  ──►  AppState::from_kit(&kit)
//!                          │
//!                          ▼
//!                     Axum handlers (Extension<Arc<AppState>>)
//! ```
//!
//! ## Module Dependency Graph
//!
//! ```text
//! SettingsModule (config: Arc<Settings>)
//!   ├── DatabaseModule    → Arc<DatabasePool>
//!   ├── HttpModule        → Arc<reqwest::Client>
//!   └── CacheModule       → CacheComponents (SearchCache + ConcurrencyController)
//!          ├── RepositoryModule    → Repositories (depends: DatabaseModule)
//!          └── EngineModule        → EngineComponents (depends: HttpModule, SettingsModule)
//!                 └── ServiceModule         → ServicesComponents (depends: all above)
//!                     └── InfrastructureModule → InfrastructureComponents (aggregates all)
//! ```
//!
//! ## Quick Start
//!
//! ### 1. Bootstrap Application (main.rs)
//!
//! ```rust,no_run
//! use crawlrs::config::Settings;
//! use crawlrs::di::modules::*;
//! use crawlrs::di::AppState;
//! use trait_kit::AsyncKit;
//!
//! # async fn bootstrap() -> anyhow::Result<()> {
//! let settings = Arc::new(Settings::load()?);
//!
//! let mut kit = AsyncKit::new();
//! kit.set_config(settings.clone());
//! kit.register::<SettingsModule>()?;
//! kit.register::<DatabaseModule>()?;
//! kit.register::<HttpModule>()?;
//! kit.register::<CacheModule>()?;
//! kit.register::<RepositoryModule>()?;
//! kit.register::<EngineModule>()?;
//! kit.register::<InfrastructureModule>()?;
//! kit.register::<ServiceModule>()?;
//!
//! let kit = kit.build().await?;
//! let app_state = AppState::from_kit(&kit)?;
//! # Ok(())
//! # }
//! ```
//!
//! ### 2. Access Dependencies in Handlers
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use axum::extract::Extension;
//! use crawlrs::di::{AppState, AppStateExt};
//!
//! async fn my_handler(
//!     Extension(app_state): Extension<Arc<AppState>>,
//! ) -> impl IntoResponse {
//!     let task_repo = app_state.task_repo();      // Arc<dyn TaskRepository>
//!     let engine_client = app_state.engine_client(); // Arc<EngineClient>
//!     // ...
//! }
//! ```
//!
//! ## Module Catalog
//!
//! | Module | Capability | Dependencies |
//! |--------|------------|--------------|
//! | `SettingsModule` | `Arc<Settings>` | (none) |
//! | `DatabaseModule` | `Arc<DatabasePool>` | Settings |
//! | `HttpModule` | `Arc<reqwest::Client>` | Settings |
//! | `CacheModule` | `CacheComponents` | Settings |
//! | `RepositoryModule` | `Repositories` | Database |
//! | `EngineModule` | `EngineComponents` | Http, Settings |
//! | `InfrastructureModule` | `InfrastructureComponents` | Database, Http, Cache, Repository |
//! | `ServiceModule` | `ServicesComponents` | Infrastructure, Engine, Settings |
//!
//! ## Testing
//!
//! Tests use `tcf::DbRedisHandle` (testcontainers) to provision PostgreSQL + Redis,
//! then build a real `AsyncKit` via the same module registration flow as production.
//! See `axum_state::tests::build_app_state()` for the canonical test helper.
//!
//! ## Error Handling
//!
//! Module construction failures return [`ModuleBuildError`](modules::ModuleBuildError),
//! which converts to `anyhow::Error` via `?` in application code.
//!
//! `TraitKitError` (from `trait_kit::TraitKitError`) does **not** implement `Sync`
//! because it wraps `Box<dyn std::error::Error + Send + 'static>`. Use the
//! `.map_err(|e| anyhow::anyhow!("...: {e}"))?` pattern when propagating
//! `register()`, `build()`, or `require()` errors through `anyhow::Result`.
