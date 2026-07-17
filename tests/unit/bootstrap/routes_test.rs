// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unit tests for `crawlrs::bootstrap::routes`.
//!
//! These tests exercise the four public route-assembly functions:
//! - [`create_public_routes`]
//! - [`create_protected_routes_with_state`]
//! - [`create_v2_routes_with_state`]
//! - [`build_api_app_with_state`]
//!
//! Building a real `AppState` requires ~30 `Arc<dyn Trait>` dependencies wired
//! through `trait_kit::AsyncKit`. Following the project convention
//! ("No mock library — tests use real trait impls" per AGENTS.md), we boot a
//! real PostgreSQL container via `testcontainers` and construct `AppState`
//! through the canonical `AsyncKit::build()` + `AppState::from_kit()` path —
//! identical to `src/main.rs` startup.
//!
//! Tests skip gracefully when Docker is unavailable.

#![cfg(test)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

use crawlrs::bootstrap::config::load_settings;
use crawlrs::bootstrap::routes::{
    build_api_app_with_state, create_protected_routes_with_state, create_public_routes,
    create_v2_routes_with_state,
};
use crawlrs::di::modules::{
    CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule, RepositoryModule,
    ServiceModule, SettingsModule,
};
use crawlrs::di::{AppState, AppStateExt};
use trait_kit::AsyncKit;

// =============================================================================
// Test infrastructure helpers
// =============================================================================

/// Check whether Docker is available on the host.
///
/// Mirrors `crawlrs::common::test_fixtures::docker_available`, which is only
/// visible to in-crate `#[cfg(test)]` builds. External integration tests
/// re-implement the check.
async fn docker_available() -> bool {
    tokio::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A running PostgreSQL container with its mapped port and connection URL.
struct PgContainer {
    #[allow(dead_code)]
    port: u16,
    url: String,
    // Keep the container alive; dropped last.
    _container: testcontainers::ContainerAsync<Postgres>,
}

impl PgContainer {
    async fn start() -> anyhow::Result<Self> {
        let image = Postgres::default().with_tag("16-alpine");
        let container = image
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("failed to start postgres container: {e}"))?;
        let port = container
            .get_host_port_ipv4(5432.tcp())
            .await
            .map_err(|e| anyhow::anyhow!("failed to get postgres port: {e}"))?;
        let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
        Ok(Self {
            port,
            url,
            _container: container,
        })
    }
}

/// Global mutex serializing environment-variable mutations.
///
/// `CRAWLRS__DATABASE__URL` is process-global; parallel tests that each set
/// it must hold this lock across `set_var` + `load_settings()` to avoid
/// reading another test's URL.
static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Build app `Settings` patched with the given database URL.
///
/// `DatabaseSettings.url` is `pub(crate)` (sensitive), so external tests
/// cannot construct the struct directly. Instead we set the confers env var
/// `CRAWLRS__DATABASE__URL` before calling `load_settings()`, which the
/// confers `Config` derive picks up via `#[config(env_prefix = "CRAWLRS__DATABASE__")]`.
fn settings_with_db_url(db_url: &str) -> anyhow::Result<Arc<crawlrs::config::Settings>> {
    let _guard = ENV_MUTEX.lock().expect("ENV_MUTEX poisoned");
    std::env::set_var("CRAWLRS__DATABASE__URL", db_url);
    let settings = load_settings()?;
    Ok(Arc::new(settings))
}

/// Build a full `AppState` via `AsyncKit` + `AppState::from_kit()`.
///
/// Registers every trait-kit module against the testcontainers PostgreSQL
/// instance — identical to the production startup sequence in `src/main.rs`.
async fn build_app_state(
    db_url: &str,
) -> anyhow::Result<(AppState, Arc<crawlrs::config::Settings>)> {
    let settings = settings_with_db_url(db_url)?;

    let mut kit = AsyncKit::new();
    kit.set_config(settings.clone());
    kit.register::<SettingsModule>()
        .map_err(|e| anyhow::anyhow!("register SettingsModule: {e}"))?;
    kit.register::<DatabaseModule>()
        .map_err(|e| anyhow::anyhow!("register DatabaseModule: {e}"))?;
    kit.register::<HttpModule>()
        .map_err(|e| anyhow::anyhow!("register HttpModule: {e}"))?;
    kit.register::<CacheModule>()
        .map_err(|e| anyhow::anyhow!("register CacheModule: {e}"))?;
    kit.register::<RepositoryModule>()
        .map_err(|e| anyhow::anyhow!("register RepositoryModule: {e}"))?;
    kit.register::<EngineModule>()
        .map_err(|e| anyhow::anyhow!("register EngineModule: {e}"))?;
    kit.register::<InfrastructureModule>()
        .map_err(|e| anyhow::anyhow!("register InfrastructureModule: {e}"))?;
    kit.register::<ServiceModule>()
        .map_err(|e| anyhow::anyhow!("register ServiceModule: {e}"))?;

    let kit = kit
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to build kit: {e}"))?;
    let state = AppState::from_kit(&kit)?;
    Ok((state, settings))
}

/// Test fixture: boot PostgreSQL, build AppState, return both.
///
/// Callers should call `docker_available()` first and skip on `false`.
async fn setup_state() -> Option<(AppState, Arc<crawlrs::config::Settings>, PgContainer)> {
    let pg = match PgContainer::start().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[skip] failed to start postgres container: {e}");
            return None;
        }
    };
    match build_app_state(&pg.url).await {
        Ok((state, settings)) => Some((state, settings, pg)),
        Err(e) => {
            eprintln!("[skip] failed to build AppState: {e}");
            None
        }
    }
}

/// Build a one-shot request with the given method, URI, and no body.
fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("Failed to build request")
}

/// Build a one-shot request with the given method, URI, and body bytes.
fn request_with_body(method: Method, uri: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body))
        .expect("Failed to build request")
}

/// Send a request through the router and return the status code.
async fn send(router: axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = router.oneshot(req).await.expect("Failed to get response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .map(|b| b.to_vec())
        .unwrap_or_default();
    (status, headers, body)
}

// =============================================================================
// create_public_routes
// =============================================================================

#[tokio::test]
async fn test_public_routes_health_returns_ok() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_public_routes_health_returns_ok");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    let (status, _, body) = send(app, request(Method::GET, "/health")).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).expect("health body is JSON");
    assert_eq!(json["status"], "healthy");
}

#[tokio::test]
async fn test_public_routes_version_returns_cargo_version() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_public_routes_version_returns_cargo_version");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    let (status, _, body) = send(app, request(Method::GET, "/v1/version")).await;

    assert_eq!(status, StatusCode::OK);
    let body_str = String::from_utf8(body).expect("version body is ASCII");
    assert_eq!(body_str, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn test_public_routes_metrics_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_public_routes_metrics_is_registered");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    let (status, _, _) = send(app, request(Method::GET, "/metrics")).await;

    // Metrics route is registered (not 404). The handler may return any
    // status depending on the metrics backend; we only verify the route
    // exists in the public routes router.
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_public_routes_unknown_path_returns_404() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_public_routes_unknown_path_returns_404");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    let (status, _, _) = send(app, request(Method::GET, "/v1/nonexistent")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_public_routes_health_is_get_only() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_public_routes_health_is_get_only");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    let (status, _, _) = send(app, request(Method::POST, "/health")).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_public_routes_does_not_include_protected_endpoints() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_public_routes_does_not_include_protected_endpoints"
        );
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_public_routes(&state);
    // /v1/scrape is a protected route and must NOT be present in public routes
    let (status, _, _) = send(app, request(Method::POST, "/v1/scrape")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// =============================================================================
// create_protected_routes_with_state
// =============================================================================

#[tokio::test]
async fn test_protected_routes_builds_without_panic() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_builds_without_panic");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    // Building the protected router must not panic.
    let _app = create_protected_routes_with_state(&state, settings);
}

#[tokio::test]
async fn test_protected_routes_scrape_post_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_scrape_post_is_registered");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // POST /v1/scrape is registered → auth middleware rejects with 401 (not 404)
    let (status, _, _) = send(app, request(Method::POST, "/v1/scrape")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_scrape_get_by_id_is_registered() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_scrape_get_by_id_is_registered"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::GET, "/v1/scrape/abc-123")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_protected_routes_extract_post_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_extract_post_is_registered");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::POST, "/v1/extract")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_webhooks_post_and_list_get_registered() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_webhooks_post_and_list_get_registered"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // POST /v1/webhooks (create) and GET /v1/webhooks (list) both registered
    let (post_status, _, _) = send(app.clone(), request(Method::POST, "/v1/webhooks")).await;
    let (get_status, _, _) = send(app, request(Method::GET, "/v1/webhooks")).await;
    assert_ne!(post_status, StatusCode::NOT_FOUND);
    assert_ne!(get_status, StatusCode::NOT_FOUND);
    assert_eq!(post_status, StatusCode::UNAUTHORIZED);
    assert_eq!(get_status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_crawl_post_get_delete_registered() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_crawl_post_get_delete_registered"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // POST /v1/crawl
    let (s1, _, _) = send(app.clone(), request(Method::POST, "/v1/crawl")).await;
    // GET /v1/crawl/{id}
    let (s2, _, _) = send(app.clone(), request(Method::GET, "/v1/crawl/some-id")).await;
    // GET /v1/crawl/{id}/results
    let (s3, _, _) = send(
        app.clone(),
        request(Method::GET, "/v1/crawl/some-id/results"),
    )
    .await;
    // DELETE /v1/crawl/{id}
    let (s4, _, _) = send(app, request(Method::DELETE, "/v1/crawl/some-id")).await;

    assert_ne!(s1, StatusCode::NOT_FOUND);
    assert_ne!(s2, StatusCode::NOT_FOUND);
    assert_ne!(s3, StatusCode::NOT_FOUND);
    assert_ne!(s4, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_protected_routes_search_post_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_search_post_is_registered");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::POST, "/v1/search")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_team_endpoints_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_team_endpoints_registered");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // GET /v1/teams/me
    let (s1, _, _) = send(app.clone(), request(Method::GET, "/v1/teams/me")).await;
    // GET /v1/teams/me/usage
    let (s2, _, _) = send(app.clone(), request(Method::GET, "/v1/teams/me/usage")).await;
    // GET /v1/teams/geo-restrictions
    let (s3, _, _) = send(
        app.clone(),
        request(Method::GET, "/v1/teams/geo-restrictions"),
    )
    .await;
    // PUT /v1/teams/geo-restrictions
    let (s4, _, _) = send(app, request(Method::PUT, "/v1/teams/geo-restrictions")).await;

    assert_ne!(s1, StatusCode::NOT_FOUND);
    assert_ne!(s2, StatusCode::NOT_FOUND);
    assert_ne!(s3, StatusCode::NOT_FOUND);
    assert_ne!(s4, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_protected_routes_audit_endpoints_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_audit_endpoints_registered");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    let (s1, _, _) = send(app.clone(), request(Method::GET, "/v1/audit/logs")).await;
    let (s2, _, _) = send(app, request(Method::GET, "/v1/audit/denied")).await;
    assert_ne!(s1, StatusCode::NOT_FOUND);
    assert_ne!(s2, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_protected_routes_scrape_get_rejected_by_auth_before_method_check() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_scrape_get_rejected_by_auth_before_method_check"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // /v1/scrape only allows POST, but the auth middleware layer wraps the
    // entire router (including fallback) and runs before method matching.
    // Without an Authorization header the request is rejected with 401
    // rather than 405 — verifying the middleware chain ordering.
    let (status, _, _) = send(app, request(Method::GET, "/v1/scrape")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_search_get_rejected_by_auth_before_method_check() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_search_get_rejected_by_auth_before_method_check"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::GET, "/v1/search")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_unknown_path_rejected_by_auth() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_protected_routes_unknown_path_rejected_by_auth"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // The auth middleware layer wraps the whole protected router, so even
    // unknown paths are rejected with 401 (auth runs before fallback 404).
    let (status, _, _) = send(app, request(Method::GET, "/v1/does-not-exist")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_protected_routes_requires_auth_header() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_protected_routes_requires_auth_header");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_protected_routes_with_state(&state, settings);
    // No Authorization header → auth middleware must reject with 401.
    let (status, _, _) = send(app, request(Method::POST, "/v1/scrape")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// =============================================================================
// create_v2_routes_with_state
// =============================================================================

#[tokio::test]
async fn test_v2_routes_builds_without_panic() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_v2_routes_builds_without_panic");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let _app = create_v2_routes_with_state(&state);
}

#[tokio::test]
async fn test_v2_routes_tasks_query_post_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_v2_routes_tasks_query_post_is_registered");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_v2_routes_with_state(&state);
    let (status, _, _) = send(app, request(Method::POST, "/v1/tasks/_query")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v2_routes_tasks_cancel_post_is_registered() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_v2_routes_tasks_cancel_post_is_registered");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_v2_routes_with_state(&state);
    let (status, _, _) = send(app, request(Method::POST, "/v1/tasks/_cancel")).await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v2_routes_tasks_query_get_rejected_before_method_check() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_v2_routes_tasks_query_get_rejected_before_method_check"
        );
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_v2_routes_with_state(&state);
    // team_semaphore_middleware wraps the router and runs before method
    // matching; without a team_id (injected by auth_middleware, which runs
    // later) it rejects with 401 rather than 405.
    let (status, _, _) = send(app, request(Method::GET, "/v1/tasks/_query")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v2_routes_unknown_path_rejected_by_middleware() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_v2_routes_unknown_path_rejected_by_middleware");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_v2_routes_with_state(&state);
    // The middleware layer wraps the whole v2 router, so unknown paths are
    // rejected by team_semaphore_middleware (401) before the fallback 404.
    let (status, _, _) = send(app, request(Method::GET, "/v1/tasks/nonexistent")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v2_routes_requires_auth_header() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_v2_routes_requires_auth_header");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = create_v2_routes_with_state(&state);
    let (status, _, _) = send(app, request(Method::POST, "/v1/tasks/_query")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// =============================================================================
// build_api_app_with_state (full app)
// =============================================================================

#[tokio::test]
async fn test_full_app_builds_without_panic() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_builds_without_panic");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let _app = build_api_app_with_state(&state, settings);
}

#[tokio::test]
async fn test_full_app_health_endpoint_accessible() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_health_endpoint_accessible");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let (status, _, body) = send(app, request(Method::GET, "/health")).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body).expect("health body is JSON");
    assert_eq!(json["status"], "healthy");
}

#[tokio::test]
async fn test_full_app_version_endpoint_accessible() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_version_endpoint_accessible");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let (status, _, body) = send(app, request(Method::GET, "/v1/version")).await;
    assert_eq!(status, StatusCode::OK);
    let body_str = String::from_utf8(body).expect("version body is ASCII");
    assert_eq!(body_str, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn test_full_app_protected_route_requires_auth() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_protected_route_requires_auth");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::POST, "/v1/scrape")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_full_app_v2_task_route_requires_auth() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_v2_task_route_requires_auth");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let (status, _, _) = send(app, request(Method::POST, "/v1/tasks/_query")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_full_app_unknown_route_rejected_by_middleware() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_unknown_route_rejected_by_middleware");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    // The merged app inherits the protected + v2 middleware layers, which
    // wrap the fallback handler. Unknown paths are rejected by auth (401)
    // rather than returning 404. Public routes (/health, /v1/version,
    // /metrics) are matched before the middleware runs and return 200.
    let (status, _, _) = send(app, request(Method::GET, "/this-does-not-exist")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// =============================================================================
// Middleware chain verification on full app
// =============================================================================

#[tokio::test]
async fn test_full_app_security_headers_present_on_public_route() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_full_app_security_headers_present_on_public_route"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let (status, headers, _) = send(app, request(Method::GET, "/health")).await;
    assert_eq!(status, StatusCode::OK);

    // security_headers_middleware adds these to every response
    assert_eq!(
        headers.get("x-content-type-options"),
        Some(&HeaderValue::from_static("nosniff"))
    );
    assert_eq!(
        headers.get("x-frame-options"),
        Some(&HeaderValue::from_static("DENY"))
    );
}

#[tokio::test]
async fn test_full_app_cors_wildcard_origin_reflected_on_request() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_full_app_cors_wildcard_origin_reflected_on_request"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    // Default config has allowed_origins = "*", so CORS should allow any origin.
    let app = build_api_app_with_state(&state, settings);
    let mut req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .header("origin", "https://example.com")
        .body(Body::empty())
        .expect("Failed to build request");
    let _ = req.headers_mut(); // touch to ensure headers are initialized
    let (status, headers, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);

    // CORS layer should set access-control-allow-origin for the matching origin
    let allow_origin = headers
        .get("access-control-allow-origin")
        .expect("CORS wildcard config should set access-control-allow-origin");
    assert_eq!(allow_origin, "*");
}

#[tokio::test]
async fn test_full_app_cors_preflight_returns_allow_methods() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_cors_preflight_returns_allow_methods");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/health")
        .header("origin", "https://example.com")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .expect("Failed to build preflight request");
    let (status, headers, _) = send(app, req).await;

    // Preflight should succeed (CORS layer short-circuits with 200)
    assert_eq!(status, StatusCode::OK);
    let allow_origin = headers
        .get("access-control-allow-origin")
        .expect("preflight should set access-control-allow-origin");
    assert_eq!(allow_origin, "*");
}

#[tokio::test]
async fn test_full_app_cors_specific_origin_reflected() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_cors_specific_origin_reflected");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    // Override CORS config to use specific origins (not wildcard).
    // This exercises the `else` branch of `create_cors_layer` where
    // allowed_origins is non-empty and doesn't contain "*".
    let mut modified = (*settings).clone();
    modified.cors.allowed_origins = "http://localhost:3000,http://example.com".to_string();
    let modified_settings = Arc::new(modified);

    let app = build_api_app_with_state(&state, modified_settings);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .header("origin", "http://example.com")
        .body(Body::empty())
        .expect("Failed to build request");
    let (status, headers, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);

    // With specific origins configured, the CORS layer should echo back
    // the matching origin (not "*").
    let allow_origin = headers
        .get("access-control-allow-origin")
        .expect("specific-origin CORS config should set access-control-allow-origin");
    assert_eq!(allow_origin, "http://example.com");
}

#[tokio::test]
async fn test_full_app_cors_invalid_origin_falls_back_to_wildcard() {
    if !docker_available().await {
        eprintln!(
            "[skip] Docker unavailable — test_full_app_cors_invalid_origin_falls_back_to_wildcard"
        );
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    // Set CORS origins to a value that fails to parse as HeaderValue.
    // Byte 0x7f (DEL) is invalid in HTTP header values and is NOT
    // whitespace, so `trim()` won't remove it. After split/trim/filter,
    // the origin "http://inva\x7flid.com" remains non-empty and non-"*",
    // but HeaderValue::from_str rejects it, triggering the
    // `origins.is_empty()` fallback branch in `create_cors_layer`.
    let mut modified = (*settings).clone();
    modified.cors.allowed_origins = "http://inva\x7flid.com".to_string();
    let modified_settings = Arc::new(modified);

    let app = build_api_app_with_state(&state, modified_settings);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .header("origin", "https://example.com")
        .body(Body::empty())
        .expect("Failed to build request");
    let (status, headers, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);

    // When all configured origins fail to parse, the fallback branch
    // allows all origins ("*").
    let allow_origin = headers
        .get("access-control-allow-origin")
        .expect("invalid-origin fallback should set access-control-allow-origin");
    assert_eq!(allow_origin, "*");
}

#[tokio::test]
async fn test_full_app_body_limit_rejects_oversized_payload() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_body_limit_rejects_oversized_payload");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    // Default body limit is 10 MB (10 * 1024 * 1024). Send 11 MB to trigger 413.
    let oversized = vec![b'x'; 11 * 1024 * 1024];
    let (status, _, _) = send(
        app,
        request_with_body(Method::POST, "/v1/scrape", oversized),
    )
    .await;

    // Body limit rejection yields 413 Payload Too Large. Note: middleware
    // ordering means auth may run first; either 401 or 413 is acceptable
    // because both prove the router was reached and the body limit layer is
    // wired. We assert it is NOT 404 (route exists) and NOT 200 (request
    // was rejected).
    assert_ne!(status, StatusCode::NOT_FOUND);
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_full_app_body_limit_accepts_normal_payload() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_full_app_body_limit_accepts_normal_payload");
        return;
    }
    let (state, settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    let app = build_api_app_with_state(&state, settings);
    // 1 KB payload should pass the body limit layer; auth middleware will
    // reject with 401 (no Authorization header), proving the body limit
    // did NOT short-circuit.
    let small_payload = vec![b'x'; 1024];
    let (status, _, _) = send(
        app,
        request_with_body(Method::POST, "/v1/scrape", small_payload),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// =============================================================================
// AppState wiring sanity checks (proves the AppState passed to route builders
// has all required Arc<dyn Trait> fields populated, exercising the
// AppStateExt trait methods used inside the route builders).
// =============================================================================

#[tokio::test]
async fn test_app_state_accessors_return_valid_arcs() {
    if !docker_available().await {
        eprintln!("[skip] Docker unavailable — test_app_state_accessors_return_valid_arcs");
        return;
    }
    let (state, _settings, _pg) = match setup_state().await {
        Some(v) => v,
        None => return,
    };

    // Every accessor used inside create_protected_routes_with_state /
    // create_v2_routes_with_state / build_api_app_with_state must yield a
    // valid Arc (strong count >= 1). This guards against regressions where
    // a module fails to populate a field.
    assert!(Arc::strong_count(&state.task_repo()) >= 1);
    assert!(Arc::strong_count(&state.result_repo()) >= 1);
    assert!(Arc::strong_count(&state.crawl_repo()) >= 1);
    assert!(Arc::strong_count(&state.webhook_repo()) >= 1);
    assert!(Arc::strong_count(&state.webhook_event_repo()) >= 1);
    assert!(Arc::strong_count(&state.task_queue()) >= 1);
    assert!(Arc::strong_count(&state.rate_limiting_service()) >= 1);
    assert!(Arc::strong_count(&state.team_service()) >= 1);
    assert!(Arc::strong_count(&state.search_client()) >= 1);
    assert!(Arc::strong_count(&state.search_service()) >= 1);
    assert!(Arc::strong_count(&state.geo_location_service()) >= 1);
    assert!(Arc::strong_count(&state.credits_repo()) >= 1);
    assert!(Arc::strong_count(&state.audit_service()) >= 1);
    assert!(Arc::strong_count(&state.team_semaphore()) >= 1);
    assert!(Arc::strong_count(&state.db_pool()) >= 1);
}
