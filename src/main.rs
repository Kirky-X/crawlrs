// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// Service type enumeration.
enum ServiceType {
    Api,
    Worker,
    Bootstrap,
}

/// Parse service type from an optional argument string.
///
/// Pure function extracted from `from_args` for testability.
/// - `Some("api")` or `None` → `Ok(ServiceType::Api)`
/// - `Some("worker")` → `Ok(ServiceType::Worker)`
/// - Other values → `Err` with descriptive message
fn parse_service_type(arg: Option<&str>) -> Result<ServiceType, String> {
    let service_type = arg.unwrap_or("api");
    match service_type {
        "api" => Ok(ServiceType::Api),
        "worker" => Ok(ServiceType::Worker),
        "bootstrap" => Ok(ServiceType::Bootstrap),
        other => Err(format!(
            "Invalid service type: '{}'. Use 'api', 'worker', or 'bootstrap'.",
            other
        )),
    }
}

impl ServiceType {
    /// Parse service type from command line arguments.
    /// Uses safe `args_os()` + explicit UTF-8 handling instead of `env::args()`,
    /// which panics on non-UTF-8 OS arguments without a user-facing error.
    fn from_args() -> Self {
        use log::error;
        use std::{env, process};
        let args: Vec<String> = env::args_os()
            .enumerate()
            .map(|(i, os)| match os.into_string() {
                Ok(s) => s,
                Err(utf8_err) => {
                    eprintln!(
                        "crawlrs: argument {} contains invalid UTF-8: {:?}",
                        i, utf8_err
                    );
                    process::exit(2);
                }
            })
            .collect();
        let arg = args.get(1).map(String::as_str);
        match parse_service_type(arg) {
            Ok(st) => st,
            Err(msg) => {
                error!("{}", msg);
                process::exit(1);
            }
        }
    }
}

mod app {
    use super::ServiceType;
    use crawlrs::bootstrap::routes::build_api_app_with_state;
    use crawlrs::bootstrap::workers::spawn_common_workers;
    use crawlrs::di::modules::{
        CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule,
        RepositoryModule, ServiceModule, SettingsModule,
    };
    use crawlrs::di::{CrawlRsState, CrawlRsStateExt};
    use crawlrs::workers::manager::{WorkerManager, WorkerManagerConfig};
    use std::env;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use trait_kit::AsyncKit;

    /// Start the API service.
    async fn start_api_service(
        app_state: &CrawlRsState,
        settings: Arc<crawlrs::config::settings::Settings>,
    ) -> anyhow::Result<()> {
        log::info!("Starting API service...");

        // Auto-bootstrap: create initial admin API key if env var is set.
        // This must run in the same process as the API server so that garrison's
        // in-memory oxcache retains the key for auth verification.
        if env::var("CRAWLRS_BOOTSTRAP_ADMIN")
            .map(|v| v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("1"))
            .unwrap_or(false)
        {
            log::info!("CRAWLRS_BOOTSTRAP_ADMIN=true, running auto-bootstrap...");
            run_bootstrap(app_state, &settings).await?;
        }

        // 启动通用 worker（webhook / backlog / expiration）
        spawn_common_workers(app_state, &settings).await;

        // Build API app with dependencies
        let app = build_api_app_with_state(app_state, settings.clone());

        // Start the server
        let addr = format!("{}:{}", settings.server.host, settings.server.port);
        let listener = TcpListener::bind(&addr).await?;
        log::info!("Server listening on {}", addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await?;

        Ok(())
    }

    /// Start the worker service.
    async fn start_worker_service(
        app_state: &CrawlRsState,
        settings: Arc<crawlrs::config::settings::Settings>,
        http_client: Arc<reqwest::Client>,
    ) -> anyhow::Result<()> {
        log::info!("Starting Worker service...");

        // R-security-004/005：优雅退出编排（design.md D3，T010）
        //
        // 创建共享 ShutdownCoordinator，spawn 信号监听任务（SIGTERM/SIGINT →
        // trigger），替换裸 `tokio::signal::ctrl_c().await`：
        // - `worker_manager.wait_for_shutdown()` 内部经 coordinator 阻塞等待信号，
        //   随后给进行中任务 ≤ graceful_period 的宽限期完成，再终止剩余句柄；
        // - 宽限期结束后 `rollback_pending_tasks` 把已锁定未完成任务回滚为 Pending。
        let coordinator = Arc::new(crawlrs::workers::shutdown::ShutdownCoordinator::new(
            std::time::Duration::from_secs(settings.workers.graceful_shutdown_seconds),
        ));
        let coord_for_signals = coordinator.clone();
        tokio::spawn(async move {
            if let Err(e) = crawlrs::workers::shutdown::listen_unix_signals(coord_for_signals).await
            {
                log::error!("Failed to listen for shutdown signals: {}", e);
            }
        });

        // 启动通用 worker（webhook / backlog / expiration）
        spawn_common_workers(app_state, &settings).await;

        // Create worker manager with dependencies (使用 DI 注入的服务)
        let deps = crawlrs::workers::manager::WorkerManagerDeps {
            queue: app_state.task_queue(),
            repository: app_state.task_repo(),
            result_repository: app_state.result_repo(),
            crawl_repository: app_state.crawl_repo(),
            webhook_service: app_state.webhook_service(),
            credits_repository: app_state.credits_repo(),
            engine_client: app_state.engine_client(),
            create_scrape_use_case: app_state.create_scrape_use_case(),
            team_semaphore: app_state.team_semaphore.clone(),
            request_coalescer: app_state.request_coalescer.clone(),
            robots_checker: app_state.robots_checker.clone(),
            http_client,
            extraction_service: app_state.extraction_service(),
            regex_cache: (*app_state.regex_cache()).clone(),
            cache_service: app_state.cache_service(),
            shutdown_coordinator: coordinator.clone(),
        };

        let config = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: settings.concurrency.default_team_limit as usize,
        };

        let mut worker_manager = WorkerManager::new(deps, config);

        // Start workers
        let worker_count = settings.workers.count.resolve();
        log::info!("Starting {} worker(s)", worker_count);
        worker_manager.start_workers(worker_count).await;

        // Keep the main thread alive until shutdown signal
        worker_manager.wait_for_shutdown().await;
        log::info!("Shutting down worker service...");

        // Roll back in-flight tasks so they aren't stuck in Active (R-security-005)
        let repo_for_rollback = app_state.task_repo();
        crawlrs::workers::shutdown::rollback_pending_tasks(
            &repo_for_rollback,
            std::time::Duration::from_secs(settings.workers.graceful_shutdown_seconds),
        )
        .await;

        Ok(())
    }

    /// Bootstrap: create initial team and admin API key for first-time deployment.
    async fn run_bootstrap(
        app_state: &CrawlRsState,
        _settings: &Arc<crawlrs::config::settings::Settings>,
    ) -> anyhow::Result<()> {
        use crawlrs::common::constants::default_identity::DEFAULT_TEAM_ID;

        let pool = app_state.db_pool();
        let session = pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("db session: {}", e))?;
        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("db conn: {}", e))?;

        // 1. Ensure default team exists
        use crawlrs::infrastructure::database::entities::team;
        use sea_orm::{EntityTrait, Set};
        let team_id = DEFAULT_TEAM_ID;
        let existing = team::Entity::find_by_id(team_id).one(conn).await
            .map_err(|e| anyhow::anyhow!("find team: {}", e))?;
        if existing.is_none() {
            let now = chrono::Utc::now();
            let model = team::ActiveModel {
                id: Set(team_id),
                name: Set("default-team".to_string()),
                allowed_countries: Set(None),
                blocked_countries: Set(None),
                ip_whitelist: Set(None),
                domain_blacklist: Set(None),
                enable_geo_restrictions: Set(false),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
            };
            team::Entity::insert(model).exec(conn).await
                .map_err(|e| anyhow::anyhow!("insert team: {}", e))?;
            println!("Created team: {}", team_id);
        } else {
            println!("Team already exists: {}", team_id);
        }

        // 2. Create admin API key via garrison
        #[cfg(feature = "auth")]
        {
            use crawlrs::infrastructure::database::entities::api_key;
            use garrison::protocol::apikey::ApiKeyHandler;

            let dao = crawlrs::infrastructure::auth::garrison_dao::get_garrison_dao()
                .ok_or_else(|| anyhow::anyhow!("garrison DAO not initialized"))?;
            let handler = ApiKeyHandler::new(dao);
            let api_key_id = uuid::Uuid::new_v4();
            let login_id = api_key_id.to_string();
            let scopes = vec![
                "crawlrs:read".to_string(),
                "crawlrs:write".to_string(),
                "crawlrs:admin".to_string(),
            ];
            let expires_in_secs: i64 = 365 * 24 * 3600;
            let plaintext_key = handler
                .generate_with_namespace(
                    login_id,
                    "crawlrs",
                    scopes,
                    expires_in_secs,
                )
                .await
                .map_err(|e| anyhow::anyhow!("garrison generate key: {}", e))?;

            // Extract key_id from "key_id.key_secret" format
            let garrison_key_id = match plaintext_key.split_once('.') {
                Some((k_id, _)) => k_id.to_string(),
                None => return Err(anyhow::anyhow!("garrison returned malformed key")),
            };

            // 3. Insert api_keys mapping
            let now_ts = chrono::Utc::now();
            let key_model = api_key::ActiveModel {
                id: Set(api_key_id),
                team_id: Set(team_id),
                key: Set(garrison_key_id),
                key_hash: Set(None),
                created_at: Set(now_ts.into()),
                updated_at: Set(None),
            };
            api_key::Entity::insert(key_model).exec(conn).await
                .map_err(|e| anyhow::anyhow!("insert api_key: {}", e))?;

            println!("Created admin API key:");
            println!("  api_key_id: {}", api_key_id);
            println!("  team_id:    {}", team_id);
            println!("  api_key:    {}", plaintext_key);
            println!("  scopes:     read, write, admin");
            println!();
            println!("Use this key as Bearer token:");
            println!("  curl -H 'Authorization: Bearer {}' http://localhost:8899/v1/teams/me", plaintext_key);
        }

        #[cfg(not(feature = "auth"))]
        {
            println!("Auth feature is not enabled in this build.");
            println!("Rebuild with default features to enable authentication.");
        }

        Ok(())
    }

    pub(crate) async fn run() -> anyhow::Result<()> {
        // 1. Load and configure settings
        let is_production = env::var("CRAWLRS_ENV")
            .map(|v| v.eq_ignore_ascii_case("production") || v.eq_ignore_ascii_case("prod"))
            .unwrap_or(false);

        let (settings, _port) = crawlrs::bootstrap::config::load_and_configure(is_production)?;
        let settings = Arc::new(settings);

        // 2. Initialize telemetry and metrics (inklog LoggerManager must be held alive)
        let _logger_manager = crawlrs::bootstrap::telemetry::init_all(&settings.logging)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize inklog logger: {}", e))?;

        // T056/C1 修复：移除 CRAWLRS_PROXY_URL 环境变量桥接。
        // 代理已统一由 EngineModule 构造 ProxyPool 注入 ReqwestEngine，
        // 通过 settings.proxy.urls 直接读取，无需通过环境变量中转。
        // 双重生效会导致：http_client 级别代理 + ReqwestEngine 级别代理 同时生效冲突。

        // 3. Build application state via trait-kit AsyncKit
        log::info!("Initializing application dependencies...");

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
            .map_err(|e| anyhow::anyhow!("build AsyncKit: {e}"))?;
        let app_state = CrawlRsState::from_kit(&kit)?;
        let http_client = kit
            .require::<HttpModule>()
            .map_err(|e| anyhow::anyhow!("require HttpModule: {e}"))?;

        log::info!("Application dependencies initialized successfully");

        // 4. Start service based on type
        match ServiceType::from_args() {
            ServiceType::Api => {
                start_api_service(&app_state, settings).await?;
            }
            ServiceType::Worker => {
                start_worker_service(&app_state, settings, http_client).await?;
            }
            ServiceType::Bootstrap => {
                run_bootstrap(&app_state, &settings).await?;
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app::run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_parse_service_type_api() {
        let result = parse_service_type(Some("api"));
        assert!(matches!(result, Ok(ServiceType::Api)));
    }

    #[test]
    fn tc_parse_service_type_worker() {
        let result = parse_service_type(Some("worker"));
        assert!(matches!(result, Ok(ServiceType::Worker)));
    }

    #[test]
    fn tc_parse_service_type_none_defaults_to_api() {
        let result = parse_service_type(None);
        assert!(matches!(result, Ok(ServiceType::Api)));
    }

    #[test]
    fn tc_parse_service_type_invalid_returns_error() {
        let result = parse_service_type(Some("invalid"));
        // ServiceType 未实现 Debug，使用 match 而非 unwrap_err()
        match result {
            Err(msg) => {
                assert!(msg.contains("Invalid service type"), "got: {}", msg);
                assert!(msg.contains("invalid"), "got: {}", msg);
                assert!(
                    msg.contains("api") || msg.contains("worker"),
                    "got: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected error for invalid service type, got Ok"),
        }
    }

    #[test]
    fn tc_parse_service_type_empty_string_returns_error() {
        let result = parse_service_type(Some(""));
        // ServiceType 未实现 Debug，使用 match 而非 unwrap_err()
        match result {
            Err(msg) => {
                assert!(msg.contains("Invalid service type"), "got: {}", msg);
            }
            Ok(_) => panic!("expected error for empty string, got Ok"),
        }
    }

    #[test]
    fn tc_parse_service_type_case_sensitive() {
        // Service type 解析应区分大小写
        let result = parse_service_type(Some("API"));
        assert!(result.is_err(), "uppercase API should be invalid");

        let result = parse_service_type(Some("Worker"));
        assert!(result.is_err(), "mixed-case Worker should be invalid");
    }
}
