// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! WebSocket task status endpoint tests (spec: ws-task-status).
//!
//! R-ws-task-status-001: `GET /v1/tasks/{id}/ws` upgrades (101) and pushes a
//! status frame per matching event from the existing `event_bus`.
//! R-ws-task-status-002: unauthenticated connections are rejected and
//! cross-tenant task ids are not subscribable.
//!
//! Tests are docker/db-gated: without a test database they return early.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use axum::Extension;
use axum_test::{TestServer, TestServerConfig, Transport};
use serde_json::json;

use crawlrs::domain::auth::ApiKeyScope;
use crawlrs::domain::id::Id;
use crawlrs::domain::models::task_domain::TaskType;
use crawlrs::domain::models::task_model::Task;
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::infrastructure::events::event_bus::{DomainEvent, EventBus};
use crawlrs::infrastructure::events::BroadcastEventBus;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::presentation::middleware::auth_middleware::AuthState;
use crawlrs::presentation::routes::task::{task_routes, TaskWsGuard};
use dbnexus::sea_orm::ConnectionTrait;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;

/// 共享测试数据库：env 优先（TEST_DATABASE_URL/DATABASE_URL），否则
/// testcontainers 起 PostgreSQL 16 并按文件名序应用 migrations/postgres/。
static DB_URL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

async fn docker_available() -> bool {
    tokio::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

fn db_url() -> Option<&'static String> {
    DB_URL
        .get_or_init(|| {
            if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
                return Some(url);
            }
            if let Ok(url) = std::env::var("DATABASE_URL") {
                return Some(url);
            }
            std::thread::scope(|s| {
                let handle = s.spawn(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("runtime for fixture init");
                    rt.block_on(async {
                        if !docker_available().await {
                            eprintln!("[skip] Docker unavailable");
                            return None;
                        }
                        let image = testcontainers_modules::postgres::Postgres::default()
                            .with_tag("16-alpine");
                        let container = image.start().await.expect("start postgres container");
                        let host = container.get_host().await.expect("container host");
                        let port = container
                            .get_host_port_ipv4(5432)
                            .await
                            .expect("container port");
                        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
                        // 容器钉住进程生命周期：ContainerAsync drop = 停容器，
                        // 池要在整个测试进程里复用这条连接。
                        std::mem::forget(container);

                        // 建表：migrations/postgres/ 按文件名序执行（幂等 DDL）。
                        let conn = dbnexus::sea_orm::Database::connect(&url)
                            .await
                            .expect("connect for migrations");
                        let mut files: Vec<_> = std::fs::read_dir(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/migrations/postgres"
                        ))
                        .expect("read migrations dir")
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "sql"))
                        .collect();
                        files.sort();
                        for file in files {
                            let sql = std::fs::read_to_string(&file).expect("read migration");
                            conn.execute_unprepared(&sql).await.unwrap_or_else(|e| {
                                panic!("apply migration {}: {e}", file.display())
                            });
                        }
                        Some(url)
                    })
                });
                handle.join().expect("fixture init thread panicked")
            })
        })
        .as_ref()
}

async fn build_pool(url: &str) -> Arc<dbnexus::DbPool> {
    dbnexus::DbPool::with_config(dbnexus::DbConfig {
        url: url.to_string(),
        pool_config: dbnexus::PoolConfig {
            max_connections: 4,
            min_connections: 0,
            acquire_timeout: 30_000,
            ..dbnexus::PoolConfig::default()
        },
        ..Default::default()
    })
    .await
    .expect("build DbPool")
    .into()
}

/// ws 升级需要真实 HTTP transport（默认 mock 不支持）。
fn ws_server(app: axum::Router) -> TestServer {
    // ws 升级需要真实 HTTP transport（默认 MockHttp 不支持 WebSocket）。
    let mut config = TestServerConfig::new();
    config.transport = Some(Transport::HttpRandomPort);
    config.build(app)
}

/// DB-backed fixture: one team owns the task; a second team exists to prove
/// cross-tenant refusal. The event bus instance is shared across both
/// routers, mirroring the production singleton semantics.
struct WsFixture {
    owner: TestServer,
    intruder: TestServer,
    unauth: TestServer,
    bus: Arc<BroadcastEventBus>,
    repo: Arc<TaskRepositoryImpl>,
    pool: Arc<dbnexus::DbPool>,
    api_key_id: Id,
    task_id: Id,
    team_id: Id,
}

async fn make_fixture() -> Option<WsFixture> {
    let Some(url) = db_url() else {
        eprintln!("[skip] no test database (set TEST_DATABASE_URL or start Docker)");
        return None;
    };
    let pool = build_pool(url).await;

    let team_id = Id::generate();
    let other_team_id = Id::generate();
    let api_key_id = Id::generate();
    let session = pool.get_session("admin").await.expect("db session");
    let conn = session.connection().expect("db connection");
    for (id, name) in [
        (team_id.to_uuid_v8().to_string(), "ws-owner-team"),
        (other_team_id.to_uuid_v8().to_string(), "ws-intruder-team"),
    ] {
        conn.execute_unprepared(&format!(
            "INSERT INTO teams (id, name) VALUES ('{id}', '{name}') ON CONFLICT (id) DO NOTHING"
        ))
        .await
        .expect("insert team");
    }

    let repo = Arc::new(TaskRepositoryImpl::new(
        pool.clone(),
        chrono::Duration::seconds(30),
    ));
    let task = Task::new(
        Id::generate(),
        TaskType::Scrape,
        team_id,
        api_key_id,
        "https://ws-test.example.com".to_string(),
        json!({}),
    );
    repo.create(&task).await.expect("create task");

    let bus = Arc::new(BroadcastEventBus::new(64));

    let owner_auth = AuthState::new(pool.clone(), team_id, api_key_id, ApiKeyScope::default());
    let intruder_auth = AuthState::new(
        pool.clone(),
        other_team_id,
        Id::generate(),
        ApiKeyScope::default(),
    );

    let owner = ws_server(
        task_routes()
            .layer(Extension(owner_auth))
            .layer(Extension(repo.clone()))
            .layer(Extension(Arc::clone(&bus) as Arc<dyn EventBus>)),
    );
    let intruder = ws_server(
        task_routes()
            .layer(Extension(intruder_auth))
            .layer(Extension(repo.clone()))
            .layer(Extension(Arc::clone(&bus) as Arc<dyn EventBus>)),
    );
    // 无 AuthState 扩展的裸 router（bus 非凭据，照常提供）：
    // 未携带凭据的连接应被 401 拒绝。
    let unauth = ws_server(
        task_routes()
            .layer(Extension(Arc::clone(&repo)))
            .layer(Extension(Arc::clone(&bus) as Arc<dyn EventBus>)),
    );

    Some(WsFixture {
        owner,
        intruder,
        unauth,
        bus,
        repo,
        pool,
        api_key_id,
        task_id: task.id,
        team_id,
    })
}

/// R-ws-task-status-001：101 升级后，同一 team 的任务状态事件以帧送达。
#[tokio::test]
async fn ws_upgrade_receives_task_status_frame() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };

    let mut ws = fx
        .owner
        .get_websocket(&format!("/v1/tasks/{}/ws", fx.task_id))
        .await
        .into_websocket()
        .await;

    fx.bus
        .publish(DomainEvent::TaskCompleted {
            task_id: fx.task_id,
            team_id: fx.team_id,
        })
        .expect("publish");

    // 第一帧：连接确认（subscribed），随后才是状态帧。
    let first = tokio::time::timeout(Duration::from_secs(5), ws.receive_text())
        .await
        .expect("receive within timeout (first frame)");
    let first: serde_json::Value = serde_json::from_str(&first).expect("frames are JSON");
    assert_eq!(
        first["event"], "subscribed",
        "connection ack frame expected first, got: {first}"
    );

    let frame = tokio::time::timeout(Duration::from_secs(5), ws.receive_text())
        .await
        .expect("receive within timeout (status frame)");
    let frame: serde_json::Value = serde_json::from_str(&frame).expect("frames are JSON");
    assert_eq!(frame["event"], "task_completed", "got: {frame}");
    assert_eq!(
        frame["task_id"],
        fx.task_id.to_string(),
        "frame must carry the task id: {frame}"
    );
}

/// R-ws-task-status-001：不存在的 task id 返回明确错误（404）而非挂起。
#[tokio::test]
async fn ws_missing_task_returns_404() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };
    let missing = Id::generate();
    let res = fx.owner.get(&format!("/v1/tasks/{missing}/ws")).await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
}

/// R-ws-task-status-002：未携带凭据（无 AuthState 扩展）的连接被拒（401，非 101）。
#[tokio::test]
async fn ws_unauthenticated_connection_rejected() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };
    let res = fx.unauth.get(&format!("/v1/tasks/{}/ws", fx.task_id)).await;
    assert_eq!(res.status_code(), StatusCode::UNAUTHORIZED);
}

/// R-ws-task-status-002：跨租户 task id 不可订阅——team B 对 team A 的任务
/// 发起连接被 404 拒绝（不泄露存在性），而同队连接能建立并收到事件。
#[tokio::test]
async fn ws_cross_tenant_task_not_subscribable() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };

    // 对照：同 team 连接建立成功（101）。
    let mut own = fx
        .owner
        .get_websocket(&format!("/v1/tasks/{}/ws", fx.task_id))
        .await
        .into_websocket()
        .await;

    // 跨租户：team B 的连接被 404 拒绝。
    let res = fx
        .intruder
        .get(&format!("/v1/tasks/{}/ws", fx.task_id))
        .await;
    assert_eq!(res.status_code(), StatusCode::NOT_FOUND);

    fx.bus
        .publish(DomainEvent::TaskFailed {
            task_id: fx.task_id,
            team_id: fx.team_id,
            error: "boom".to_string(),
        })
        .expect("publish");

    // 对照连接收到状态帧，证明事件链路真实生效（拒绝是过滤而非静默）。
    let ack = tokio::time::timeout(Duration::from_secs(5), own.receive_text())
        .await
        .expect("receive within timeout (ack frame)");
    let ack: serde_json::Value = serde_json::from_str(&ack).expect("JSON");
    assert_eq!(ack["event"], "subscribed", "ack first, got: {ack}");
    let frame = tokio::time::timeout(Duration::from_secs(5), own.receive_text())
        .await
        .expect("receive within timeout (status frame)");
    assert!(
        frame.contains("task_failed"),
        "own-team connection must receive the event: {frame}"
    );
}

/// R-ws-task-status-003：连接数超限的新连接被拒（429），且名额真实占用。
#[tokio::test]
async fn ws_connection_cap_rejects_over_limit() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };

    // max_connections=1 的守卫：首连接占满后，第二个连接被 429 拒绝。
    let capped = ws_server(
        task_routes()
            .layer(Extension(AuthState::new(
                fx.pool.clone(),
                fx.team_id,
                fx.api_key_id,
                ApiKeyScope::default(),
            )))
            .layer(Extension(fx.repo.clone()))
            .layer(Extension(Arc::clone(&fx.bus) as Arc<dyn EventBus>))
            .layer(Extension(Arc::new(TaskWsGuard::new(
                1,
                Duration::from_secs(60),
            )))),
    );

    // 占住唯一名额。
    let probe = capped
        .get_websocket(&format!("/v1/tasks/{}/ws", fx.task_id))
        .await;
    let _held = probe.into_websocket().await;

    let res = capped.get(&format!("/v1/tasks/{}/ws", fx.task_id)).await;
    assert_eq!(res.status_code(), StatusCode::TOO_MANY_REQUESTS);
}

/// R-ws-task-status-003：空闲超时后服务端关闭连接并释放名额——
/// 同一请求从 429（占用中）变为 426（可获名额但非升级请求）。
#[tokio::test]
async fn ws_idle_timeout_closes_and_frees_slot() {
    let Some(fx) = make_fixture().await else {
        eprintln!("[skip] no test database");
        return;
    };

    let capped = ws_server(
        task_routes()
            .layer(Extension(AuthState::new(
                fx.pool.clone(),
                fx.team_id,
                fx.api_key_id,
                ApiKeyScope::default(),
            )))
            .layer(Extension(fx.repo.clone()))
            .layer(Extension(Arc::clone(&fx.bus) as Arc<dyn EventBus>))
            .layer(Extension(Arc::new(TaskWsGuard::new(
                1,
                Duration::from_secs(1),
            )))),
    );

    let ws = capped
        .get_websocket(&format!("/v1/tasks/{}/ws", fx.task_id))
        .await
        .into_websocket()
        .await;

    let res = capped.get(&format!("/v1/tasks/{}/ws", fx.task_id)).await;
    assert_eq!(
        res.status_code(),
        StatusCode::TOO_MANY_REQUESTS,
        "slot must be held while the connection is live"
    );

    // 不发布任何事件 → 1s 空闲超时后服务端关闭连接并释放名额。
    tokio::time::sleep(Duration::from_secs(2)).await;

    let res = capped.get(&format!("/v1/tasks/{}/ws", fx.task_id)).await;
    assert_eq!(
        res.status_code(),
        StatusCode::UPGRADE_REQUIRED,
        "after idle timeout the slot must be freed (426 = acquired then non-upgrade)"
    );
    drop(ws);
}
