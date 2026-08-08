// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 路由处理函数
//!
//! 从 mod.rs 拆出的 routes/health_check/version 函数实现。

// R-teams-004 / T014：teams feature 关闭时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
// R-wh-001 / T028：webhook feature 关闭时不导入 WebhookRepoImpl
#[cfg(feature = "webhook")]
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    task_handler,
};
// R-wh-001 / T028：webhook-off 时 webhook_handler 模块不编译
#[cfg(feature = "webhook")]
use crate::presentation::handlers::webhook_handler;
// R-teams-002 / T012：teams-off 时 team_handler 模块不编译
#[cfg(feature = "teams")]
use crate::presentation::handlers::team_handler;
use axum::{
    routing::{get, post},
    Extension, Json, Router,
};
use dbnexus::DbPool;
use sea_orm::ConnectionTrait;
use std::sync::Arc;
use std::time::Duration;
// R-teams-002 / T012：put 仅在 teams-on 时被使用（/v1/teams/geo-restrictions PUT）
#[cfg(feature = "teams")]
use axum::routing::put;
use serde_json::json;

/// 创建应用路由
///
/// # 返回值
///
/// 返回配置好的路由
///
/// R-teams-004 / R-wh-003：feature-off 时跳过对应路由注册。
/// `bootstrap/routes.rs::build_api_app_with_state` 是主装配入口，
/// 此函数保留用于 routes/mod.rs 单元测试。
pub fn routes() -> Router {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(version))
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route(
            "/v1/scrape/{id}/_cancel",
            post(scrape_handler::cancel_scrape),
        )
        .route("/v1/crawl", post(crawl_handler::create_crawl))
        .route("/v1/crawl/{id}", get(crawl_handler::get_crawl))
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route("/v1/crawl/{id}/_cancel", post(crawl_handler::cancel_crawl))
        .route("/v1/search", post(search_handler::search));

    // R-teams-003 / T013：extract 路由按 teams feature 分裂
    #[cfg(feature = "teams")]
    let app = app.route(
        "/v1/extract",
        post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
    );
    #[cfg(not(feature = "teams"))]
    let app = app.route("/v1/extract", post(extract_handler::extract));

    // R-wh-001 / T028：/v1/webhooks 路由按 webhook feature 分裂
    #[cfg(feature = "webhook")]
    let app = app.route(
        "/v1/webhooks",
        post(webhook_handler::create_webhook::<WebhookRepoImpl>),
    );

    // R-teams-002 / T012：/v1/teams/* 路由按 teams feature 分裂
    #[cfg(feature = "teams")]
    let app = app
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        );

    app.route("/v1/audit/logs", get(audit_handler::get_audit_logs))
        .route("/v1/audit/denied", get(audit_handler::get_denied_requests))
        .route(
            "/v1/tasks/_query",
            post(task_handler::query_tasks::<TaskRepositoryImpl>),
        )
        .route(
            "/v1/tasks/_cancel",
            post(task_handler::cancel_tasks::<TaskRepositoryImpl>),
        )
}

/// 健康检查端点（liveness probe）
///
/// 这是 Kubernetes liveness probe — 总是返回 200 OK + "healthy"，
/// 表示进程存活。不检查依赖（数据库、缓存）— 避免依赖短暂故障导致 pod 重启。
/// 如需 readiness probe（检查依赖是否就绪），应单独实现 /ready endpoint
/// 注入 AppState 检查数据库连接池、缓存可用性等。
///
/// # 返回值
///
/// 返回JSON格式的健康状态 + 版本号
pub async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// 版本信息端点
///
/// # 返回值
///
/// 返回应用版本号
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 就绪检查端点（readiness probe）
///
/// Kubernetes readiness probe — 检查关键依赖（PostgreSQL、缓存）是否就绪。
/// 与 liveness probe（`/health`）不同，此端点验证实际依赖可用性。
///
/// # 返回值
///
/// - 200 OK + `"status": "ready"` — 所有依赖就绪
/// - 503 Service Unavailable + `"status": "not_ready"` — 任一依赖不可用
///
/// # 依赖检查
///
/// - PostgreSQL: 执行 `SELECT 1`，超时 3 秒
/// - 缓存: 执行 `get("__readiness_probe__")`，超时 1 秒
pub async fn readiness_check(
    Extension(db_pool): Extension<Arc<DbPool>>,
    Extension(cache_service): Extension<Arc<dyn crate::infrastructure::oxcache::CacheService>>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    use axum::http::StatusCode;

    let mut details = serde_json::Map::new();
    let mut all_ready = true;

    // Check PostgreSQL
    let db_check = async {
        let session = db_pool
            .get_session("readiness")
            .await
            .map_err(|e| e.to_string())?;
        let conn = session.connection().map_err(|e| e.to_string())?;
        conn.execute_unprepared("SELECT 1")
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    };

    let db_status = match tokio::time::timeout(Duration::from_secs(3), db_check).await {
        Ok(Ok(())) => json!({"status": "up"}),
        Ok(Err(e)) => {
            all_ready = false;
            log::warn!("Readiness check: PostgreSQL down - {e}");
            json!({"status": "down", "error": e})
        }
        Err(_) => {
            all_ready = false;
            log::warn!("Readiness check: PostgreSQL timeout (3s)");
            json!({"status": "down", "error": "timeout"})
        }
    };
    details.insert("database".to_string(), db_status);

    // Check cache
    let cache_check = async {
        cache_service
            .get("__readiness_probe__")
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    };

    let cache_status = match tokio::time::timeout(Duration::from_secs(1), cache_check).await {
        Ok(Ok(())) => json!({"status": "up"}),
        Ok(Err(e)) => {
            all_ready = false;
            log::warn!("Readiness check: cache down - {e}");
            json!({"status": "down", "error": e})
        }
        Err(_) => {
            all_ready = false;
            log::warn!("Readiness check: cache timeout (1s)");
            json!({"status": "down", "error": "timeout"})
        }
    };
    details.insert("cache".to_string(), cache_status);

    let status = if all_ready { "ready" } else { "not_ready" };
    let status_code = if all_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(json!({
            "status": status,
            "checks": details,
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::testcontainers_fixtures as tcf;
    use crate::infrastructure::database::dbnexus_connection::create_pool;
    use crate::infrastructure::oxcache::CacheService;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tower::ServiceExt;

    /// Mock CacheService that can be configured to succeed or fail.
    struct MockCacheService {
        should_fail: AtomicBool,
    }

    impl MockCacheService {
        fn new(should_fail: bool) -> Self {
            Self {
                should_fail: AtomicBool::new(should_fail),
            }
        }
    }

    impl CacheService for MockCacheService {
        fn get(
            &self,
            _key: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
        > {
            let should_fail = self.should_fail.load(Ordering::Relaxed);
            Box::pin(async move {
                if should_fail {
                    Err(anyhow::anyhow!("mock cache error"))
                } else {
                    Ok(None)
                }
            })
        }

        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl_seconds: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }

        fn delete(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }

        fn exists(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
        {
            Box::pin(async { Ok(false) })
        }
    }

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    /// Build a test router with readiness_check handler.
    fn build_ready_router(db_pool: Arc<DbPool>, cache_service: Arc<dyn CacheService>) -> Router {
        Router::new()
            .route("/ready", get(readiness_check))
            .layer(Extension(db_pool))
            .layer(Extension(cache_service))
    }

    /// Create a DbPool from testcontainers handle.
    async fn db_pool_from_handle(handle: &tcf::DbHandle) -> Arc<DbPool> {
        let settings = tcf::database_settings(&handle.pg.url);
        let pool = create_pool(&settings)
            .await
            .expect("failed to create test DbPool");
        Arc::new(pool)
    }

    #[tokio::test]
    async fn tc_readiness_all_up_returns_200() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_readiness_all_up_returns_200");
            return;
        }
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start DB: {e}");
                return;
            }
        };
        let pool = db_pool_from_handle(&handle).await;
        let cache = Arc::new(MockCacheService::new(false));
        let app = build_ready_router(pool, cache);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ready");
        assert_eq!(json["checks"]["database"]["status"], "up");
        assert_eq!(json["checks"]["cache"]["status"], "up");
        assert!(json["version"].is_string());
    }

    #[tokio::test]
    async fn tc_readiness_cache_down_returns_503() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_readiness_cache_down_returns_503");
            return;
        }
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start DB: {e}");
                return;
            }
        };
        let pool = db_pool_from_handle(&handle).await;
        let cache = Arc::new(MockCacheService::new(true)); // fails
        let app = build_ready_router(pool, cache);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "not_ready");
        assert_eq!(json["checks"]["database"]["status"], "up");
        assert_eq!(json["checks"]["cache"]["status"], "down");
    }
}
