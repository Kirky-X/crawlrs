// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! fluxweir 嵌入式网关端到端集成测试（变更 `fluxweir-llm-gateway`）
//!
//! 全部用例 `#[ignore]`——需真实 PostgreSQL（run-live-test.sh 起 :5499 容器，
//! 或自备 TEST_DATABASE_URL）。运行：
//! `TEST_DATABASE_URL=... cargo test --test integration_tests \
//!   --features full,test-mocks,gateway -- --ignored fluxweir_gateway_test`
//!
//! 覆盖：嵌入式起服与 readyz 探活（R-gateway-lifecycle-001）、单池共享落库
//! 与迁移历史表分离（R-gateway-lifecycle-002/005 全新库分支）、SDK 回环形态
//! 的网关侧摘要鉴权（R-admin-auth-003）。

#![cfg(feature = "gateway")]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dbnexus::sea_orm::{ConnectionTrait, Statement};
use dbnexus::{DbConfig, DbPool};

/// 测试网关监听端口（避开 8899/8900 默认面）
const TEST_GATEWAY_PORT: u16 = 18765;
/// 网关 API key（测试明文；网关侧摘要入库，经 api_key_env 注入）
const TEST_GATEWAY_KEY: &str = "fluxweir-integration-key";
const TEST_KEY_ENV: &str = "FLUXWEIR_TEST_GATEWAY_KEY";

fn live_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!("TEST_DATABASE_URL required (scripts/run-live-test.sh integration_tests)")
    })
}

async fn count_rows(pool: &DbPool, table: &str) -> i64 {
    let session = pool.get_session("admin").await.expect("session");
    let conn = session.connection().expect("conn").clone();
    let backend = conn.get_database_backend();
    let row = conn
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS n FROM {table}"),
        ))
        .await
        .expect("count query")
        .expect("row");
    row.try_get::<i64>("", "n").expect("count value")
}

fn test_settings() -> Arc<crawlrs::config::Settings> {
    let mut settings = crawlrs::config::Settings::default();
    settings.fluxweir.enabled = true;
    settings.fluxweir.mode = "embedded".to_string();
    settings.fluxweir.listen_addr = format!("127.0.0.1:{TEST_GATEWAY_PORT}");
    settings.fluxweir.trusted_proxy = false;
    settings.fluxweir.request_timeout_secs = 30;
    settings.fluxweir.rag_fallback = true;
    // 密钥经 env 注入（api_key 字段 pub(crate)，测试目标为独立 crate 不可直写）
    std::env::set_var(TEST_KEY_ENV, TEST_GATEWAY_KEY);
    settings.fluxweir.api_key_env = TEST_KEY_ENV.to_string();
    // 探针 vendor 的 key env（内联装配引用；值无意义，路径不可达即失败落库）
    std::env::set_var("FLUXWEIR_PROBE_VENDOR_KEY", "probe-key-not-used");
    Arc::new(settings)
}

/// 嵌入式网关生命周期 + 单池落库 + 迁移历史表分离（全新库分支）
#[tokio::test]
#[ignore = "requires live PostgreSQL (TEST_DATABASE_URL)"]
async fn fluxweir_gateway_embedded_lifecycle() {
    let settings = test_settings();

    // 1. 共享池先行：crawlrs 专用历史表跑迁移（run_crawlrs_migrations 显式执行）
    let pool = DbPool::with_config(DbConfig {
        url: live_db_url(),
        pool_config: dbnexus::PoolConfig {
            max_connections: 8,
            min_connections: 1,
            ..Default::default()
        },
        admin_role: "admin".to_string(),
        retry_policy: Some(dbnexus::RetryPolicy::default()),
        ..Default::default()
    })
    .await
    .expect("shared pool");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let applied = crawlrs::infrastructure::database::dbnexus_connection::run_crawlrs_migrations(
        &pool,
        true,
        Some(&manifest_dir),
    )
    .await
    .expect("crawlrs migrations via dedicated history table");
    eprintln!("crawlrs migrations applied: {applied}");
    let crawlrs_rows = count_rows(&pool, "crawlrs_migrations").await;
    assert!(
        crawlrs_rows >= 8,
        "crawlrs_migrations must carry crawlrs schema versions, got {crawlrs_rows}"
    );

    // 2. 嵌入式网关起服（单池共享：fluxweir::Database::with_pool 接收同一池；
    //    db_url 直传——fluxweir DI kit 池按此建连，共享池只覆盖 Repository 面）
    let gateway = crawlrs::bootstrap::fluxweir_gateway::init_fluxweir_gateway(
        &settings,
        Some(Arc::new(pool.clone())),
        &live_db_url(),
    )
    .await
    .expect("gateway init");
    assert!(gateway.enabled(), "gateway must be enabled");
    let base_url = gateway.base_url().expect("base_url").to_string();

    // 3. readyz 探活通过
    let http = reqwest::Client::new();
    let ready = http
        .get(format!("{base_url}/readyz"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("readyz probe");
    assert!(ready.status().is_success(), "readyz must be 2xx");

    // 4. 经网关发一次 chat 请求（无 vendor 上游 → 失败形态仍被记录；
    //    R-gateway-lifecycle-002：落库行存在即为单池接管实证）
    let before = count_rows(&pool, "request_logs").await;
    let chat = http
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(TEST_GATEWAY_KEY)
        .json(&serde_json::json!({
            "model": "integration-probe",
            "messages": [{"role": "user", "content": "probe"}]
        }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .expect("chat request reaches gateway");
    let status = chat.status();
    let body = chat.text().await.unwrap_or_default();
    eprintln!(
        "gateway chat response: {status} body={}",
        &body[..body.len().min(300)]
    );
    // 出站失败（503 熔断/5xx）即为记录路径；落库为异步批量写——轮询等待
    let mut after = before;
    for _ in 0..20 {
        after = count_rows(&pool, "request_logs").await;
        if after > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        after > before,
        "request_logs must gain a row for the gateway request (before {before}, after {after})"
    );

    // 5. 迁移历史表互不短路（R-gateway-lifecycle-005 全新库分支）：
    //    fluxweir 版本 1..=9 落默认表 dbnexus_migrations，与 crawlrs_migrations 并存
    let fluxweir_rows = count_rows(&pool, "dbnexus_migrations").await;
    assert!(
        fluxweir_rows >= 9,
        "dbnexus_migrations must carry fluxweir core versions 1..=9, got {fluxweir_rows}"
    );

    // 6. 无 key 头的请求 → 401（[auth] 摘要鉴权在网关侧生效）
    let no_key = http
        .post(format!("{base_url}/v1/chat/completions"))
        .json(&serde_json::json!({"model": "x", "messages": []}))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("unauthed request");
    assert_eq!(no_key.status().as_u16(), 401, "missing key must be 401");

    // 7. 优雅停机：flush 完成才返回（顺序钉死语义的端到端锚点）
    gateway.shutdown().await;
}
