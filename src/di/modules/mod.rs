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
//! SettingsModule (config: Arc<Settings>)           ← 根节点，无依赖
//!   ├── DatabaseModule → Arc<DatabasePool>          (dep: Settings)
//!   ├── HttpModule → Arc<reqwest::Client>           (dep: Settings)
//!   └── CacheModule → CacheComponents               (dep: Settings)
//!          ├── RepositoryModule → Repositories       (dep: Database, Settings)
//!          ├── EngineModule → EngineComponents       (dep: Http, Settings)
//!          └── InfrastructureModule → InfraComponents(dep: Database, Http, Cache, Repository, Settings)
//!                 └── ServiceModule → ServicesComponents (dep: Infrastructure, Engine, Settings)
//! ```

// ---------------------------------------------------------------------------
// 子模块声明
// ---------------------------------------------------------------------------

pub mod cache_module;
pub mod database_module;
pub mod engine_module;
pub mod http_module;
pub mod infrastructure_module;
pub mod repository_module;
pub mod service_module;
pub mod settings_module;

// ---------------------------------------------------------------------------
// Re-export — 保持所有原有公共路径可用
// ---------------------------------------------------------------------------

pub use cache_module::CacheModule;
pub use database_module::DatabaseModule;
pub use engine_module::EngineModule;
pub use http_module::HttpModule;
pub use infrastructure_module::InfrastructureModule;
pub use repository_module::RepositoryModule;
pub use service_module::ServiceModule;
pub use settings_module::SettingsModule;

// ---------------------------------------------------------------------------
// 共享类型
// ---------------------------------------------------------------------------

use trait_kit::TraitKitError;

use crate::infrastructure::oxcache::{ConcurrencyController, SearchCache};
use std::sync::Arc;

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

/// TraitKitError → ModuleBuildError 转换
impl From<TraitKitError> for ModuleBuildError {
    fn from(e: TraitKitError) -> Self {
        ModuleBuildError::DependencyMissing(e.to_string())
    }
}

/// 缓存组件（SearchCache + ConcurrencyController）
#[derive(Clone)]
pub struct CacheComponents {
    /// 搜索缓存
    pub search_cache: Arc<SearchCache>,
    /// 并发控制器
    pub concurrency_controller: Arc<ConcurrencyController>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::config::load_settings;
    use crate::bootstrap::engines::EngineComponents;
    use crate::bootstrap::infrastructure::{InfrastructureComponents, Repositories};
    use crate::bootstrap::services::ServicesComponents;
    use crate::common::test_support::testcontainers_fixtures as tcf;
    use crate::config::settings::Settings;
    use crate::infrastructure::database::dbnexus_connection::DatabasePool;
    use trait_kit::kit::AsyncKit;

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
    /// 需要 Docker (PostgreSQL via testcontainers)。
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
    /// 需要 Docker (PostgreSQL via testcontainers)。
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

        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
        let kit = kit.build().await.expect("Failed to build kit");
        let _services: ServicesComponents = kit
            .require::<ServiceModule>()
            .expect("Failed to require ServiceModule");
    }

    /// 测试模块依赖图拓扑排序正确 — 所有模块同时注册，build 按依赖顺序构建。
    /// 需要 Docker (PostgreSQL via testcontainers)。
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

        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
