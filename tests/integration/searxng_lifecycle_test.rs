// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 生命周期端到端集成测试（真实 PostgreSQL + 本地 axum mock 实例）
//!
//! 覆盖：采集登记 → 启动期探活入池（target=3）→ 搜索引擎 429 故障切换到下一实例
//! → 连续失败逐出并晋升候补 → 采集轮幂等（行数不增）→ 状态机终态断言。
//!
//! 实例 URL 使用 `https://searx-*.test` 形态（准入强制 HTTPS 且拒绝内网地址，
//! 本地 mock 无法作为实例直连目标）；`RewritingTransport` 把这些虚拟 host 重写到
//! 本地 axum mock 的对应路径 —— HTTP 层是真实 reqwest 往返，仅目标地址被改写。
//!
//! 运行：`scripts/run-live-test.sh integration_tests searxng_lifecycle`
//! （需要 `--features full,test-mocks`）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::Notify;

use crawlrs::config::SearxngSearchSettings;
use crawlrs::domain::repositories::searxng_instance_repository::{
    SearxngInstanceRepository, SearxngInstanceState,
};
use crawlrs::domain::services::searxng::{
    SearxngError, SearxngInstanceServiceImpl, SearxngTransport,
};
use crawlrs::infrastructure::database::repositories::searxng_instance_repo_impl::SearxngInstanceRepositoryImpl;
use crawlrs::search::client::searxng::SearxngSearchEngine;
use crawlrs::search::engine_trait::{SearchEngine, SearchRequest};
use crawlrs::search::types::EngineHealth;

use super::helpers::create_test_app_no_worker;
use super::searxng_repo_test::searxng_test_lock;

/// 测试实例的固定 host（按 uptime 降序即池内顺序 A → B → C）
const HOST_A: &str = "searx-a.test";
const HOST_B: &str = "searx-b.test";
const HOST_C: &str = "searx-c.test";
const HOST_D: &str = "searx-d.test";

/// mock 服务控制面
#[derive(Clone, Default)]
struct MockControl {
    failing_hosts: Arc<Mutex<HashSet<String>>>,
    dataset: Arc<Mutex<Value>>,
    search_calls: Arc<AtomicUsize>,
}

impl MockControl {
    fn set_dataset(&self, dataset: Value) {
        *self.dataset.lock().expect("dataset lock") = dataset;
    }

    fn fail_host(&self, host: &str) {
        self.failing_hosts
            .lock()
            .expect("failing hosts lock")
            .insert(host.to_string());
    }

    fn is_failing(&self, host: &str) -> bool {
        self.failing_hosts
            .lock()
            .expect("failing hosts lock")
            .contains(host)
    }

    fn search_calls(&self) -> usize {
        self.search_calls.load(Ordering::SeqCst)
    }
}

/// 启动本地 axum mock 实例服务
///
/// - `GET /dataset`：采集数据集（`control.dataset`）
/// - `GET /{host}/search`：实例搜索/探活端点（失败 host 返回 429）
async fn spawn_mock_instance() -> (String, MockControl) {
    let control = MockControl::default();

    let dataset_state = control.clone();
    let failing_state = control.clone();
    let app = Router::new()
        .route(
            "/dataset",
            get(move || {
                let state = dataset_state.clone();
                async move { Json(state.dataset.lock().expect("dataset lock").clone()) }
            }),
        )
        .route(
            "/{host}/search",
            get(
                move |axum::extract::Path(host): axum::extract::Path<String>| {
                    let state = failing_state.clone();
                    async move {
                        state.search_calls.fetch_add(1, Ordering::SeqCst);
                        if state.is_failing(&host) {
                            return (
                                StatusCode::TOO_MANY_REQUESTS,
                                Json(json!({ "error": "rate limited" })),
                            );
                        }
                        let is_probe = true; // 探活与搜索共用同一判活口径
                        let _ = is_probe;
                        (
                            StatusCode::OK,
                            Json(json!({
                                "number_of_results": 2,
                                "results": [
                                    { "url": format!("https://result.{host}/1"), "title": format!("{host} result 1"), "content": "alpha" },
                                    { "url": format!("https://result.{host}/2"), "title": format!("{host} result 2"), "content": "beta" }
                                ]
                            })),
                        )
                    }
                },
            ),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock instance");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock serve");
    });

    (format!("http://{addr}"), control)
}

/// 虚拟 https host → 本地 mock 的传输实现（真实 reqwest HTTP 往返）
struct RewritingTransport {
    client: reqwest::Client,
    base: String,
}

#[async_trait]
impl SearxngTransport for RewritingTransport {
    async fn get_json(&self, url: &str, timeout: Duration) -> Result<(u16, Value), SearxngError> {
        let parsed = url::Url::parse(url).map_err(|e| SearxngError::Parse(e.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| SearxngError::Parse(format!("missing host: {url}")))?;

        let target = if host == "dataset.test" {
            format!("{}/dataset", self.base)
        } else {
            format!(
                "{}/{host}{}?{}",
                self.base,
                parsed.path(),
                parsed.query().unwrap_or("")
            )
        };

        let response = self
            .client
            .get(&target)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    SearxngError::Timeout(timeout)
                } else {
                    SearxngError::Network(e.to_string())
                }
            })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Ok((status, Value::Null));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| SearxngError::Parse(e.to_string()))?;
        Ok((status, body))
    }
}

/// 测试数据集：4 个实例（A/B/C 高 uptime 入池，D 作候补）
fn dataset() -> Value {
    let mut instances = serde_json::Map::new();
    for (host, uptime) in [
        (HOST_A, 99.4),
        (HOST_B, 99.3),
        (HOST_C, 99.2),
        (HOST_D, 99.1),
    ] {
        instances.insert(
            format!("https://{host}"),
            json!({
                "network_type": "normal",
                "api": true,
                "uptime": { "day": uptime }
            }),
        );
    }
    json!({ "instances": Value::Object(instances) })
}

fn instance_url(host: &str) -> String {
    format!("https://{host}")
}

/// 本测试登记的实例行数（按 url 前缀过滤，隔离其它测试数据）
async fn count_test_rows(pool: &dbnexus::DbPool) -> u64 {
    use crawlrs::infrastructure::database::entities::searxng_instance;
    use dbnexus::sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

    let session = pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("connection");
    searxng_instance::Entity::find()
        .filter(searxng_instance::Column::Url.like("https://searx-%.test"))
        .count(conn)
        .await
        .expect("count query")
}

/// 读取一条实例的当前状态
async fn instance_state(repo: &SearxngInstanceRepositoryImpl, host: &str) -> SearxngInstanceState {
    repo.get_by_url(&instance_url(host))
        .await
        .expect("read instance")
        .expect("instance must exist")
        .state
}

/// 等待热池达到期望成员数（异步晋升是 spawn 的，避免假阴性）
async fn wait_for_pool(service: &SearxngInstanceServiceImpl, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if service.pool_len() == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "pool did not reach {expected} members (current {})",
            service.pool_len()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 清理本测试登记的实例行（前置 + 后置共用，失败重跑不留残留）
async fn cleanup_test_rows(pool: &dbnexus::DbPool) {
    use dbnexus::sea_orm::ConnectionTrait;
    let session = pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("connection");
    conn.execute_unprepared("DELETE FROM searxng_instances WHERE url LIKE 'https://searx-%.test'")
        .await
        .expect("cleanup searxng_instances");
}

#[tokio::test]
async fn test_full_lifecycle_collect_probe_failover_promote() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    cleanup_test_rows(&app.db_pool).await;
    let repo = Arc::new(SearxngInstanceRepositoryImpl::new(app.db_pool.clone()));

    let (base, control) = spawn_mock_instance().await;
    control.set_dataset(dataset());
    let transport = Arc::new(RewritingTransport {
        client: reqwest::Client::new(),
        base,
    });

    let mut settings = SearxngSearchSettings::default();
    settings.pool_target = 3;
    settings.collector.data_url = "https://dataset.test/data".to_string();
    settings.health.probe_query = "crawlrs".to_string();
    let service = Arc::new(SearxngInstanceServiceImpl::new(
        repo.clone(),
        transport.clone(),
        settings,
    ));

    // ---- 1. 采集：4 个实例进入登记簿（candidate）----
    let report = service.collect().await.expect("collect round");
    assert_eq!(report.fetched, 4);
    assert_eq!(report.admitted_new, 4);
    assert_eq!(report.errors, 0);
    for host in [HOST_A, HOST_B, HOST_C, HOST_D] {
        assert_eq!(
            instance_state(&repo, host).await,
            SearxngInstanceState::Candidate,
            "{host} must be registered as candidate after collect"
        );
    }
    assert_eq!(count_test_rows(&app.db_pool).await, 4);

    // ---- 2. 启动期重建：探活通过 3 个入池，D 留作候补 ----
    service.bootstrap().await;
    let snapshot = service.pool_snapshot().await;
    assert_eq!(snapshot.members.len(), 3, "pool must reach pool_target");
    assert_eq!(snapshot.health, EngineHealth::Healthy);
    for host in [HOST_A, HOST_B, HOST_C] {
        assert_eq!(
            instance_state(&repo, host).await,
            SearxngInstanceState::Active,
            "{host} must be probed into the pool"
        );
    }
    assert_eq!(
        instance_state(&repo, HOST_D).await,
        SearxngInstanceState::Candidate,
        "the 4th instance (lowest uptime) must stay a candidate"
    );

    // ---- 3. 搜索引擎：实例 A 注入 429 → 自动切换到 B 并成功 ----
    let engine = SearxngSearchEngine::new(
        service.clone(),
        transport.clone(),
        Duration::from_secs(5),
        2,
    );
    control.fail_host(HOST_A);
    let calls_before = control.search_calls();
    let response = engine
        .search(&SearchRequest::new("q").with_limit(2))
        .await
        .expect("failover search succeeds");
    assert_eq!(response.items.len(), 2);
    assert!(
        response.items[0].url.contains(HOST_B),
        "second instance must serve the results, got {:?}",
        response.items
    );
    assert_eq!(
        control.search_calls() - calls_before,
        2,
        "one failed attempt on A + one successful attempt on B"
    );
    let a_row = repo
        .get_by_url(&instance_url(HOST_A))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a_row.consecutive_failures, 1);
    assert!(a_row.last_error.is_some());

    // ---- 4. A 再失败一次 → 逐出 + cooldown，候补 D 异步晋升回池 ----
    service
        .report(
            &instance_url(HOST_A),
            crawlrs::domain::services::searxng::InstanceOutcome::Failure {
                error: SearxngError::RateLimited,
            },
        )
        .await;
    wait_for_pool(&service, 3).await;

    assert_eq!(
        instance_state(&repo, HOST_A).await,
        SearxngInstanceState::Cooldown
    );
    assert_eq!(
        instance_state(&repo, HOST_D).await,
        SearxngInstanceState::Active,
        "candidate D must be promoted into the pool"
    );
    let snapshot = service.pool_snapshot().await;
    assert_eq!(snapshot.members.len(), 3);
    assert!(
        snapshot
            .members
            .iter()
            .all(|m| m.url != instance_url(HOST_A)),
        "evicted instance must not stay in the pool"
    );

    // ---- 5. 采集轮幂等：数据集未变 → 只更新不新增，行数不增 ----
    let second = service.collect().await.expect("second collect");
    assert_eq!(second.admitted_new, 0);
    assert_eq!(second.updated, 4);
    assert_eq!(
        count_test_rows(&app.db_pool).await,
        4,
        "idempotent upsert must not create duplicate rows"
    );

    // ---- 6. 状态机终态断言 ----
    assert_eq!(
        instance_state(&repo, HOST_B).await,
        SearxngInstanceState::Active
    );
    assert_eq!(
        instance_state(&repo, HOST_C).await,
        SearxngInstanceState::Active
    );
    assert_eq!(
        instance_state(&repo, HOST_D).await,
        SearxngInstanceState::Active
    );
    assert_eq!(
        instance_state(&repo, HOST_A).await,
        SearxngInstanceState::Cooldown,
        "A must stay in cooldown after being evicted"
    );
    assert_eq!(service.health(), EngineHealth::Healthy);

    // 清理：本测试登记的实例行
    cleanup_test_rows(&app.db_pool).await;
}

/// 门控传输替身：首个调用会阻塞到测试放行，便于制造「采集进行中」的并发窗口
struct GatedTransport {
    payload: Value,
    calls: AtomicUsize,
    started: Notify,
    release: Notify,
}

impl GatedTransport {
    fn new(payload: Value) -> Self {
        Self {
            payload,
            calls: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        }
    }
}

#[async_trait]
impl SearxngTransport for GatedTransport {
    async fn get_json(&self, _url: &str, _timeout: Duration) -> Result<(u16, Value), SearxngError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.notified().await;
        Ok((200, self.payload.clone()))
    }
}

#[tokio::test]
async fn test_concurrent_collect_runs_body_once() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(SearxngInstanceRepositoryImpl::new(app.db_pool.clone()));
    let transport = Arc::new(GatedTransport::new(json!({ "instances": {} })));
    let service = Arc::new(SearxngInstanceServiceImpl::new(
        repo.clone(),
        transport.clone(),
        SearxngSearchSettings::default(),
    ));

    // 第一个 collect 持锁后阻塞在数据集拉取
    let first_service = Arc::clone(&service);
    let first = tokio::spawn(async move { first_service.collect().await });

    transport.started.notified().await;

    // 并发第二个 collect：拿不到 advisory lock → 跳过本轮，且不触达传输层
    let second = service.collect().await.expect("skip is not an error");
    assert!(second.skipped, "concurrent round must be skipped");
    assert_eq!(
        transport.calls.load(Ordering::SeqCst),
        1,
        "skipped round must not fetch the dataset again"
    );

    // 放行第一轮并收尾
    transport.release.notify_one();
    let first_report = first.await.expect("join").expect("first round succeeds");
    assert!(!first_report.skipped);

    // 锁已释放：再跑一轮会真正执行（验证 release 生效）
    transport.release.notify_one();
    let third = service.collect().await.expect("third round");
    assert!(!third.skipped);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
}
