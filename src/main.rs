// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crawlrs::bootstrap::config;
use crawlrs::bootstrap::engines;
use crawlrs::bootstrap::infrastructure;
use crawlrs::bootstrap::routes;
use crawlrs::bootstrap::services;
use crawlrs::bootstrap::telemetry;
use crawlrs::config::settings::Settings;
use crawlrs::utils::retry_policy::RetryPolicy;
use crawlrs::workers::backlog_worker::BacklogWorker;
use crawlrs::workers::manager::WorkerManager;
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
///
/// # Arguments
///
/// * `settings` - Application settings
/// * `infrastructure` - Initialized infrastructure components
/// * `services` - Initialized services
async fn start_api_service(
    settings: Arc<Settings>,
    infrastructure: infrastructure::InfrastructureComponents,
    services: services::ServicesComponents,
) -> anyhow::Result<()> {
    tracing::info!("Starting API service...");

    // Start webhook worker
    let webhook_processor = Arc::new(WebhookWorker::new(
        infrastructure.repositories.webhook_event_repo.clone(),
        services.webhook_service.clone(),
        RetryPolicy::default(),
    ));
    let webhook_worker = AbstractWorker::new(webhook_processor, std::time::Duration::from_secs(5));
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // Start backlog worker
    let backlog_processor = Arc::new(BacklogWorker::new(
        infrastructure.repositories.tasks_backlog_repo.clone(),
        infrastructure.repositories.task_repo.clone(),
        services.rate_limiting_service.clone(),
        settings.concurrency.default_team_limit as usize,
    ));
    let backlog_worker = AbstractWorker::new(backlog_processor, std::time::Duration::from_secs(30));
    tokio::spawn(async move {
        backlog_worker.run().await;
    });

    // Build and configure the API application
    let app = routes::build_api_app(&infrastructure, &services, &settings);

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
///
/// # Arguments
///
/// * `settings` - Application settings
/// * `infrastructure` - Initialized infrastructure components
/// * `services` - Initialized services
/// * `engine_components` - Initialized engine components
async fn start_worker_service(
    settings: Arc<Settings>,
    infrastructure: infrastructure::InfrastructureComponents,
    services: services::ServicesComponents,
    engine_components: engines::EngineComponents,
) -> anyhow::Result<()> {
    tracing::info!("Starting Worker service...");

    // Start webhook worker
    let webhook_processor = Arc::new(WebhookWorker::new(
        infrastructure.repositories.webhook_event_repo.clone(),
        services.webhook_service.clone(),
        RetryPolicy::default(),
    ));
    let webhook_worker = AbstractWorker::new(webhook_processor, std::time::Duration::from_secs(5));
    tokio::spawn(async move {
        webhook_worker.run().await;
    });

    // Create and start worker manager
    let mut worker_manager = WorkerManager::new(
        services.queue.clone(),
        infrastructure.repositories.task_repo.clone(),
        infrastructure.repositories.result_repo.clone(),
        infrastructure.repositories.crawl_repo.clone(),
        infrastructure.storage_repo.clone(),
        infrastructure.repositories.webhook_event_repo.clone(),
        infrastructure.repositories.credits_repo.clone(),
        engine_components.engine_client.clone(),
        services.create_scrape_use_case.clone(),
        (*infrastructure.redis_client).clone(),
        services.robots_checker.clone(),
        settings.clone(),
        settings.concurrency.default_team_limit as usize,
    );

    // Start workers
    let worker_count = settings.workers.count.resolve();
    tracing::info!("Starting {} worker(s)", worker_count);
    worker_manager.start_workers(worker_count).await;

    // Start backlog worker
    let backlog_processor = Arc::new(BacklogWorker::new(
        infrastructure.repositories.tasks_backlog_repo.clone(),
        infrastructure.repositories.task_repo.clone(),
        services.rate_limiting_service.clone(),
        settings.concurrency.default_team_limit as usize,
    ));
    let backlog_worker = AbstractWorker::new(backlog_processor, std::time::Duration::from_secs(30));
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

    let (settings, _port) = config::load_and_configure(is_production)?;
    let settings = Arc::new(settings);

    // 2. Initialize telemetry and metrics
    telemetry::init_all(&settings.logging);

    // 3. Set proxy environment variables if enabled
    if settings.proxy.enabled {
        env::set_var("CRAWLRS_PROXY_URL", settings.proxy.url());
        tracing::info!("HTTP proxy enabled (credentials hidden)");
    }

    // 4. Initialize infrastructure (database, redis, repositories)
    let infrastructure = infrastructure::init_infrastructure(&settings).await?;

    // 5. Initialize engines
    let engine_components =
        engines::init_engine_components(&settings.proxy.url(), &settings.engines);

    // 6. Initialize services
    let services =
        services::init_services(&infrastructure, engine_components.router.clone(), &settings);

    // 7. Start service based on type
    match ServiceType::from_args() {
        ServiceType::Api => {
            start_api_service(settings, infrastructure, services).await?;
        }
        ServiceType::Worker => {
            start_worker_service(settings, infrastructure, services, engine_components).await?;
        }
    }

    Ok(())
}
