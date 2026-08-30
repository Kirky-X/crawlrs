// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! BDD 验收套件支撑层：World 定义 + 完整启动链 + 通用步骤。
//!
//! 启动链（进程内一次，garrison 单例约束）：
//! testcontainers PG（migrations 全量）→ env 注入 → Settings → DI kit →
//! CrawlRsState → garrison init → bootstrap admin key → build_api_app_with_state
//! → axum-test TestServer → spawn_common_workers。
//!
//! 数据隔离：场景间用 UUID 唯一资源，不依赖全局清理（与既有集成测试约定一致）。

use std::sync::{Arc, OnceLock};

use testcontainers_modules::postgres::Postgres;

use axum_test::TestServer;
use cucumber::{given, then, when};
use serde_json::Value as Json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 共享 Harness（进程级单次启动）
// ---------------------------------------------------------------------------

pub struct SharedHarness {
    pub server: TestServer,
    /// bootstrap 签发的 admin key（`crawlrs:admin` 全权限，`<key_id>.<secret>` 形态）
    pub admin_key: String,
}

static HARNESS: tokio::sync::OnceCell<Arc<SharedHarness>> = tokio::sync::OnceCell::const_new();

pub async fn harness() -> Arc<SharedHarness> {
    HARNESS
        .get_or_init(|| async {
        Arc::new(SharedHarness::bootstrap().await.expect("bootstrap harness"))
    })
        .await
        .clone()
}

const TEST_JWT_SECRET: &str = "acceptance-test-strong-secret-key-for-garrison-32-bytes!!";

impl SharedHarness {
    async fn bootstrap() -> anyhow::Result<Self> {
        use trait_kit::AsyncKit;

        // 0. 进程级 env：本测试二进制独立进程，不存在与其他测试的并行冲突。
        // SSRF 关闭使 wiremock（127.0.0.1）可作 mock 目标站（specs/acceptance-testing
        // Constraints：零外部网络依赖）。
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");
        std::env::set_var("CRAWLRS__AUTH__JWT_SECRET", TEST_JWT_SECRET);

        // 1. 数据库 URL：env 优先，否则 testcontainers PG + migrations
        let db_url = resolve_acceptance_db_url().ok_or_else(|| {
            anyhow::anyhow!("no test database: set TEST_DATABASE_URL or start Docker")
        })?;
        std::env::set_var("CRAWLRS__DATABASE__URL", &db_url);

        // 2. Settings
        let settings = Arc::new(crawlrs::bootstrap::config::load_settings()?);

        // 3. DI kit 组装（对齐 main.rs 的 8 模块注册）
        let mut kit = AsyncKit::new();
        kit.set_config(settings.clone());
        kit.register::<crawlrs::di::modules::SettingsModule>()
            .map_err(|e| anyhow::anyhow!("register SettingsModule: {e}"))?;
        kit.register::<crawlrs::di::modules::DatabaseModule>()
            .map_err(|e| anyhow::anyhow!("register DatabaseModule: {e}"))?;
        kit.register::<crawlrs::di::modules::HttpModule>()
            .map_err(|e| anyhow::anyhow!("register HttpModule: {e}"))?;
        kit.register::<crawlrs::di::modules::CacheModule>()
            .map_err(|e| anyhow::anyhow!("register CacheModule: {e}"))?;
        kit.register::<crawlrs::di::modules::RepositoryModule>()
            .map_err(|e| anyhow::anyhow!("register RepositoryModule: {e}"))?;
        kit.register::<crawlrs::di::modules::EngineModule>()
            .map_err(|e| anyhow::anyhow!("register EngineModule: {e}"))?;
        kit.register::<crawlrs::di::modules::InfrastructureModule>()
            .map_err(|e| anyhow::anyhow!("register InfrastructureModule: {e}"))?;
        kit.register::<crawlrs::di::modules::ServiceModule>()
            .map_err(|e| anyhow::anyhow!("register ServiceModule: {e}"))?;
        let kit = kit
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("build AsyncKit: {e}"))?;
        let app_state = crawlrs::di::CrawlRsState::from_kit(&kit)?;

        // 4. garrison 单例初始化已由 ServiceModule 内部完成（重复调用会触发
        // "global DAO already injected"），此处只做 bootstrap 数据准备。
        // 5. bootstrap：default team + admin key（对齐 main.rs run_bootstrap 语义）
        let admin_key = bootstrap_admin_key(app_state.db_pool.clone()).await?;

        // 6. app + TestServer（内存 HTTP，不占端口）
        let app =
            crawlrs::bootstrap::routes::build_api_app_with_state(&app_state, settings.clone());
        let server = TestServer::new(app);

        // 7. workers 真实运行（scrape/crawl 任务由 DB 队列驱动消费）
        let _worker_handles =
            crawlrs::bootstrap::workers::spawn_common_workers(&app_state, &settings, true).await;

        Ok(Self { server, admin_key })
    }
}

/// 创建 default team 并经 garrison 签发 admin key（含 api_keys 映射）。
/// 与 `tests/integration/auth_garrison_test.rs::setup_garrison_env` 同模式。
async fn bootstrap_admin_key(pool: Arc<dbnexus::DbPool>) -> anyhow::Result<String> {
    use crawlrs::common::constants::default_identity::DEFAULT_TEAM_ID;
    use crawlrs::common::time_utils;
    use crawlrs::infrastructure::database::entities::api_key::ActiveModel as ApiKeyActiveModel;
    use crawlrs::infrastructure::database::entities::api_key::Entity as ApiKeyEntity;
    use crawlrs::infrastructure::database::entities::team::ActiveModel as TeamActiveModel;
    use crawlrs::infrastructure::database::entities::team::Entity as TeamEntity;
    use sea_orm::{ActiveValue, EntityTrait};
    use uuid::Uuid;

    let team_id = DEFAULT_TEAM_ID;
    let session = pool.get_session("admin").await?;
    let conn = session.connection()?;

    if TeamEntity::find_by_id(team_id).one(conn).await?.is_none() {
        let now = time_utils::to_db_datetime(chrono::Utc::now());
        TeamEntity::insert(TeamActiveModel {
            id: ActiveValue::Set(team_id),
            name: ActiveValue::Set("default-team".to_string()),
            allowed_countries: ActiveValue::Set(None),
            blocked_countries: ActiveValue::Set(None),
            ip_whitelist: ActiveValue::Set(None),
            domain_blacklist: ActiveValue::Set(None),
            enable_geo_restrictions: ActiveValue::Set(false),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        })
        .exec(conn)
        .await?;
    }

    #[cfg(feature = "auth")]
    {
        use crawlrs::infrastructure::auth::get_garrison_dao;
        use garrison::protocol::apikey::ApiKeyHandler;

        let dao = get_garrison_dao()
            .ok_or_else(|| anyhow::anyhow!("garrison DAO not initialized"))?;
        let handler = ApiKeyHandler::new(dao);
        let api_key_id = Uuid::new_v4();
        let plaintext_key = handler
            .generate_with_namespace(
                api_key_id.to_string(),
                "crawlrs",
                vec![
                    "crawlrs:read".to_string(),
                    "crawlrs:write".to_string(),
                    "crawlrs:admin".to_string(),
                ],
                30 * 24 * 60 * 60,
            )
            .await?;

        let garrison_key_id = plaintext_key
            .split_once('.')
            .map(|(k, _)| k.to_string())
            .ok_or_else(|| anyhow::anyhow!("garrison key malformed (no '.')"))?;

        let now = time_utils::to_db_datetime(chrono::Utc::now());
        #[allow(deprecated)]
        ApiKeyEntity::insert(ApiKeyActiveModel {
            id: ActiveValue::Set(api_key_id),
            team_id: ActiveValue::Set(team_id),
            key: ActiveValue::Set(garrison_key_id),
            key_hash: ActiveValue::Set(None),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(None),
        })
        .exec(conn)
        .await?;

        return Ok(plaintext_key);
    }

    #[cfg(not(feature = "auth"))]
    anyhow::bail!("acceptance suite requires the `auth` feature (platform implies auth)");
}

// ---------------------------------------------------------------------------
// World
// ---------------------------------------------------------------------------

#[derive(cucumber::World, Default)]
#[world(init = Self::new)]
pub struct AcceptanceWorld {
    /// 上一请求的响应
    pub last_response: Option<axum_test::TestResponse>,
    /// 请求用 Authorization 头（None = 不带认证）
    pub auth_key: Option<String>,
    /// 场景内共享上下文（task id 等）
    pub ctx: std::collections::HashMap<String, String>,
    /// response 被消费后的状态码遗留（sign 步骤等，供 then_status 断言）
    pub last_status: Option<u16>,
    /// response 被消费后的响应体遗留
    pub last_body: Option<String>,
}

/// 手写 Debug：TestResponse / Harness 无 Debug，仅输出结构性字段。
impl std::fmt::Debug for AcceptanceWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcceptanceWorld")
            .field("has_response", &self.last_response.is_some())
            .field("authed", &self.auth_key.is_some())
            .finish()
    }
}

impl AcceptanceWorld {
    fn new() -> Self {
        // 同步上下文中不能 await——harness 的惰性初始化在首个 Given 步骤完成。
        Self::default()
    }

    pub async fn get_harness(&self) -> Arc<SharedHarness> {
        harness().await
    }
}

// ---------------------------------------------------------------------------
// 通用步骤
// ---------------------------------------------------------------------------

#[given("an admin API key")]
async fn given_admin_key(w: &mut AcceptanceWorld) {
    let harness = w.get_harness().await;
    w.auth_key = Some(harness.admin_key.clone());
}

#[given("no API key")]
async fn given_no_key(w: &mut AcceptanceWorld) {
    w.auth_key = None;
}

/// 经 admin 端点签发普通 key（R-acceptance-003：签发→使用完整链路）。
#[given("a regular API key signed via admin endpoint")]
async fn given_regular_key(w: &mut AcceptanceWorld) {
    let harness = w.get_harness().await;
    let payload = serde_json::json!({
        "team_id": uuid::Uuid::from_u128(1), // DEFAULT_TEAM_ID（bootstrap 已建）
        "scopes": ["read", "write"],
    });
    let response = harness
        .server
        .post("/v1/admin/api-keys")
        .json(&payload)
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {}", harness.admin_key),
        )
        .await;
    assert_eq!(
        response.status_code().as_u16(),
        201,
        "regular key signing failed: {}",
        response.text()
    );
    let json: Json = response.json();
    let key = json
        .pointer("/data/api_key")
        .and_then(|v| v.as_str())
        .expect("data.api_key missing")
        .to_string();
    w.auth_key = Some(key);
}

#[given("an invalid API key")]
async fn given_invalid_key(w: &mut AcceptanceWorld) {
    w.auth_key = Some("00000000000000000000000000000000.invalidsecretinvalidsecretinvalidsecret".to_string());
}

#[when(expr = "I GET {string}")]
async fn when_get(w: &mut AcceptanceWorld, path: String) {
    let harness = w.get_harness().await;
    let mut request = harness.server.get(&path);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// 原样发送 Authorization 头（畸形格式异常矩阵用）。
#[when(expr = "I GET {string} with raw Authorization {string}")]
async fn when_get_raw_auth(w: &mut AcceptanceWorld, path: String, raw: String) {
    let harness = w.get_harness().await;
    let request = harness
        .server
        .get(&path)
        .add_header(axum_test::http::header::AUTHORIZATION, &raw);
    w.last_response = Some(request.await);
}

/// 通用 POST 辅助：当前认证态 + JSON body。
async fn post_json(w: &mut AcceptanceWorld, path: &str, payload: Json) {
    let harness = w.get_harness().await;
    let mut request = harness.server.post(path).json(&payload);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// 用当前认证态签发 API key（given 步骤辅助，断言 201 并返回明文 key）。
async fn sign_api_key(w: &mut AcceptanceWorld, scopes: &[&str]) -> String {
    let harness = w.get_harness().await;
    let payload = serde_json::json!({
        "team_id": uuid::Uuid::from_u128(1), // DEFAULT_TEAM_ID（bootstrap 已建）
        "scopes": scopes,
    });
    let response = harness
        .server
        .post("/v1/admin/api-keys")
        .json(&payload)
        .add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {}", harness.admin_key),
        )
        .await;
    assert_eq!(
        response.status_code().as_u16(),
        201,
        "regular key signing failed: {}",
        response.text()
    );
    let json: Json = response.json();
    json.pointer("/data/api_key")
        .and_then(|v| v.as_str())
        .expect("data.api_key missing")
        .to_string()
}

#[when(expr = "I sign a regular API key with scopes {string}")]
async fn when_sign_regular_key(w: &mut AcceptanceWorld, scopes: String) {
    let scope_list: Vec<&str> = scopes.split(',').map(str::trim).collect();
    let harness = w.get_harness().await;
    let payload = serde_json::json!({
        "team_id": uuid::Uuid::from_u128(1), // DEFAULT_TEAM_ID（bootstrap 已建）
        "scopes": scope_list,
    });
    let mut request = harness.server.post("/v1/admin/api-keys").json(&payload);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    let response = request.await;
    let status = response.status_code().as_u16();
    let text = response.text();
    w.last_status = Some(status);
    if status == 201 {
        let json: Json =
            serde_json::from_str(&text).expect("sign response JSON malformed");
        let key = json
            .pointer("/data/api_key")
            .and_then(|v| v.as_str())
            .expect("data.api_key missing")
            .to_string();
        w.auth_key = Some(key);
        w.last_body = Some(text);
    }
}

#[when(expr = "I POST {string} scraping {string}")]
async fn when_post_scrape(w: &mut AcceptanceWorld, path: String, url: String) {
    post_json(w, &path, serde_json::json!({"url": url})).await;
}

#[when(expr = "I POST {string} extracting {string}")]
async fn when_post_extract(w: &mut AcceptanceWorld, path: String, url: String) {
    post_json(w, &path, serde_json::json!({"url": url})).await;
}

#[when(expr = "I POST {string} crawling {string} with max depth {int}")]
async fn when_post_crawl(
    w: &mut AcceptanceWorld,
    path: String,
    url: String,
    max_depth: i32,
) {
    post_json(
        w,
        &path,
        serde_json::json!({"url": url, "config": {"max_depth": max_depth}}),
    )
    .await;
}

#[when(expr = "I POST {string} searching {string}")]
async fn when_post_search(w: &mut AcceptanceWorld, path: String, query: String) {
    post_json(w, &path, serde_json::json!({"query": query})).await;
}

#[when(expr = "I POST {string} mapping {string}")]
async fn when_post_map(w: &mut AcceptanceWorld, path: String, url: String) {
    post_json(w, &path, serde_json::json!({"url": url})).await;
}

#[when(expr = "I POST {string} creating webhook {string} for events {string}")]
async fn when_post_webhook(w: &mut AcceptanceWorld, path: String, url: String, events: String) {
    let event_list: Vec<&str> = events.split(',').map(str::trim).collect();
    post_json(
        w,
        &path,
        serde_json::json!({"url": url, "events": event_list}),
    )
    .await;
}

#[when(expr = "I DELETE {string}")]
async fn when_delete(w: &mut AcceptanceWorld, path: String) {
    let harness = w.get_harness().await;
    let mut request = harness.server.delete(&path);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

#[then(expr = "the response status is {int}")]
async fn then_status(w: &mut AcceptanceWorld, expected: u16) {
    if let Some(response) = &w.last_response {
        let actual = response.status_code().as_u16();
        assert_eq!(
            actual, expected,
            "unexpected status; body: {}",
            response.text()
        );
    } else if let Some(status) = w.last_status {
        assert_eq!(status, expected, "unexpected status; body: {:?}", w.last_body);
    } else {
        panic!("no response captured — send a request first");
    }
}

/// 取最近响应的 JSON：优先 TestResponse，response 被消费时用 last_body 遗留。
async fn last_json(w: &AcceptanceWorld) -> Json {
    if let Some(response) = &w.last_response {
        response.json()
    } else if let Some(body) = &w.last_body {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("last body malformed: {e}: {body}"))
    } else {
        panic!("no response captured — send a request first");
    }
}

#[then(expr = "the response JSON field {string} is {string}")]
async fn then_json_field_is_string(w: &mut AcceptanceWorld, field: String, expected: String) {
    let json = last_json(w).await;
    // 字段名自动转 JSON Pointer：顶层 "status" → "/status"，嵌套 "a.b" → "/a/b"
    let pointer = format!("/{}", field.replace('.', "/"));
    let actual = json
        .pointer(&pointer)
        .unwrap_or_else(|| panic!("field {field} missing in response: {json}"));
    assert_eq!(
        actual.as_str().unwrap_or_else(|| panic!("field {field} is not a string")),
        expected,
        "field {field} mismatch"
    );
}

#[then(expr = "the response JSON field {string} is true")]
async fn then_json_field_is_true(w: &mut AcceptanceWorld, field: String) {
    let json = last_json(w).await;
    let pointer = format!("/{}", field.replace('.', "/"));
    let actual = json
        .pointer(&pointer)
        .unwrap_or_else(|| panic!("field {field} missing in response: {json}"));
    assert!(actual.as_bool() == Some(true), "field {field} must be true: {json}");
}

#[then(expr = "the response JSON field {string} is a non-empty string")]
async fn then_json_field_nonempty_string(w: &mut AcceptanceWorld, field: String) {
    let json = last_json(w).await;
    let pointer = format!("/{}", field.replace('.', "/"));
    let actual = json
        .pointer(&pointer)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("field {field} missing or not a string: {json}"));
    assert!(!actual.is_empty(), "field {field} must be non-empty");
}

#[then(expr = "the response JSON pointer {string} is a non-empty array")]
async fn then_json_pointer_nonempty_array(w: &mut AcceptanceWorld, pointer: String) {
    let json = last_json(w).await;
    let value = json
        .pointer(&pointer)
        .unwrap_or_else(|| panic!("pointer {pointer} missing in response: {json}"));
    let arr = value
        .as_array()
        .unwrap_or_else(|| panic!("pointer {pointer} is not an array: {json}"));
    assert!(!arr.is_empty(), "pointer {pointer} is empty: {json}");
}

/// 生成场景内唯一 URL 路径段（数据隔离）
pub fn unique_suffix() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

// ---------------------------------------------------------------------------
// 测试数据库解析（等价 src/common/test_helpers.rs，但该模块是 lib-test 门控，
// 验收二进制不可见，故在此自持实现；migrations 覆盖全部 7 个 SQL 文件）
// ---------------------------------------------------------------------------

/// 解析验收数据库 URL：TEST_DATABASE_URL / DATABASE_URL 优先，否则 testcontainers。
fn resolve_acceptance_db_url() -> Option<String> {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return Some(url);
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return Some(url);
    }
    if !docker_available() {
        return None;
    }
    Some(start_testcontainers_pg())
}

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 起 PostgreSQL 16 容器并按文件名序执行 migrations/*.sql（进程内一次）。
fn start_testcontainers_pg() -> String {
    use testcontainers::core::IntoContainerPort;
    use testcontainers::runners::AsyncRunner;
    use testcontainers::ImageExt;

    static TC_URL: OnceLock<String> = OnceLock::new();
    static TC_CONTAINER: OnceLock<testcontainers::ContainerAsync<Postgres>> = OnceLock::new();

    TC_URL
        .get_or_init(|| {
            std::thread::scope(|s| {
                let handle = s.spawn(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("runtime for testcontainers");
                    rt.block_on(async {
                        let image = Postgres::default()
                            .with_init_sql(b"ALTER SYSTEM SET max_connections = 2000;".to_vec())
                            .with_tag("16-alpine");
                        let container = image
                            .start()
                            .await
                            .expect("failed to start testcontainers PostgreSQL");
                        let port = container
                            .get_host_port_ipv4(5432.tcp())
                            .await
                            .expect("failed to get postgres port");
                        let url =
                            format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

                        use sea_orm::{ConnectOptions, ConnectionTrait, Database};
                        let mut opt = ConnectOptions::new(&url);
                        opt.sqlx_logging(false);
                        let conn = Database::connect(opt)
                            .await
                            .expect("connect for migrations");

                        // 全部 migrations 按文件名序执行（含 006 message_queue、
                        // 007 retention 索引，lib 侧 test_helpers 尚未覆盖）
                        let mut files: Vec<_> = std::fs::read_dir("migrations")
                            .expect("read migrations dir")
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false))
                            .collect();
                        files.sort();
                        for path in files {
                            let sql = std::fs::read_to_string(&path)
                                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                            conn.execute_unprepared(&sql)
                                .await
                                .unwrap_or_else(|e| panic!("run {}: {e}", path.display()));
                        }

                        // 容器句柄保活至进程结束（Ryuk 兜底 + 正常 drop 清理）
                        let _ = TC_CONTAINER.set(container);

                        eprintln!(
                            "[testcontainers] PostgreSQL started at 127.0.0.1:{port} with migrations applied"
                        );
                        url
                    })
                });
                handle.join().expect("testcontainers thread panicked")
            })
        })
        .clone()
}
