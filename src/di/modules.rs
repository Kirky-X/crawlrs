// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! trait-kit 模块定义 — 接管所有 DI 组件的构建。
//!
//! 每个模块实现 `ModuleMeta` + `AsyncAutoBuilder`，通过 `AsyncKit` 注册和构建。
//! 模块间通过 `kit.require::<DepModule>()` 实现依赖注入。
//!
//! # 模块依赖图
//!
//! ```text
//! SettingsModule (config: Arc<Settings>)
//!   ├── DatabaseModule → Arc<DatabasePool>
//!   ├── HttpModule → Arc<reqwest::Client>
//!   └── CacheModule → CacheComponents
//!          ├── RepositoryModule → Repositories (depends: DatabaseModule)
//!          └── EngineModule → EngineComponents (depends: HttpModule, SettingsModule)
//!                 └── ServiceModule → ServicesComponents (depends: all above)
//! ```

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;
use trait_kit::TraitKitError;

use crate::bootstrap::engines::EngineComponents;
use crate::bootstrap::infrastructure::{InfrastructureComponents, Repositories};
use crate::bootstrap::services::ServicesComponents;
use crate::config::settings::Settings;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;
use crate::infrastructure::oxcache::{ConcurrencyController, SearchCache};

// =============================================================================
// 错误类型
// =============================================================================

/// 模块构建错误
#[derive(Debug, thiserror::Error)]
pub enum ModuleBuildError {
    #[error("Settings 未配置: {0}")]
    SettingsNotConfigured(String),

    #[error("数据库初始化失败: {0}")]
    DatabaseInit(String),

    #[error("HTTP 客户端初始化失败: {0}")]
    HttpInit(String),

    #[error("缓存初始化失败: {0}")]
    CacheInit(String),

    #[error("仓储初始化失败: {0}")]
    RepositoryInit(String),

    #[error("引擎初始化失败: {0}")]
    EngineInit(String),

    #[error("服务初始化失败: {0}")]
    ServiceInit(String),

    #[error("基础设施初始化失败: {0}")]
    InfrastructureInit(String),

    #[error("依赖缺失: {0}")]
    DependencyMissing(String),
}

// =============================================================================
// 缓存组件聚合
// =============================================================================

/// 缓存组件（SearchCache + ConcurrencyController）
#[derive(Clone)]
pub struct CacheComponents {
    /// 搜索缓存
    pub search_cache: Arc<SearchCache>,
    /// 并发控制器
    pub concurrency_controller: Arc<ConcurrencyController>,
}

// =============================================================================
// 模块定义 — 仅 ModuleMeta，AsyncAutoBuilder 在 Green 阶段实现
// =============================================================================

/// Settings 模块 — 提供 `Arc<Settings>`
///
/// 从 kit 的 config store 读取预先加载的 Settings。
pub struct SettingsModule;

/// 数据库模块 — 提供 `Arc<DatabasePool>`
///
/// 依赖 `SettingsModule`，使用 `init_database()` 创建连接池。
pub struct DatabaseModule;

/// HTTP 客户端模块 — 提供 `Arc<reqwest::Client>`
///
/// 依赖 `SettingsModule`，根据配置创建 reqwest::Client。
pub struct HttpModule;

/// 缓存模块 — 提供 `CacheComponents`
///
/// 依赖 `SettingsModule`，使用 `create_cache()` 创建 oxcache 实例。
pub struct CacheModule;

/// 仓储模块 — 提供 `Repositories`
///
/// 依赖 `DatabaseModule`，创建所有仓储实现实例。
pub struct RepositoryModule;

/// 引擎模块 — 提供 `EngineComponents`
///
/// 依赖 `HttpModule` 和 `SettingsModule`，创建 EngineRouter + EngineClient。
pub struct EngineModule;

/// 基础设施模块 — 提供 `InfrastructureComponents`
///
/// 依赖 `DatabaseModule`、`HttpModule`、`CacheModule`、`RepositoryModule`，
/// 聚合所有基础设施组件。
pub struct InfrastructureModule;

/// 服务模块 — 提供 `ServicesComponents`
///
/// 依赖 `InfrastructureModule`、`EngineModule`、`SettingsModule`，
/// 创建所有应用服务实例。
pub struct ServiceModule;

// =============================================================================
// ModuleMeta 实现
// =============================================================================

impl ModuleMeta for SettingsModule {
    const NAME: &'static str = "settings";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        &[]
    }
}

impl ModuleMeta for DatabaseModule {
    const NAME: &'static str = "database";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl ModuleMeta for HttpModule {
    const NAME: &'static str = "http-client";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl ModuleMeta for CacheModule {
    const NAME: &'static str = "cache";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl ModuleMeta for RepositoryModule {
    const NAME: &'static str = "repositories";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(DatabaseModule::NAME, TypeId::of::<DatabaseModule>())];
        &DEPS
    }
}

impl ModuleMeta for EngineModule {
    const NAME: &'static str = "engines";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 2] = [
            (HttpModule::NAME, TypeId::of::<HttpModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

impl ModuleMeta for InfrastructureModule {
    const NAME: &'static str = "infrastructure";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 4] = [
            (DatabaseModule::NAME, TypeId::of::<DatabaseModule>()),
            (HttpModule::NAME, TypeId::of::<HttpModule>()),
            (CacheModule::NAME, TypeId::of::<CacheModule>()),
            (RepositoryModule::NAME, TypeId::of::<RepositoryModule>()),
        ];
        &DEPS
    }
}

impl ModuleMeta for ServiceModule {
    const NAME: &'static str = "services";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 3] = [
            (
                InfrastructureModule::NAME,
                TypeId::of::<InfrastructureModule>(),
            ),
            (EngineModule::NAME, TypeId::of::<EngineModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

// =============================================================================
// TraitKitError → ModuleBuildError 转换
// =============================================================================

impl From<TraitKitError> for ModuleBuildError {
    fn from(e: TraitKitError) -> Self {
        ModuleBuildError::DependencyMissing(e.to_string())
    }
}

// =============================================================================
// AsyncAutoBuilder 实现
// =============================================================================

impl AsyncAutoBuilder for SettingsModule {
    type Capability = Arc<Settings>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            kit.config::<Arc<Settings>>()
                .map_err(|e| ModuleBuildError::SettingsNotConfigured(e.to_string()))
        })
    }
}

impl AsyncAutoBuilder for DatabaseModule {
    type Capability = Arc<DatabasePool>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let pool = crate::bootstrap::infrastructure::init_database(&settings)
                .await
                .map_err(|e| ModuleBuildError::DatabaseInit(e.to_string()))?;
            Ok(pool)
        })
    }
}

impl AsyncAutoBuilder for HttpModule {
    type Capability = Arc<reqwest::Client>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let client = crate::bootstrap::infrastructure::init_http_client(&settings)
                .map_err(|e| ModuleBuildError::HttpInit(e.to_string()))?;
            Ok(client)
        })
    }
}

impl AsyncAutoBuilder for CacheModule {
    type Capability = CacheComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;

            let search_cache = crate::infrastructure::oxcache::create_cache(&settings.cache)
                .await
                .map_err(|e| ModuleBuildError::CacheInit(e.to_string()))?;

            let max_permits = std::cmp::max(1, settings.concurrency.default_team_limit as usize);
            let concurrency_controller = Arc::new(ConcurrencyController::new(max_permits));

            Ok(CacheComponents {
                search_cache,
                concurrency_controller,
            })
        })
    }
}

impl AsyncAutoBuilder for RepositoryModule {
    type Capability = Repositories;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let db = kit.require::<DatabaseModule>()?;
            let repos = crate::bootstrap::infrastructure::init_repositories(db, &settings);
            Ok(repos)
        })
    }
}

impl AsyncAutoBuilder for EngineModule {
    type Capability = EngineComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let http_client = kit.require::<HttpModule>()?;
            let proxy_url = settings.proxy.url();
            let engines = crate::bootstrap::engines::init_engine_components(
                http_client,
                proxy_url.to_string(),
                &settings.engines,
            );
            Ok(engines)
        })
    }
}

impl AsyncAutoBuilder for InfrastructureModule {
    type Capability = InfrastructureComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let db = kit.require::<DatabaseModule>()?;
            let http_client = kit.require::<HttpModule>()?;
            let cache_components = kit.require::<CacheModule>()?;
            let repositories = kit.require::<RepositoryModule>()?;

            let cache_service = crate::bootstrap::infrastructure::init_cache_service(&settings)
                .await
                .map_err(|e| ModuleBuildError::InfrastructureInit(e.to_string()))?;

            Ok(InfrastructureComponents {
                db,
                oxcache: Some(cache_components.search_cache),
                cache_service,
                http_client,
                repositories,
            })
        })
    }
}

impl AsyncAutoBuilder for ServiceModule {
    type Capability = ServicesComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let infrastructure = kit.require::<InfrastructureModule>()?;
            let engines = kit.require::<EngineModule>()?;

            let services = crate::bootstrap::services::init_services(
                &infrastructure,
                engines.router.clone(),
                engines.engine_client.clone(),
                infrastructure.http_client.clone(),
                &settings,
            )
            .await;

            Ok(services)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::config::load_settings;
    use crate::common::test_support::testcontainers_fixtures as tcf;

    /// 测试 SettingsModule 可以注册并构建，返回 Arc<Settings>。
    #[tokio::test]
    async fn test_settings_module_builds_from_config() {
        let settings = Arc::new(load_settings().expect("Failed to load settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let cap: Arc<Settings> = kit
            .require::<SettingsModule>()
            .expect("Failed to require SettingsModule");
        assert_eq!(cap.server.port, settings.server.port);
    }

    /// 测试 HttpModule 可以注册并构建，返回 Arc<reqwest::Client>。
    #[tokio::test]
    async fn test_http_module_builds_client() {
        let settings = Arc::new(load_settings().expect("Failed to load settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");
        kit.register::<HttpModule>()
            .expect("Failed to register HttpModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let _client: Arc<reqwest::Client> = kit
            .require::<HttpModule>()
            .expect("Failed to require HttpModule");
    }

    /// 测试 CacheModule 可以注册并构建，返回 CacheComponents。
    #[tokio::test]
    async fn test_cache_module_builds_components() {
        let settings = Arc::new(load_settings().expect("Failed to load settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");
        kit.register::<CacheModule>()
            .expect("Failed to register CacheModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let cache: CacheComponents = kit
            .require::<CacheModule>()
            .expect("Failed to require CacheModule");
        assert!(cache.concurrency_controller.available_permits() > 0);
    }

    /// 测试 EngineModule 可以注册并构建，返回 EngineComponents。
    #[tokio::test]
    async fn test_engine_module_builds_components() {
        let settings = Arc::new(load_settings().expect("Failed to load settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");
        kit.register::<HttpModule>()
            .expect("Failed to register HttpModule");
        kit.register::<EngineModule>()
            .expect("Failed to register EngineModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let engines: EngineComponents = kit
            .require::<EngineModule>()
            .expect("Failed to require EngineModule");
        assert!(!engines.engines.is_empty());
    }

    /// 测试 DatabaseModule 可以注册并构建，返回 Arc<DatabasePool>。
    /// 需要 Docker (PostgreSQL via testcontainers)。
    #[tokio::test]
    async fn tc_database_module_builds_pool() {
        if !tcf::docker_available().await {
            eprintln!("[skip] Docker unavailable — tc_database_module_builds_pool");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings =
            Arc::new(tcf::settings_with_urls(&pg.url).expect("Failed to build settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");
        kit.register::<DatabaseModule>()
            .expect("Failed to register DatabaseModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let _pool: Arc<DatabasePool> = kit
            .require::<DatabaseModule>()
            .expect("Failed to require DatabaseModule");
    }

    /// 测试 RepositoryModule 可以注册并构建，返回 Repositories。
    /// 需要 Docker (PostgreSQL via testcontainers)。
    #[tokio::test]
    async fn tc_repository_module_builds_repositories() {
        if !tcf::docker_available().await {
            eprintln!("[skip] Docker unavailable — tc_repository_module_builds_repositories");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings =
            Arc::new(tcf::settings_with_urls(&pg.url).expect("Failed to build settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<SettingsModule>()
            .expect("Failed to register SettingsModule");
        kit.register::<DatabaseModule>()
            .expect("Failed to register DatabaseModule");
        kit.register::<RepositoryModule>()
            .expect("Failed to register RepositoryModule");

        let kit = kit.build().await.expect("Failed to build kit");
        let repos: Repositories = kit
            .require::<RepositoryModule>()
            .expect("Failed to require RepositoryModule");
        // 验证所有仓储实例均已创建（Arc 强引用计数 >= 1）
        assert!(Arc::strong_count(&repos.task_repo) >= 1);
        assert!(Arc::strong_count(&repos.credits_repo) >= 1);
        assert!(Arc::strong_count(&repos.crawl_repo) >= 1);
    }

    /// 测试 InfrastructureModule 可以注册并构建，返回 InfrastructureComponents。
    /// 需要 Docker (PostgreSQL + Redis via testcontainers)。
    #[tokio::test]
    async fn tc_infrastructure_module_builds_components() {
        if !tcf::docker_available().await {
            eprintln!("[skip] Docker unavailable — tc_infrastructure_module_builds_components");
            return;
        }
        let combo = match tcf::DbHandle::start().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings =
            Arc::new(tcf::settings_with_urls(&combo.pg.url).expect("Failed to build settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings);
        kit.register::<SettingsModule>().unwrap();
        kit.register::<DatabaseModule>().unwrap();
        kit.register::<HttpModule>().unwrap();
        kit.register::<CacheModule>().unwrap();
        kit.register::<RepositoryModule>().unwrap();
        kit.register::<InfrastructureModule>().unwrap();

        let kit = kit.build().await.expect("Failed to build kit");
        let infra: InfrastructureComponents = kit
            .require::<InfrastructureModule>()
            .expect("Failed to require InfrastructureModule");
        assert!(infra.oxcache.is_some());
    }

    /// 测试 ServiceModule 可以注册并构建，返回 ServicesComponents。
    /// 需要 Docker (PostgreSQL + Redis via testcontainers)。
    #[tokio::test]
    async fn tc_service_module_builds_components() {
        if !tcf::docker_available().await {
            eprintln!("[skip] Docker unavailable — tc_service_module_builds_components");
            return;
        }
        let combo = match tcf::DbHandle::start().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings =
            Arc::new(tcf::settings_with_urls(&combo.pg.url).expect("Failed to build settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings);
        kit.register::<SettingsModule>().unwrap();
        kit.register::<DatabaseModule>().unwrap();
        kit.register::<HttpModule>().unwrap();
        kit.register::<CacheModule>().unwrap();
        kit.register::<RepositoryModule>().unwrap();
        kit.register::<EngineModule>().unwrap();
        kit.register::<InfrastructureModule>().unwrap();
        kit.register::<ServiceModule>().unwrap();

        let kit = kit.build().await.expect("Failed to build kit");
        let _services: ServicesComponents = kit
            .require::<ServiceModule>()
            .expect("Failed to require ServiceModule");
    }

    /// 测试模块依赖图拓扑排序正确 — 所有模块同时注册，build 按依赖顺序构建。
    /// 需要 Docker (PostgreSQL + Redis via testcontainers)。
    #[tokio::test]
    async fn tc_all_modules_registered_simultaneously() {
        if !tcf::docker_available().await {
            eprintln!("[skip] Docker unavailable — tc_all_modules_registered_simultaneously");
            return;
        }
        let combo = match tcf::DbHandle::start().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings =
            Arc::new(tcf::settings_with_urls(&combo.pg.url).expect("Failed to build settings"));

        let mut kit = AsyncKit::new();
        kit.set_config(settings);
        kit.register::<SettingsModule>().unwrap();
        kit.register::<DatabaseModule>().unwrap();
        kit.register::<HttpModule>().unwrap();
        kit.register::<CacheModule>().unwrap();
        kit.register::<RepositoryModule>().unwrap();
        kit.register::<EngineModule>().unwrap();
        kit.register::<InfrastructureModule>().unwrap();
        kit.register::<ServiceModule>().unwrap();

        let kit = kit.build().await.expect("Failed to build all modules");

        assert!(kit.contains::<SettingsModule>());
        assert!(kit.contains::<DatabaseModule>());
        assert!(kit.contains::<HttpModule>());
        assert!(kit.contains::<CacheModule>());
        assert!(kit.contains::<RepositoryModule>());
        assert!(kit.contains::<EngineModule>());
        assert!(kit.contains::<InfrastructureModule>());
        assert!(kit.contains::<ServiceModule>());
    }
}
