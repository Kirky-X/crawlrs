// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::bootstrap::routes::build_api_app_with_state;
use crawlrs::di::{AppState, AppStateExt};
use crawlrs::utils::retry_policy::RetryPolicy;
use crawlrs::workers::backlog_worker::BacklogWorker;
use crawlrs::workers::manager::{WorkerManager, WorkerManagerConfig};
use crawlrs::workers::webhook_worker::WebhookWorker;
use crawlrs::workers::{AbstractWorker, Worker};
use std::sync::Arc;
use std::{env, process};
use tokio::net::TcpListener;
use tracing::error;

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
    tracing::info!("Starting API service...");

    // Start webhook worker
    let webhook_processor = Arc::new(WebhookWorker::new(
        app_state.webhook_event_repo(),
        app_state.webhook_service(),
        RetryPolicy::default(),
    ));
    let webhook_worker = AbstractWorker::new(webhook_processor, std::time::Duration::from_secs(5));
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // Start backlog worker
    let backlog_processor = Arc::new(BacklogWorker::new(
        app_state.tasks_backlog_repo(),
        app_state.task_repo(),
        app_state.rate_limiting_service(),
        settings.concurrency.default_team_limit as usize,
    ));
    let backlog_worker = AbstractWorker::new(
        backlog_processor,
        std::time::Duration::from_secs(settings.timeouts.workers.backlog_interval_seconds),
    );
    tokio::spawn(async move {
        backlog_worker.run().await;
    });

    // Build API app with dependencies
    let app = build_api_app_with_state(app_state, settings.clone());

    // Start the server
    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

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
    tracing::info!("Starting Worker service...");

    // Start webhook worker
    let webhook_processor = Arc::new(WebhookWorker::new(
        app_state.webhook_event_repo(),
        app_state.webhook_service(),
        RetryPolicy::default(),
    ));
    let webhook_worker = AbstractWorker::new(webhook_processor, std::time::Duration::from_secs(5));
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // Create worker manager with dependencies (使用 DI 注入的服务)
    let deps = crawlrs::workers::manager::WorkerManagerDeps {
        queue: app_state.task_queue(),
        repository: app_state.task_repo(),
        result_repository: app_state.result_repo(),
        crawl_repository: app_state.crawl_repo(),
        storage_repository: Some(app_state.storage_repo()),
        webhook_event_repository: app_state.webhook_event_repo(),
        credits_repository: app_state.credits_repo(),
        engine_client: app_state.engine_client(),
        create_scrape_use_case: Arc::new(
            crawlrs::application::use_cases::create_scrape::CreateScrapeUseCase::new(
                app_state.engine_client.clone(),
            ),
        ),
        redis: (*app_state.redis_client).clone(),
        robots_checker: app_state.robots_checker.clone(),
        http_client,
        llm_service: (*app_state.llm_service()).clone(),
        regex_cache: (*app_state.regex_cache()).clone(),
    };

    let config = WorkerManagerConfig {
        settings: settings.clone(),
        default_concurrency_limit: settings.concurrency.default_team_limit as usize,
    };

    let mut worker_manager = WorkerManager::new(deps, config);

    // Start workers
    let worker_count = settings.workers.count.resolve();
    tracing::info!("Starting {} worker(s)", worker_count);
    worker_manager.start_workers(worker_count).await;

    // Start backlog worker
    let backlog_processor = Arc::new(BacklogWorker::new(
        app_state.tasks_backlog_repo(),
        app_state.task_repo(),
        app_state.rate_limiting_service(),
        settings.concurrency.default_team_limit as usize,
    ));
    let backlog_worker = AbstractWorker::new(
        backlog_processor,
        std::time::Duration::from_secs(settings.timeouts.workers.backlog_interval_seconds),
    );
    tokio::spawn(async move {
        backlog_worker.run().await;
    });

    // Wait for shutdown signal
    worker_manager.wait_for_shutdown().await;

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

    // 2. Initialize telemetry and metrics
    crawlrs::bootstrap::telemetry::init_all(&settings.logging);

    // 3. Set proxy environment variables if enabled
    if settings.proxy.enabled {
        env::set_var("CRAWLRS_PROXY_URL", settings.proxy.url());
        tracing::info!("HTTP proxy enabled (credentials hidden)");
    }

    // 4. Create application state using bootstrap infrastructure
    tracing::info!("Initializing application dependencies...");

    // Initialize infrastructure first (needed for http_client)
    let infrastructure = crawlrs::bootstrap::infrastructure::init_infrastructure(&settings).await?;
    let http_client = infrastructure.http_client.clone();

    // Initialize engines (needed by services)
    let engines = crawlrs::bootstrap::engines::init_engine_components(
        http_client.clone(),
        &settings.proxy.url(),
        &settings.engines,
    );

    // Initialize services (needs engines and http_client)
    let services = crawlrs::bootstrap::services::init_services(
        &infrastructure,
        engines.router.clone(),
        engines.engine_client.clone(),
        http_client.clone(),
        &settings,
    );

    // Create SearchClient for AppState
    let search_client = Arc::new(crawlrs::search::client::SearchClient::new(engines.engine_client.clone()));

    // Create AppState using struct literal
    let app_state = AppState {
            db: infrastructure.db,
            redis_client: infrastructure.redis_client,
            task_repo: infrastructure.repositories.task_repo,
            credits_repo: infrastructure.repositories.credits_repo,
            crawl_repo: infrastructure.repositories.crawl_repo,
            result_repo: infrastructure.repositories.result_repo,
            webhook_repo: infrastructure.repositories.webhook_repo,
            webhook_event_repo: infrastructure.repositories.webhook_event_repo,
            tasks_backlog_repo: infrastructure.repositories.tasks_backlog_repo,
            storage_repo: infrastructure.storage_repo.unwrap_or_else(|| {
                Arc::new(crawlrs::domain::repositories::storage_repository::NoOpStorage)
            }),
            task_queue: services.queue,
            rate_limiting_service: services.rate_limiting_service,
            team_service: services.team_service,
            webhook_service: services.webhook_service,
            robots_checker: services.robots_checker,
            team_semaphore: services.team_semaphore,
            engine_router: engines.router,
            engine_client: engines.engine_client,
            search_client,
            search_service: services.search_service,
            auth_scope_service: services.auth_scope_service,
            audit_service: services.audit_service,
            llm_service: services.llm_service,
            regex_cache: services.regex_cache,
        };

    tracing::info!("Application dependencies initialized successfully");

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
