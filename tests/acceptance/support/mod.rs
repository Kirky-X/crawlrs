// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! BDD 验收套件支撑层：World 定义 + 完整启动链 + 通用步骤。
//!
//! 启动链（进程内一次，garrison 单例约束）：
//! testcontainers PG（migrations 全量）→ env 注入 → Settings → DI kit →
//! CrawlRsState → garrison init → bootstrap admin key → build_api_app_with_state
//! → axum-test TestServer → spawn_common_workers + WorkerManager（任务消费）。
//!
//! 数据隔离：场景间用 UUID 唯一资源，不依赖全局清理（与既有集成测试约定一致）。

use std::sync::{Arc, OnceLock};

use testcontainers_modules::postgres::Postgres;

use axum_test::TestServer;
use crawlrs::di::CrawlRsStateExt as _;
use cucumber::{given, then, when};
use serde_json::Value as Json;

// ---------------------------------------------------------------------------
// 共享 Harness（进程级单次启动）
// ---------------------------------------------------------------------------

pub struct SharedHarness {
    pub server: TestServer,
    /// bootstrap 签发的 admin key（`crawlrs:admin` 全权限，`<key_id>.<secret>` 形态）
    pub admin_key: String,
    /// 场景模板变量（feature 步骤中 {name} 引用，含 `mock_base`）
    pub template_vars: std::collections::HashMap<String, String>,
    /// mock 目标站句柄保活
    _mock_guards: (
        wiremock::MockServer,
        wiremock::MockServer,
        wiremock::MockServer,
        wiremock::MockServer,
        wiremock::MockServer,
    ),
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

/// webhook 签名测试密钥（与 harness env 注入一致）
const TEST_WEBHOOK_SECRET: &str = "whsec_acceptance-test-webhook-secret-key";

/// wiremock 目标站：页面 + sitemap + 搜索引擎 mock（零外部网络依赖）。
async fn start_mock_target() -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;

    // 正常页面（含可提取正文；内容足够长以通过反爬 near-empty 启发式）
    let rich_page = "<html><head><title>Acceptance Page</title></head><body><h1>Acceptance</h1><p>acceptance-marker-content</p><p>Rust is a multi-paradigm, general-purpose programming language that emphasizes performance, type safety, and concurrency. It enforces memory safety, meaning that all references point to valid memory.</p><p>Web scraping is data extraction used for copying data from the web. This page contains enough textual content to satisfy content-quality heuristics during crawl acceptance verification runs.</p><ul><li>item one with descriptive text content</li><li>item two with additional descriptive text</li></ul></body></html>";
    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rich_page))
        .mount(&server)
        .await;
    // 失败端点（500）
    Mock::given(method("GET"))
        .and(path("/fail"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    // crawl 互链三页
    for (page, link) in [
        ("/page_a", "/page_b"),
        ("/page_b", "/page_c"),
        ("/page_c", ""),
    ] {
        let body = if link.is_empty() {
            format!("<html><body><h1>{page}</h1><p>This leaf page carries substantial descriptive content so that the anti-bot near-empty-body heuristic does not misjudge it during acceptance runs.</p><p>Additional paragraph with plenty of real textual content describing the purpose of this fixture page in detail.</p></body></html>")
        } else {
            format!("<html><body><a href=\"{link}\">next</a><p>{page} carries substantial descriptive content so the anti-bot near-empty-body heuristic does not misjudge this fixture page during acceptance verification runs.</p><p>Another paragraph with enough real text to pass content quality checks reliably.</p></body></html>")
        };
        Mock::given(method("GET"))
            .and(path(page))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }
    // hook 端点（webhook 投递目标）
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    server
}

/// sitemap 站：`{sitemap_base}/sitemap.xml` 返回 index（2 子 sitemap 共 5 loc）
async fn start_mock_sitemap_site() -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    // 根 sitemap：3 loc，其中 1 个匹配 */blog/*
    Mock::given(method("GET"))
        .and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<urlset>\
             <url><loc>https://example.com/</loc></url>\
             <url><loc>https://example.com/blog/post-1</loc></url>\
             <url><loc>https://example.com/blog/post-2</loc></url></urlset>",
        ))
        .mount(&server)
        .await;
    server
}

/// 空站：任何 GET 都 404（map「缺 sitemap」场景）
async fn start_mock_404_site() -> wiremock::MockServer {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    server
}

/// 故障站：任何 GET 都 500（map「目标不可达」场景）
async fn start_mock_500_site() -> wiremock::MockServer {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    server
}

/// sitemap index 站：根为 index（2 子 sitemap 共 5 loc），map 递归场景
async fn start_mock_sitemap_index_site() -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let base = server.uri();
    let index_body = format!(
        "<sitemapindex>         <sitemap><loc>{base}/s1.xml</loc></sitemap>         <sitemap><loc>{base}/s2.xml</loc></sitemap></sitemapindex>"
    );
    Mock::given(method("GET"))
        .and(path("/sitemap.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(index_body))
        .mount(&server)
        .await;
    // 5 loc 合计（s1: 3，s2: 2）；s1 含一个 blog 路径供 include 场景复用
    let s1 = "<urlset>              <url><loc>https://example.com/1</loc></url>              <url><loc>https://example.com/2</loc></url>              <url><loc>https://example.com/blog/keep</loc></url></urlset>";
    let s2 = "<urlset>              <url><loc>https://example.com/4</loc></url>              <url><loc>https://example.com/5</loc></url></urlset>";
    Mock::given(method("GET"))
        .and(path("/s1.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(s1))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/s2.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(s2))
        .mount(&server)
        .await;
    server
}

impl SharedHarness {
    async fn bootstrap() -> anyhow::Result<Self> {
        use trait_kit::AsyncKit;

        // 0. 进程级 env：本测试二进制独立进程，不存在与其他测试的并行冲突。
        // SSRF 关闭使 wiremock（127.0.0.1）可作 mock 目标站（specs/acceptance-testing
        // Constraints：零外部网络依赖）。
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");
        std::env::set_var("CRAWLRS__AUTH__JWT_SECRET", TEST_JWT_SECRET);
        std::env::set_var("CRAWLRS__WEBHOOK__SECRET", TEST_WEBHOOK_SECRET);

        // 1. 数据库 URL：env 优先，否则 testcontainers PG + migrations
        let db_url = resolve_acceptance_db_url().ok_or_else(|| {
            anyhow::anyhow!("no test database: set TEST_DATABASE_URL or start Docker")
        })?;
        std::env::set_var("CRAWLRS__DATABASE__URL", &db_url);

        // 2. Settings
        let settings = Arc::new(crawlrs::bootstrap::config::load_settings()?);

        // 2.5 telemetry：worker 的 log 宏经 inklog 输出（RUST_LOG=debug 可见）
        let _logger_manager = crawlrs::bootstrap::telemetry::init_all(&settings.logging)
            .await
            .map_err(|e| anyhow::anyhow!("init inklog: {e}"))?;

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
        // HttpModule 持有共享 reqwest::Client，WorkerManager 需要它（对齐 main.rs:357）
        let http_client: std::sync::Arc<reqwest::Client> = kit
            .require::<crawlrs::di::modules::HttpModule>()
            .map_err(|e| anyhow::anyhow!("require HttpModule: {e}"))?;

        // 4. garrison 单例初始化已由 ServiceModule 内部完成（重复调用会触发
        // "global DAO already injected"），此处只做 bootstrap 数据准备。
        // 5. bootstrap：default team + admin key（对齐 main.rs run_bootstrap 语义）
        let admin_key = bootstrap_admin_key(app_state.db_pool.clone()).await?;

        // 6. app + TestServer（内存 HTTP，不占端口）
        let app =
            crawlrs::bootstrap::routes::build_api_app_with_state(&app_state, settings.clone());
        // ConnectInfo 注入：部分 handler 提取 ConnectInfo<SocketAddr>（限流/审计），
        // oneshot transport 默认不提供——显式包一层 make service（axum-test 文档形态）。
        let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
        let server = TestServer::new(app);

        // 7. workers 真实运行（scrape/crawl 任务由 DB 队列驱动消费）
        let _worker_handles =
            crawlrs::bootstrap::workers::spawn_common_workers(&app_state, &settings, true).await;

        // 7b. 任务执行 worker：spawn_common_workers 只覆盖 webhook/backlog/expiration/
        // retention，队列里的 scrape/crawl 任务需 WorkerManager 消费（对齐 main.rs
        // start_worker_service）。缺失时任务永远停在 queued，生命周期场景即为假绿。
        let coordinator =
            std::sync::Arc::new(crawlrs::workers::shutdown::ShutdownCoordinator::new(
                std::time::Duration::from_secs(settings.workers.graceful_shutdown_seconds),
            ));
        let worker_deps = crawlrs::workers::manager::WorkerManagerDeps {
            queue: app_state.task_queue(),
            repository: app_state.task_repo(),
            result_repository: app_state.result_repo(),
            crawl_repository: app_state.crawl_repo(),
            webhook_service: app_state.webhook_service(),
            credits_repository: app_state.credits_repo(),
            engine_client: app_state.engine_client(),
            create_scrape_use_case: app_state.create_scrape_use_case(),
            team_semaphore: app_state.team_semaphore.clone(),
            webhook_event_repository: app_state.webhook_event_repo(),
            geo_restriction_repository: app_state.geo_restriction_repo(),
            audit_service: app_state.audit_service(),
            request_coalescer: app_state.request_coalescer.clone(),
            robots_checker: app_state.robots_checker.clone(),
            http_client,
            extraction_service: app_state.extraction_service(),
            regex_cache: (*app_state.regex_cache()).clone(),
            cache_service: app_state.cache_service(),
            shutdown_coordinator: coordinator,
            retention_lock: std::sync::Arc::new(
                crawlrs::workers::retention_worker::PgRetentionLock::new(app_state.db_pool()),
            ),
        };
        let worker_config = crawlrs::workers::manager::WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: settings.concurrency.default_team_limit as usize,
        };
        let mut worker_manager =
            crawlrs::workers::manager::WorkerManager::new(worker_deps, worker_config);
        let worker_count = settings.workers.count.resolve();
        worker_manager.start_workers(worker_count).await;
        // WorkerManager::drop 会 abort 全部 worker，而 harness 需存活至进程结束
        // （静态 OnceCell 语义）。显式 forget 保持 worker 运行，进程退出即回收。
        std::mem::forget(worker_manager);

        // 8. mock 目标站群（零外部网络依赖）
        let mock_main = start_mock_target().await;
        let mock_sitemap = start_mock_sitemap_site().await;
        let mock_index = start_mock_sitemap_index_site().await;
        let mock_404 = start_mock_404_site().await;
        let mock_500 = start_mock_500_site().await;
        // 场景模板变量（feature 中以 {mock_base} 等引用）
        let mut template_vars = std::collections::HashMap::new();
        template_vars.insert("mock_base".to_string(), mock_main.uri());
        template_vars.insert("sitemap_base".to_string(), mock_sitemap.uri());
        template_vars.insert("index_base".to_string(), mock_index.uri());
        template_vars.insert("site404".to_string(), mock_404.uri());
        template_vars.insert("site500".to_string(), mock_500.uri());

        Ok(Self {
            server,
            admin_key,
            template_vars,
            _mock_guards: (mock_main, mock_sitemap, mock_index, mock_404, mock_500),
        })
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
    use sea_orm::{ActiveValue, ConnectionTrait, EntityTrait};
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

    // credits 充值：scrape/crawl/search 走 check_and_deduct_quota（balance=0 会
    // 返回 QUOTA_EXCEEDED）。验收场景需要可用余额。
    conn.execute_unprepared(&format!(
        "INSERT INTO credits (team_id, balance) VALUES ('{}', 100000)
         ON CONFLICT (team_id) DO UPDATE SET balance = GREATEST(credits.balance, 100000)",
        team_id
    ))
    .await?;

    #[cfg(feature = "auth")]
    {
        use crawlrs::infrastructure::auth::get_garrison_dao;
        use garrison::protocol::apikey::ApiKeyHandler;

        let dao =
            get_garrison_dao().ok_or_else(|| anyhow::anyhow!("garrison DAO not initialized"))?;
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
    w.auth_key = Some(
        "00000000000000000000000000000000.invalidsecretinvalidsecretinvalidsecret".to_string(),
    );
}

#[when(expr = "I GET {string}")]
async fn when_get(w: &mut AcceptanceWorld, path: String) {
    let harness = w.get_harness().await;
    let path = expand_templates(w, path);
    let mut request = harness.server.get(&path);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// 展开 harness 级模板变量（{mock_base} 等）与场景 ctx 变量（{task_id} 等）。
fn expand_templates(w: &AcceptanceWorld, mut s: String) -> String {
    if let Some(arc) = HARNESS.get() {
        for (k, v) in &arc.template_vars {
            s = s.replace(&format!("{{{k}}}"), v);
        }
    }
    for (k, v) in &w.ctx {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
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
    let path = expand_templates(w, path.to_string());
    let mut request = harness.server.post(&path).json(&payload);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
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
        let json: Json = serde_json::from_str(&text).expect("sign response JSON malformed");
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
    let url = expand_templates(w, url);
    post_json(w, &path, serde_json::json!({"url": url})).await;
}

#[when(expr = "I POST {string} extracting {string}")]
async fn when_post_extract(w: &mut AcceptanceWorld, path: String, url: String) {
    post_json(w, &path, serde_json::json!({"url": url})).await;
}

#[when(expr = "I POST {string} crawling {string} with max depth {int}")]
async fn when_post_crawl(w: &mut AcceptanceWorld, path: String, url: String, max_depth: i32) {
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

// ---- feature 面向业务的步骤别名（与 feature 文件语句一致） ----

/// scrape.feature：创建 scrape 任务并记录 task id。
#[when(expr = "I create a scrape at {string} for {string}")]
async fn when_create_scrape(w: &mut AcceptanceWorld, path: String, url: String) {
    let url = expand_templates(w, url);
    post_json(w, &path, serde_json::json!({"url": url})).await;
    let json = last_json(w).await;
    if let Some(id) = json
        .pointer("/data/id")
        .or_else(|| json.pointer("/data/task_id"))
        .and_then(|v| v.as_str().map(str::to_string))
    {
        w.ctx.insert("task_id".into(), id);
    }
}

/// scrape.feature：空 body POST（缺 url 字段异常矩阵）。
#[when(expr = "I POST {string} with an empty body")]
async fn when_post_empty_body(w: &mut AcceptanceWorld, path: String) {
    let harness = w.get_harness().await;
    let path = expand_templates(w, path);
    let mut request = harness.server.post(&path).json(&serde_json::json!({}));
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// extract.feature：`I extract url {url} at {path}`（ExtractRequestDto 契约为
/// `urls` 数组；Gherkin `{string}` 不支持内嵌引号，故单 URL 入参在步骤内包数组）。
#[when(expr = "I extract url {string} at {string}")]
async fn when_extract_urls(w: &mut AcceptanceWorld, url: String, path: String) {
    let url = expand_templates(w, url);
    // DTO 契约：prompt/schema/rules 三选一必填，默认给一条 CSS 规则
    post_json(
        w,
        &path,
        serde_json::json!({"urls": [url], "extraction_rules": {"content": {"selector": "body", "is_array": false}}}),
    )
    .await;
}

/// search.feature：`I search for {query} at {path}`。
#[when(expr = "I search for {string} at {string}")]
async fn when_search_for(w: &mut AcceptanceWorld, query: String, path: String) {
    post_json(w, &path, serde_json::json!({"query": query})).await;
}

/// search.feature：显式指定引擎（`{"query": q, "engine": e}`）。
#[when(expr = "I search for {string} at {string} with engine {string}")]
async fn when_search_with_engine(
    w: &mut AcceptanceWorld,
    query: String,
    path: String,
    engine: String,
) {
    post_json(
        w,
        &path,
        serde_json::json!({"query": query, "engine": engine}),
    )
    .await;
}

/// map.feature：`I map {url}`。
#[when(expr = "I map {string}")]
async fn when_map(w: &mut AcceptanceWorld, url: String) {
    let url = expand_templates(w, url);
    post_json(w, "/v1/map", serde_json::json!({"url": url})).await;
}

/// map.feature：带 include 过滤。
#[when(expr = "I map {string} including {string}")]
async fn when_map_including(w: &mut AcceptanceWorld, url: String, include: String) {
    let url = expand_templates(w, url);
    post_json(
        w,
        "/v1/map",
        serde_json::json!({"url": url, "include_patterns": [include]}),
    )
    .await;
}

/// map.feature：带 limit。
#[when(expr = "I map {string} with limit {int}")]
async fn when_map_with_limit(w: &mut AcceptanceWorld, url: String, limit: i32) {
    let url = expand_templates(w, url);
    post_json(
        w,
        "/v1/map",
        serde_json::json!({"url": url, "limit": limit}),
    )
    .await;
}

/// platform.feature：创建 webhook（Standard Webhooks 签名：HMAC-SHA256 over
/// `{msg_id}.{timestamp}.{body}`，header `webhook-signature: v1,<b64>`）。
#[when(expr = "I create a webhook at {string} pointing to {string} for events {string}")]
async fn when_create_webhook(w: &mut AcceptanceWorld, path: String, url: String, events: String) {
    use base64::Engine as _;
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    let url = expand_templates(w, url);
    let event_list: Vec<&str> = events.split(',').map(str::trim).collect();
    let _ = event_list; // CreateWebhookRequest 契约仅 url 字段（服务端事件类型由任务驱动）
    let payload = serde_json::json!({"url": url});
    let body = serde_json::to_string(&payload).expect("serialize webhook payload");

    let msg_id = format!("msg-{}", uuid::Uuid::new_v4());
    let timestamp = chrono::Utc::now().timestamp();
    let to_sign = format!("{msg_id}.{timestamp}.{body}");
    let mut mac: Hmac<Sha256> =
        Hmac::new_from_slice(TEST_WEBHOOK_SECRET.as_bytes()).expect("hmac key");
    mac.update(to_sign.as_bytes());
    let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    let harness = w.get_harness().await;
    let path = expand_templates(w, path);
    let mut request = harness
        .server
        .post(&path)
        .add_header("webhook-id", &msg_id)
        .add_header("webhook-timestamp", &timestamp.to_string())
        .add_header("webhook-signature", &format!("v1,{sig}"))
        .add_header(axum_test::http::header::CONTENT_TYPE, "application/json")
        .text(&body);
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
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
            actual,
            expected,
            "unexpected status; body: {}",
            response.text()
        );
    } else if let Some(status) = w.last_status {
        assert_eq!(
            status, expected,
            "unexpected status; body: {:?}",
            w.last_body
        );
    } else {
        panic!("no response captured — send a request first");
    }
}

#[then(expr = "the response status is {int} or {int}")]
async fn then_status_either(w: &mut AcceptanceWorld, a: u16, b: u16) {
    let (actual, body) = if let Some(response) = &w.last_response {
        let code = response.status_code().as_u16();
        let body = if code == a || code == b {
            String::new()
        } else {
            response.text()
        };
        (code, body)
    } else if let Some(status) = w.last_status {
        (status, w.last_body.clone().unwrap_or_default())
    } else {
        panic!("no response captured — send a request first");
    };
    assert!(
        actual == a || actual == b,
        "unexpected status {actual}; expected {a} or {b}; body: {body}"
    );
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
        actual
            .as_str()
            .unwrap_or_else(|| panic!("field {field} is not a string")),
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
    assert!(
        actual.as_bool() == Some(true),
        "field {field} must be true: {json}"
    );
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

#[then(expr = "the response JSON pointer {string} is an array of length {int}")]
async fn then_json_pointer_array_length(w: &mut AcceptanceWorld, pointer: String, expected: usize) {
    let json = last_json(w).await;
    let value = json
        .pointer(&pointer)
        .unwrap_or_else(|| panic!("pointer {pointer} missing in response: {json}"));
    let arr = value
        .as_array()
        .unwrap_or_else(|| panic!("pointer {pointer} is not an array: {json}"));
    assert_eq!(
        arr.len(),
        expected,
        "pointer {pointer} length mismatch: {json}"
    );
}

#[then(expr = "the response JSON field {string} is one of {string}")]
async fn then_json_field_in_set(w: &mut AcceptanceWorld, field: String, allowed: String) {
    let json = last_json(w).await;
    let pointer = format!("/{}", field.replace('.', "/"));
    let actual = json
        .pointer(&pointer)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("field {field} missing or not a string: {json}"))
        .to_string();
    let allowed: Vec<&str> = allowed.split(',').map(str::trim).collect();
    assert!(
        allowed.contains(&actual.as_str()),
        "field {field} = {actual} not in {:?}",
        allowed.join(",")
    );
}

#[then(expr = "the response JSON field {string} is a non-empty string or number")]
async fn then_json_field_nonempty_string_or_number(w: &mut AcceptanceWorld, field: String) {
    let json = last_json(w).await;
    let pointer = format!("/{}", field.replace('.', "/"));
    let value = json
        .pointer(&pointer)
        .unwrap_or_else(|| panic!("field {field} missing: {json}"));
    let ok = match value {
        Json::String(s) => !s.is_empty(),
        Json::Number(_) => true,
        _ => false,
    };
    assert!(
        ok,
        "field {field} must be non-empty string or number: {json}"
    );
}

// ---------------------------------------------------------------------------
// 任务终态轮询（scrape/crawl 生命周期场景共用）
// ---------------------------------------------------------------------------

/// 轮询任务至终态（completed/failed/cancelled），返回最后状态。
async fn poll_task_to_terminal(
    w: &mut AcceptanceWorld,
    detail_path: String,
    max_secs: u64,
) -> String {
    let harness = w.get_harness().await;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(max_secs);
    let mut last_status = String::new();
    let mut last_body = String::new();
    while tokio::time::Instant::now() < deadline {
        let response = harness
            .server
            .get(&detail_path)
            .add_header(
                axum_test::http::header::AUTHORIZATION,
                &format!("Bearer {}", w.auth_key.clone().unwrap_or_default()),
            )
            .await;
        let status = response.status_code().as_u16();
        last_body = response.text();
        if status == 200 {
            let json: Json = serde_json::from_str(&last_body)
                .unwrap_or_else(|e| panic!("task detail malformed: {e}: {last_body}"));
            last_status = json
                .pointer("/data/status")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if matches!(last_status.as_str(), "completed" | "failed" | "cancelled") {
                w.last_status = Some(status);
                w.last_body = Some(last_body.clone());
                return last_status;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    panic!(
        "task did not reach terminal state within {max_secs}s; last status={last_status} body={last_body}"
    );
}

/// Given 版本：Given a scrape task at {path} for {url} completed within {n} seconds
#[given(expr = "a scrape task at {string} for {string} completed within {int} seconds")]
async fn given_scrape_completed(w: &mut AcceptanceWorld, path: String, url: String, secs: u64) {
    post_json(w, &path, serde_json::json!({"url": url})).await;
    let json = last_json(w).await;
    let task_id = json
        .pointer("/data/id")
        .or_else(|| json.pointer("/data/task_id"))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("task id missing in creation response: {json}"));
    w.ctx.insert("task_id".into(), task_id.clone());
    let status = poll_task_to_terminal(w, format!("/v1/scrape/{task_id}"), secs).await;
    assert_eq!(status, "completed", "scrape task must complete");
}

/// When 版本：创建 scrape 并轮询至终态
#[when(expr = "I wait for scrape {string} to complete within {int} seconds")]
async fn when_wait_scrape_terminal(w: &mut AcceptanceWorld, path_prefix: String, secs: u64) {
    let task_id = w
        .ctx
        .get("task_id")
        .cloned()
        .expect("task_id in ctx (create a scrape first)");
    let _ = path_prefix;
    let status = poll_task_to_terminal(w, format!("/v1/scrape/{task_id}"), secs).await;
    w.ctx.insert("terminal_status".into(), status);
}

// ---- crawl 生命周期 ----

/// 创建 crawl 任务并记录 task id。
#[when(expr = "I create a crawl at {string} for {string} with max depth {int}")]
async fn when_create_crawl(w: &mut AcceptanceWorld, path: String, url: String, max_depth: i32) {
    let url = expand_templates(w, url);
    post_json(
        w,
        &path,
        serde_json::json!({"url": url, "config": {"max_depth": max_depth}}),
    )
    .await;
    let json = last_json(w).await;
    if let Some(id) = json
        .pointer("/data/id")
        .or_else(|| json.pointer("/data/task_id"))
        .or_else(|| json.pointer("/data/crawl_id"))
        .and_then(|v| v.as_str().map(str::to_string))
    {
        w.ctx.insert("task_id".into(), id);
    }
}

/// 轮询 crawl 任务至终态。
#[when(expr = "I wait for crawl {string} to complete within {int} seconds")]
async fn when_wait_crawl_terminal(w: &mut AcceptanceWorld, path_prefix: String, secs: u64) {
    let task_id = w
        .ctx
        .get("task_id")
        .cloned()
        .expect("task_id in ctx (create a crawl first)");
    let _ = path_prefix;
    let status = poll_task_to_terminal(w, format!("/v1/crawl/{task_id}"), secs).await;
    w.ctx.insert("terminal_status".into(), status);
}

/// Given：crawl 已完成（供 results 场景复用）。
#[given(expr = "a crawl of {string} completed")]
async fn given_crawl_completed(w: &mut AcceptanceWorld, url: String) {
    let url = expand_templates(w, url);
    given_admin_key(w).await;
    post_json(
        w,
        "/v1/crawl",
        serde_json::json!({"url": url, "config": {"max_depth": 1}}),
    )
    .await;
    let json = last_json(w).await;
    let task_id = json
        .pointer("/data/id")
        .or_else(|| json.pointer("/data/task_id"))
        .or_else(|| json.pointer("/data/crawl_id"))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("crawl id missing: {json}"));
    w.ctx.insert("task_id".into(), task_id);
    let status = poll_task_to_terminal(w, format!("/v1/crawl/{}", w.ctx["task_id"]), 60).await;
    assert_eq!(status, "completed", "crawl must complete");
}

/// When：GET 任务详情（用 ctx 里的 task_id，前缀决定 scrape/crawl 面）。
#[when(expr = "I GET the task detail at {string}")]
async fn when_get_task_detail(w: &mut AcceptanceWorld, path_prefix: String) {
    let task_id = w.ctx.get("task_id").cloned().expect("task_id in ctx");
    let harness = w.get_harness().await;
    let mut request = harness.server.get(&format!("{path_prefix}/{task_id}"));
    if let Some(key) = &w.auth_key {
        request = request.add_header(
            axum_test::http::header::AUTHORIZATION,
            &format!("Bearer {key}"),
        );
    }
    w.last_response = Some(request.await);
}

/// 状态码属于给定集合（契约尚不稳定的错误映射用，避免把未定型的口径写成硬断言）。
#[then(expr = "the response status is one of {string}")]
async fn then_status_in_set(w: &mut AcceptanceWorld, allowed: String) {
    let expected: Vec<u16> = allowed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u16>().expect("status list must be u16 csv"))
        .collect();
    let status = if let Some(response) = &w.last_response {
        response.status_code().as_u16()
    } else if let Some(status) = w.last_status {
        status
    } else {
        panic!("no response captured — send a request first");
    };
    assert!(
        expected.contains(&status),
        "unexpected status {status}; expected one of {expected:?}"
    );
}

/// 响应体文本包含子串（结果内容特征串断言，规避结果嵌套结构差异）。
#[then(expr = "the response body contains {string}")]
async fn then_body_contains(w: &mut AcceptanceWorld, needle: String) {
    let body = w
        .last_body
        .clone()
        .or_else(|| w.last_response.as_ref().map(|r| r.text()))
        .expect("no response captured");
    assert!(
        body.contains(&needle),
        "response body does not contain {needle:?}: {body}"
    );
}

/// 记录任意 JSON 字段值到 ctx（后续 URL 模板引用）。
#[when(expr = "I store response JSON field {string} as {string}")]
async fn when_store_field(w: &mut AcceptanceWorld, field: String, name: String) {
    let json = last_json(w).await;
    let pointer = format!("/{}", field.replace('.', "/"));
    let value = json
        .pointer(&pointer)
        .map(|v| match v {
            Json::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| panic!("field {field} missing: {json}"));
    w.ctx.insert(name, value);
}

/// 用 ctx 模板 GET：路径中 {name} 占位符替换为 ctx 值。
#[when(expr = "I GET template {string}")]
async fn when_get_template(w: &mut AcceptanceWorld, template: String) {
    let mut path = template;
    for (name, value) in &w.ctx {
        path = path.replace(&format!("{{{name}}}"), value);
    }
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

/// 用 ctx 模板 DELETE。
#[when(expr = "I DELETE template {string}")]
async fn when_delete_template(w: &mut AcceptanceWorld, template: String) {
    let mut path = template;
    for (name, value) in &w.ctx {
        path = path.replace(&format!("{{{name}}}"), value);
    }
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
