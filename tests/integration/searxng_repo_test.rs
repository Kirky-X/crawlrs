// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例登记簿仓储集成测试（真实 PostgreSQL）
//!
//! 覆盖：幂等 upsert、候选排序、非法状态迁移拒绝、状态计数、
//! 数据集缺席退役（seed 豁免）、探活结果回填。
//!
//! 运行：`scripts/run-live-test.sh integration_tests`（自动注入 TEST_DATABASE_URL
//! 并按序应用 `migrations/*.sql`，含 008 searxng_instances）。

use dbnexus::sea_orm::ConnectionTrait;

use crawlrs::domain::id::Id;
use crawlrs::domain::repositories::searxng_instance_repository::{
    AdmittedInstance, SearxngInstanceRepository, SearxngInstanceState, UpsertOutcome,
};
use crawlrs::infrastructure::database::repositories::searxng_instance_repo_impl::SearxngInstanceRepositoryImpl;

use super::helpers::create_test_app_no_worker;

/// SearXNG 集成测试全局串行锁
///
/// 登记簿是全进程共享表，多个测试并行 upsert/退役会互相干扰计数与排序断言，
/// 故 searxng 相关集成测试统一串行执行。
pub(crate) fn searxng_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 生成唯一前缀（并行测试隔离）
fn unique_prefix() -> String {
    format!("https://searx-{}", Id::generate())
}

/// 构造准入实例元数据
fn admitted(url: &str, uptime_day: Option<f64>, latency_ms: Option<i64>) -> AdmittedInstance {
    let _ = latency_ms;
    AdmittedInstance {
        url: url.to_string(),
        network_type: Some("normal".to_string()),
        version: Some("2024.1.1".to_string()),
        api_grade: Some("A".to_string()),
        http_grade: Some("A+".to_string()),
        tls_grade: Some("A+".to_string()),
        cdn_grade: None,
        uptime_day,
        uptime_month: None,
    }
}

/// 清理本测试写入的实例行
async fn cleanup(pool: &dbnexus::DbPool, prefix: &str) {
    let session = pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("connection");
    conn.execute_unprepared(&format!(
        "DELETE FROM searxng_instances WHERE url LIKE '{}%'",
        prefix
    ))
    .await
    .expect("cleanup searxng_instances");
}

#[tokio::test]
async fn test_upsert_from_dataset_is_idempotent() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();
    let url = format!("{prefix}.example.org");
    let payload = admitted(&url, Some(99.0), None);

    let first = repo
        .upsert_from_dataset(&payload)
        .await
        .expect("first upsert");
    assert_eq!(first, UpsertOutcome::Inserted);

    let second = repo
        .upsert_from_dataset(&payload)
        .await
        .expect("second upsert");
    assert_eq!(second, UpsertOutcome::Updated);

    let session = app.db_pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("connection");
    let rows = conn
        .query_all_raw(dbnexus::sea_orm::Statement::from_sql_and_values(
            dbnexus::sea_orm::DatabaseBackend::Postgres,
            "SELECT url, state, dataset_miss_count FROM searxng_instances WHERE url = $1",
            [url.clone().into()],
        ))
        .await
        .expect("query rows");
    assert_eq!(rows.len(), 1, "same url must map to a single row");
    let state: String = rows[0].try_get("", "state").expect("state column");
    let miss: i32 = rows[0]
        .try_get("", "dataset_miss_count")
        .expect("dataset_miss_count column");
    assert_eq!(state, "candidate");
    assert_eq!(miss, 0);

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_list_candidates_orders_by_uptime_and_excludes_retired() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();

    let low = format!("{prefix}-low.example.org");
    let high = format!("{prefix}-high.example.org");
    let retired = format!("{prefix}-retired.example.org");

    repo.upsert_from_dataset(&admitted(&low, Some(96.0), None))
        .await
        .expect("upsert low");
    repo.upsert_from_dataset(&admitted(&high, Some(99.9), None))
        .await
        .expect("upsert high");
    repo.upsert_from_dataset(&admitted(&retired, Some(99.99), None))
        .await
        .expect("upsert retired");
    repo.transition_state(&retired, SearxngInstanceState::Retired)
        .await
        .expect("candidate -> retired is legal");

    let candidates = repo.list_candidates(10_000).await.expect("list candidates");
    let ours: Vec<&str> = candidates
        .iter()
        .map(|i| i.url.as_str())
        .filter(|u| u.starts_with(&prefix))
        .collect();
    assert_eq!(
        ours,
        vec![high.as_str(), low.as_str()],
        "candidates must be ordered by uptime_day DESC and exclude retired"
    );

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_illegal_transition_is_rejected() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();
    let url = format!("{prefix}.example.org");

    repo.upsert_from_dataset(&admitted(&url, Some(99.0), None))
        .await
        .expect("upsert");

    // active -> candidate 非法（回退必须经 cooldown）
    repo.transition_state(&url, SearxngInstanceState::Active)
        .await
        .expect("candidate -> active is legal");
    let err = repo
        .transition_state(&url, SearxngInstanceState::Candidate)
        .await
        .expect_err("active -> candidate must be rejected");
    assert!(
        err.to_string()
            .contains("illegal searxng instance state transition"),
        "unexpected error: {err}"
    );

    // active -> retired 非法（必须先落 cooldown）
    repo.transition_state(&url, SearxngInstanceState::Retired)
        .await
        .expect_err("active -> retired must be rejected");

    // 不存在的 url → NotFound
    let missing = repo
        .transition_state(
            "https://searx-does-not-exist.example.org",
            SearxngInstanceState::Active,
        )
        .await
        .expect_err("missing instance must be NotFound");
    assert!(matches!(
        missing,
        crawlrs::domain::repositories::task_repository::RepositoryError::NotFound
    ));

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_count_by_state_is_accurate() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();

    let before = repo
        .count_by_state(SearxngInstanceState::Candidate)
        .await
        .expect("count before");

    repo.upsert_from_dataset(&admitted(
        &format!("{prefix}-1.example.org"),
        Some(99.0),
        None,
    ))
    .await
    .expect("upsert 1");
    repo.upsert_from_dataset(&admitted(
        &format!("{prefix}-2.example.org"),
        Some(98.0),
        None,
    ))
    .await
    .expect("upsert 2");

    let after = repo
        .count_by_state(SearxngInstanceState::Candidate)
        .await
        .expect("count after");
    assert_eq!(after, before + 2);

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_dataset_miss_retires_searx_space_but_not_seed() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();

    let dataset_url = format!("{prefix}-dataset.example.org");
    let seed_url = format!("{prefix}-seed.example.org");

    repo.upsert_from_dataset(&admitted(&dataset_url, Some(99.0), None))
        .await
        .expect("upsert dataset instance");
    repo.upsert_seed(&seed_url).await.expect("upsert seed");

    let retire_after = 2;
    let mut retired_flags = Vec::new();
    for _ in 0..(retire_after + 1) {
        retired_flags.push(
            repo.record_dataset_miss(&dataset_url, retire_after)
                .await
                .expect("record dataset miss"),
        );
        let seed_retired = repo
            .record_dataset_miss(&seed_url, retire_after)
            .await
            .expect("record seed miss");
        assert!(
            !seed_retired,
            "seed instances must never retire on dataset miss"
        );
    }
    assert_eq!(
        retired_flags,
        vec![false, false, true],
        "retire must trigger only after exceeding retire_miss_cycles"
    );

    let dataset_instance = repo
        .get_by_url(&dataset_url)
        .await
        .expect("get dataset instance")
        .expect("dataset instance exists");
    assert_eq!(dataset_instance.state, SearxngInstanceState::Retired);
    assert_eq!(dataset_instance.dataset_miss_count, retire_after + 1);

    let seed_instance = repo
        .get_by_url(&seed_url)
        .await
        .expect("get seed instance")
        .expect("seed instance exists");
    assert_ne!(seed_instance.state, SearxngInstanceState::Retired);
    assert_eq!(
        seed_instance.dataset_miss_count, 0,
        "seed instances must not accumulate dataset miss counts"
    );

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_record_probe_result_updates_counters() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();
    let url = format!("{prefix}.example.org");

    repo.upsert_from_dataset(&admitted(&url, Some(99.0), None))
        .await
        .expect("upsert");

    repo.record_probe_result(&url, false, None, Some("HTTP 429"))
        .await
        .expect("record failure 1");
    repo.record_probe_result(&url, false, None, Some("timeout"))
        .await
        .expect("record failure 2");

    let after_failures = repo.get_by_url(&url).await.expect("get").expect("exists");
    assert_eq!(after_failures.consecutive_failures, 2);
    assert_eq!(after_failures.last_error.as_deref(), Some("timeout"));
    assert!(after_failures.last_failure_at.is_some());

    repo.record_probe_result(&url, true, Some(123), None)
        .await
        .expect("record success");

    let after_success = repo.get_by_url(&url).await.expect("get").expect("exists");
    assert_eq!(after_success.consecutive_failures, 0);
    assert_eq!(after_success.latency_ms, Some(123));
    assert!(after_success.last_success_at.is_some());
    assert!(after_success.last_error.is_none());

    // 不存在的 url → NotFound（显性化，不静默吞掉）
    let err = repo
        .record_probe_result(
            "https://searx-does-not-exist.example.org",
            true,
            Some(1),
            None,
        )
        .await
        .expect_err("missing instance must be NotFound");
    assert!(matches!(
        err,
        crawlrs::domain::repositories::task_repository::RepositoryError::NotFound
    ));

    cleanup(&app.db_pool, &prefix).await;
}

#[tokio::test]
async fn test_upsert_seed_marks_source_seed_and_preserves_row() {
    let _guard = searxng_test_lock().lock().await;
    let app = create_test_app_no_worker().await;
    let repo = SearxngInstanceRepositoryImpl::new(app.db_pool.clone());
    let prefix = unique_prefix();
    let url = format!("{prefix}.example.org");

    // 先由数据集准入，再被 seed 引导：仅改来源，不新增行
    repo.upsert_from_dataset(&admitted(&url, Some(99.0), None))
        .await
        .expect("dataset upsert");
    let outcome = repo.upsert_seed(&url).await.expect("seed upsert");
    assert_eq!(outcome, UpsertOutcome::Updated);

    let instance = repo.get_by_url(&url).await.expect("get").expect("exists");
    assert_eq!(
        instance.source,
        crawlrs::domain::repositories::searxng_instance_repository::SearxngInstanceSource::Seed
    );

    // 非 https 输入被拒绝（契约：调用方必须先规范化）
    let err = repo
        .upsert_seed("not-a-normalized-url")
        .await
        .expect_err("non-https url must be rejected");
    assert!(err.to_string().contains("normalized https url"));

    cleanup(&app.db_pool, &prefix).await;
}
