// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::bootstrap::routes::build_api_app_with_state;
use crawlrs::di::modules::{
    CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule, RepositoryModule,
    ServiceModule, SettingsModule,
};
use crawlrs::di::{AppState, AppStateExt};
use crawlrs::workers::manager::{WorkerManager, WorkerManagerConfig};
use crawlrs::workers::{AbstractWorker, Worker};
use log::error;
use std::sync::Arc;
use std::{env, process};
use tokio::net::TcpListener;
use trait_kit::AsyncKit;

/// Service type enumeration.
enum ServiceType {
    Api,
    Worker,
}

impl ServiceType {
    /// Parse service type from command line arguments.
    fn from_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let service_type = args.get(1).map(String::as_str).unwrap_or("api");

        match service_type {
            "api" => ServiceType::Api,
            "worker" => ServiceType::Worker,
            _ => {
                error!(
                    "Invalid service type: '{}'. Use 'api' or 'worker'.",
                    service_type
                );
                process::exit(1);
            }
        }
    }
}

/// Start the API service.
async fn start_api_service(
    app_state: &AppState,
    settings: Arc<crawlrs::config::settings::Settings>,
) -> anyhow::Result<()> {
    log::info!("Starting API service...");

    // Start webhook worker
    let webhook_worker = AbstractWorker::new(
        app_state.webhook_worker(),
        std::time::Duration::from_secs(5),
    );
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // Start backlog worker
    let backlog_worker = AbstractWorker::new(
        app_state.backlog_worker(),
        std::time::Duration::from_secs(settings.timeouts.workers.backlog_interval_seconds),
    );
    tokio::spawn(async move {
        backlog_worker.run().await;
    });

    // Start expiration worker
    let expiration_worker = AbstractWorker::new(
        app_state.expiration_worker(),
        std::time::Duration::from_secs(3600), // Run every hour
    );
    tokio::spawn(async move {
        expiration_worker.run().await;
    });

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
    app_state: &AppState,
    settings: Arc<crawlrs::config::settings::Settings>,
    http_client: Arc<reqwest::Client>,
) -> anyhow::Result<()> {
    log::info!("Starting Worker service...");

    // Start webhook worker
    let webhook_worker = AbstractWorker::new(
        app_state.webhook_worker(),
        std::time::Duration::from_secs(5),
    );
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

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
        redis: (*app_state.redis_client).clone(),
        robots_checker: app_state.robots_checker.clone(),
        http_client,
        extraction_service: app_state.extraction_service(),
        regex_cache: (*app_state.regex_cache()).clone(),
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

    // Start backlog worker
    let backlog_worker = AbstractWorker::new(
        app_state.backlog_worker(),
        std::time::Duration::from_secs(settings.timeouts.workers.backlog_interval_seconds),
    );
    tokio::spawn(async move {
        backlog_worker.run().await;
    });

    // Start expiration worker
    let expiration_worker = AbstractWorker::new(
        app_state.expiration_worker(),
        std::time::Duration::from_secs(3600), // Run every hour
    );
    tokio::spawn(async move {
        expiration_worker.run().await;
    });

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    log::info!("Shutting down worker service...");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    // 3. Set proxy environment variables if enabled
    if settings.proxy.enabled {
        env::set_var("CRAWLRS_PROXY_URL", settings.proxy.url());
        log::info!("HTTP proxy enabled (credentials hidden)");
    }

    // 4. Build application state via trait-kit AsyncKit
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
    let app_state = AppState::from_kit(&kit)?;
    let http_client = kit
        .require::<HttpModule>()
        .map_err(|e| anyhow::anyhow!("require HttpModule: {e}"))?;

    log::info!("Application dependencies initialized successfully");

    // 5. Start service based on type
    match ServiceType::from_args() {
        ServiceType::Api => {
            start_api_service(&app_state, settings).await?;
        }
        ServiceType::Worker => {
            start_worker_service(&app_state, settings, http_client).await?;
        }
    }

    Ok(())
}
