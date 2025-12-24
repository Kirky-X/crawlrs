// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::{routing::get, Router};
use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use crawlrs::domain::services::crawl_service::CrawlService;
use tokio::net::TcpListener;
use uuid::Uuid;

#[tokio::test]
async fn test_process_crawl_result_creates_tasks_integration() {
    let app = create_test_app().await;

    // 1. Setup Local Server for Robots.txt
    let app_router =
        Router::new().route("/robots.txt", get(|| async { "User-agent: *\nAllow: /" }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app_router).await.unwrap();
    });

    // 2. Initialize Service with Real Repository from App
    // Note: RobotsChecker will be instantiated internally by CrawlService::new() if we use that.
    // It uses reqwest::Client::new(), which follows standard DNS.
    // Our local server is on 127.0.0.1, which is accessible.

    let service = CrawlService::new(app.task_repo.clone());

    let link_url = format!("{}/page1", base_url);

    // 3. Create Parent Task
    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id: app.api_key.parse().unwrap_or(Uuid::new_v4()),
        url: base_url.clone(),
        payload: serde_json::json!({
            "depth": 0,
            "max_depth": 3,
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: Some(Utc::now().into()),
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    let html = format!(r#"<a href="{}">Link</a>"#, link_url);

    // 4. Process
    // Disable SSRF protection for local test
    std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");

    let created_tasks = service
        .process_crawl_result(&parent_task, &html)
        .await
        .expect("Process failed");

    std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");

    // 5. Verify
    assert_eq!(created_tasks.len(), 1);
    assert_eq!(created_tasks[0].url, link_url);

    // Verify in DB
    let saved_task = app
        .task_repo
        .find_by_id(created_tasks[0].id)
        .await
        .expect("DB check failed")
        .expect("Task not found in DB");
    assert_eq!(saved_task.url, link_url);
}

#[tokio::test]
async fn test_process_crawl_result_respects_domain_blacklist() {
    let app = create_test_app().await;

    // 1. Setup Local Server for Robots.txt
    let app_router =
        Router::new().route("/robots.txt", get(|| async { "User-agent: *\nAllow: /" }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app_router).await.unwrap();
    });

    let service = CrawlService::new(app.task_repo.clone());

    let allowed_url = format!("{}/allowed", base_url);
    let blocked_url = "http://malicious.com/bad";

    // 3. Create Parent Task with Domain Blacklist
    let parent_task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        team_id: app.api_key.parse().unwrap_or(Uuid::new_v4()),
        url: base_url.clone(),
        payload: serde_json::json!({
            "depth": 0,
            "max_depth": 3,
            "domain_blacklist": ["malicious.com"]
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: Some(Utc::now().into()),
        completed_at: None,
        crawl_id: Some(Uuid::new_v4()),
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    };

    let html = format!(
        r#"<a href="{}">Allowed Link</a><a href="{}">Blocked Link</a>"#,
        allowed_url, blocked_url
    );

    // 4. Process
    // Disable SSRF protection for local test
    std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");

    let created_tasks = service
        .process_crawl_result(&parent_task, &html)
        .await
        .expect("Process failed");

    std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");

    // 5. Verify
    assert_eq!(created_tasks.len(), 1);
    assert_eq!(created_tasks[0].url, allowed_url);

    // Verify the blocked URL was NOT created
    // Note: We don't need to check the DB for the blocked URL because process_crawl_result only returns created tasks.
    // If it's not in created_tasks, it wasn't created.
}
