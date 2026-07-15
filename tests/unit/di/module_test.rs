// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! trait-kit 模块接口测试
//!
//! 验证 DI 模块的 ModuleMeta 元数据（NAME/dependencies）与 AsyncKit 注册/解析行为。
//! 仅覆盖不需要 Docker 的模块（Settings/Http/Cache/Engine）；Docker 依赖模块
//! （Database/Repository/Infrastructure/Service）由 src/di/modules.rs 内联测试覆盖。

use std::any::TypeId;
use std::sync::Arc;

use trait_kit::core::ModuleMeta;
use trait_kit::kit::AsyncKit;

use crawlrs::bootstrap::config::load_settings;
use crawlrs::config::settings::Settings;
use crawlrs::di::modules::{
    CacheComponents, CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule,
    ModuleBuildError, RepositoryModule, ServiceModule, SettingsModule,
};
use crawlrs::di::AppState;

// =============================================================================
// ModuleMeta::NAME 常量校验
// =============================================================================

#[test]
fn settings_module_name() {
    assert_eq!(SettingsModule::NAME, "settings");
}

#[test]
fn database_module_name() {
    assert_eq!(DatabaseModule::NAME, "database");
}

#[test]
fn http_module_name() {
    assert_eq!(HttpModule::NAME, "http-client");
}

#[test]
fn cache_module_name() {
    assert_eq!(CacheModule::NAME, "cache");
}

#[test]
fn repository_module_name() {
    assert_eq!(RepositoryModule::NAME, "repositories");
}

#[test]
fn engine_module_name() {
    assert_eq!(EngineModule::NAME, "engines");
}

#[test]
fn infrastructure_module_name() {
    assert_eq!(InfrastructureModule::NAME, "infrastructure");
}

#[test]
fn service_module_name() {
    assert_eq!(ServiceModule::NAME, "services");
}

// =============================================================================
// ModuleMeta::dependencies() 依赖图校验
// =============================================================================

/// 辅助：断言依赖列表中包含指定 (name, TypeId) 对。
fn assert_dep_contains(
    deps: &[(&'static str, TypeId)],
    expected_name: &str,
    expected_type_id: TypeId,
) {
    let found = deps
        .iter()
        .any(|(n, tid)| *n == expected_name && *tid == expected_type_id);
    assert!(
        found,
        "dependency list missing expected entry: ({expected_name}, {expected_type_id:?})"
    );
}

#[test]
fn settings_module_has_no_dependencies() {
    let deps = SettingsModule::dependencies();
    assert!(
        deps.is_empty(),
        "SettingsModule should have no dependencies"
    );
}

#[test]
fn database_module_depends_on_settings() {
    let deps = DatabaseModule::dependencies();
    assert_eq!(deps.len(), 1);
    assert_dep_contains(deps, SettingsModule::NAME, TypeId::of::<SettingsModule>());
}

#[test]
fn http_module_depends_on_settings() {
    let deps = HttpModule::dependencies();
    assert_eq!(deps.len(), 1);
    assert_dep_contains(deps, SettingsModule::NAME, TypeId::of::<SettingsModule>());
}

#[test]
fn cache_module_depends_on_settings() {
    let deps = CacheModule::dependencies();
    assert_eq!(deps.len(), 1);
    assert_dep_contains(deps, SettingsModule::NAME, TypeId::of::<SettingsModule>());
}

#[test]
fn repository_module_depends_on_database() {
    let deps = RepositoryModule::dependencies();
    assert_eq!(deps.len(), 1);
    assert_dep_contains(deps, DatabaseModule::NAME, TypeId::of::<DatabaseModule>());
}

#[test]
fn engine_module_depends_on_http_and_settings() {
    let deps = EngineModule::dependencies();
    assert_eq!(deps.len(), 2);
    assert_dep_contains(deps, HttpModule::NAME, TypeId::of::<HttpModule>());
    assert_dep_contains(deps, SettingsModule::NAME, TypeId::of::<SettingsModule>());
}

#[test]
fn infrastructure_module_depends_on_four_modules() {
    let deps = InfrastructureModule::dependencies();
    assert_eq!(deps.len(), 4);
    assert_dep_contains(deps, DatabaseModule::NAME, TypeId::of::<DatabaseModule>());
    assert_dep_contains(deps, HttpModule::NAME, TypeId::of::<HttpModule>());
    assert_dep_contains(deps, CacheModule::NAME, TypeId::of::<CacheModule>());
    assert_dep_contains(
        deps,
        RepositoryModule::NAME,
        TypeId::of::<RepositoryModule>(),
    );
}

#[test]
fn service_module_depends_on_three_modules() {
    let deps = ServiceModule::dependencies();
    assert_eq!(deps.len(), 3);
    assert_dep_contains(
        deps,
        InfrastructureModule::NAME,
        TypeId::of::<InfrastructureModule>(),
    );
    assert_dep_contains(deps, EngineModule::NAME, TypeId::of::<EngineModule>());
    assert_dep_contains(deps, SettingsModule::NAME, TypeId::of::<SettingsModule>());
}

// =============================================================================
// 依赖图拓扑完整性 — 所有引用的依赖名必须有对应模块（无悬挂引用）
// =============================================================================

#[test]
fn dependency_graph_has_no_dangling_references() {
    // 所有模块名 → TypeId 的有效映射
    let valid_names: &[(&str, TypeId)] = &[
        (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        (DatabaseModule::NAME, TypeId::of::<DatabaseModule>()),
        (HttpModule::NAME, TypeId::of::<HttpModule>()),
        (CacheModule::NAME, TypeId::of::<CacheModule>()),
        (RepositoryModule::NAME, TypeId::of::<RepositoryModule>()),
        (EngineModule::NAME, TypeId::of::<EngineModule>()),
        (
            InfrastructureModule::NAME,
            TypeId::of::<InfrastructureModule>(),
        ),
        (ServiceModule::NAME, TypeId::of::<ServiceModule>()),
    ];

    let all_deps = [
        SettingsModule::dependencies(),
        DatabaseModule::dependencies(),
        HttpModule::dependencies(),
        CacheModule::dependencies(),
        RepositoryModule::dependencies(),
        EngineModule::dependencies(),
        InfrastructureModule::dependencies(),
        ServiceModule::dependencies(),
    ];

    for deps in all_deps {
        for (name, tid) in deps {
            let found = valid_names.iter().any(|(n, t)| n == name && t == tid);
            assert!(found, "dangling dependency reference: ({name}, {tid:?})");
        }
    }
}

// =============================================================================
// AsyncKit 注册/解析行为 — 非 Docker 模块
// =============================================================================

/// SettingsModule 可注册、构建并解析，返回 Arc<Settings>。
#[tokio::test]
async fn settings_module_registers_and_resolves() {
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
    assert!(kit.contains::<SettingsModule>());
}

/// HttpModule 依赖 SettingsModule，可注册、构建并解析，返回 Arc<reqwest::Client>。
#[tokio::test]
async fn http_module_registers_and_resolves() {
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

    assert!(kit.contains::<SettingsModule>());
    assert!(kit.contains::<HttpModule>());
}

/// CacheModule 依赖 SettingsModule，可注册、构建并解析，返回 CacheComponents。
#[tokio::test]
async fn cache_module_registers_and_resolves() {
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

    // 并发控制器可用 permit 数 > 0
    assert!(cache.concurrency_controller.available_permits() > 0);
    assert!(kit.contains::<CacheModule>());
}

/// EngineModule 依赖 HttpModule + SettingsModule，可注册、构建并解析，返回 EngineComponents。
#[tokio::test]
async fn engine_module_registers_and_resolves() {
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
    let engines = kit
        .require::<EngineModule>()
        .expect("Failed to require EngineModule");

    assert!(!engines.engines.is_empty());
    assert!(kit.contains::<EngineModule>());
}

/// 未注册的模块在 require 时应返回 TraitKitError（kit 层错误：missing capability）。
#[tokio::test]
async fn require_unregistered_module_returns_error() {
    use trait_kit::TraitKitError;

    let settings = Arc::new(load_settings().expect("Failed to load settings"));

    let mut kit = AsyncKit::new();
    kit.set_config(settings);
    // 只注册 SettingsModule，不注册 HttpModule
    kit.register::<SettingsModule>()
        .expect("Failed to register SettingsModule");

    let kit = kit.build().await.expect("Failed to build kit");
    let result: Result<Arc<reqwest::Client>, TraitKitError> = kit.require::<HttpModule>();
    assert!(
        result.is_err(),
        "requiring unregistered HttpModule should error"
    );
    let err = result.unwrap_err();
    // require 返回 TraitKitError，未注册的 capability 报告 missing
    let msg = err.to_string();
    assert!(
        msg.contains("http-client"),
        "expected missing-capability message mentioning http-client, got: {msg}"
    );
}

/// 未设置 config 时构建 SettingsModule 应在 build 阶段失败（BuildFailed 包装 SettingsNotConfigured）。
#[tokio::test]
async fn settings_module_without_config_errors() {
    use trait_kit::TraitKitError;

    let mut kit = AsyncKit::new();
    kit.register::<SettingsModule>()
        .expect("Failed to register SettingsModule");

    // build() 会立即构建所有已注册模块；SettingsModule 缺少 config 时返回 SettingsNotConfigured，
    // kit 将其包装为 BuildFailed { context: "settings", source: SettingsNotConfigured(...) }
    let result: Result<_, TraitKitError> = kit.build().await;
    assert!(
        result.is_err(),
        "build should fail when SettingsModule lacks config"
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("settings") || msg.contains("Settings"),
        "expected build-failed message mentioning settings, got: {msg}"
    );
    assert!(
        msg.contains("config") || msg.contains("Settings"),
        "expected message about missing config, got: {msg}"
    );
}

// =============================================================================
// ModuleBuildError Display impl 校验
// =============================================================================

#[test]
fn module_build_error_display_settings_not_configured() {
    let err = ModuleBuildError::SettingsNotConfigured("missing key".into());
    assert_eq!(err.to_string(), "Settings 未配置: missing key");
}

#[test]
fn module_build_error_display_database_init() {
    let err = ModuleBuildError::DatabaseInit("connection refused".into());
    assert_eq!(err.to_string(), "数据库初始化失败: connection refused");
}

#[test]
fn module_build_error_display_http_init() {
    let err = ModuleBuildError::HttpInit("tls error".into());
    assert_eq!(err.to_string(), "HTTP 客户端初始化失败: tls error");
}

#[test]
fn module_build_error_display_cache_init() {
    let err = ModuleBuildError::CacheInit("backend unavailable".into());
    assert_eq!(err.to_string(), "缓存初始化失败: backend unavailable");
}

#[test]
fn module_build_error_display_repository_init() {
    let err = ModuleBuildError::RepositoryInit("schema mismatch".into());
    assert_eq!(err.to_string(), "仓储初始化失败: schema mismatch");
}

#[test]
fn module_build_error_display_engine_init() {
    let err = ModuleBuildError::EngineInit("no engines configured".into());
    assert_eq!(err.to_string(), "引擎初始化失败: no engines configured");
}

#[test]
fn module_build_error_display_service_init() {
    let err = ModuleBuildError::ServiceInit("dependency cycle".into());
    assert_eq!(err.to_string(), "服务初始化失败: dependency cycle");
}

#[test]
fn module_build_error_display_infrastructure_init() {
    let err = ModuleBuildError::InfrastructureInit("pool exhausted".into());
    assert_eq!(err.to_string(), "基础设施初始化失败: pool exhausted");
}

#[test]
fn module_build_error_display_dependency_missing() {
    let err = ModuleBuildError::DependencyMissing("HttpModule not registered".into());
    assert_eq!(err.to_string(), "依赖缺失: HttpModule not registered");
}

// =============================================================================
// TraitKitError → ModuleBuildError 转换
// =============================================================================

#[test]
fn traitkit_error_converts_to_dependency_missing() {
    use trait_kit::TraitKitError;

    let original = TraitKitError::DependencyMissing {
        module: "service",
        missing: "infrastructure",
    };
    let converted: ModuleBuildError = original.into();
    assert!(
        matches!(converted, ModuleBuildError::DependencyMissing(_)),
        "TraitKitError should convert to DependencyMissing, got {converted:?}"
    );
    assert!(
        converted.to_string().contains("infrastructure"),
        "converted error should preserve original message: {}",
        converted
    );
}

// =============================================================================
// AppState::from_kit() 错误路径（非 Docker 依赖）
// =============================================================================
//
// T054: R-di-007 要求"测试覆盖 AppStateExt trait 方法"。28 个 accessor 方法
// 已由 src/di/axum_state.rs 内联 3 个 tc_ 测试覆盖（Docker 依赖）。这里补充
// 非 Docker 路径：from_kit() 在 InfrastructureModule 未注册时返回错误。
//
// InfrastructureModule 依赖 DatabaseModule（需 PostgreSQL），故无法在非 Docker
// 环境构建 InfrastructureModule。但可验证 from_kit() 在缺少该模块时返回
// ModuleBuildError::DependencyMissing，覆盖 from_kit() 第一个 require 调用
// 的错误分支。

/// 空 kit（无任何模块注册）调用 from_kit 应返回错误，提及 InfrastructureModule。
#[tokio::test]
async fn from_kit_empty_kit_returns_dependency_missing_error() {
    let kit = AsyncKit::new();
    let built = kit.build().await.expect("empty kit should build");
    let result = AppState::from_kit(&built);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("from_kit should fail with empty kit"),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("infrastructure") || msg.contains("Infrastructure"),
        "expected error mentioning infrastructure module, got: {msg}"
    );
}

/// 仅注册 SettingsModule（无 Docker 依赖）调用 from_kit 应返回错误，
/// 提及 InfrastructureModule。
#[tokio::test]
async fn from_kit_only_settings_returns_dependency_missing_error() {
    let settings = Arc::new(load_settings().expect("Failed to load settings"));

    let mut kit = AsyncKit::new();
    kit.set_config(settings);
    kit.register::<SettingsModule>()
        .expect("register SettingsModule");

    let built = kit
        .build()
        .await
        .expect("kit with only SettingsModule should build");
    let result = AppState::from_kit(&built);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("from_kit should fail without InfrastructureModule"),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("infrastructure") || msg.contains("Infrastructure"),
        "expected error mentioning infrastructure module, got: {msg}"
    );
}

/// 注册全部非 Docker 模块（Settings/Http/Cache/Engine）但仍缺 InfrastructureModule
/// 时调用 from_kit 应返回错误，提及 InfrastructureModule。
#[tokio::test]
async fn from_kit_non_docker_modules_only_returns_dependency_missing_error() {
    let settings = Arc::new(load_settings().expect("Failed to load settings"));

    let mut kit = AsyncKit::new();
    kit.set_config(settings);
    kit.register::<SettingsModule>()
        .expect("register SettingsModule");
    kit.register::<HttpModule>().expect("register HttpModule");
    kit.register::<CacheModule>().expect("register CacheModule");
    kit.register::<EngineModule>()
        .expect("register EngineModule");

    let built = kit
        .build()
        .await
        .expect("kit with non-Docker modules should build");
    let result = AppState::from_kit(&built);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("from_kit should fail without InfrastructureModule"),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("infrastructure") || msg.contains("Infrastructure"),
        "expected error mentioning infrastructure module, got: {msg}"
    );
}
