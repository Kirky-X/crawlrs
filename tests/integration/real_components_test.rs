// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::Duration;
use chrono::{DateTime, FixedOffset, Utc};
use crawlrs::config::settings::Settings;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crawlrs::domain::services::llm_service::LLMService;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use migration::MigratorTrait;
use sea_orm::Database;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Test configuration helper for real integration tests
pub struct RealTestContext {
    #[allow(dead_code)]
    pub db_pool: Arc<sea_orm::DatabaseConnection>,
    pub task_repo: Arc<TaskRepositoryImpl>,
    pub search_engine: BingSearchEngine,
    pub llm_service: LLMService,
    pub team_id: Uuid,
}

impl RealTestContext {
    pub async fn new() -> Self {
        // Setup real database
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let db_pool = Arc::new(db);

        // Run migrations
        migration::Migrator::up(db_pool.as_ref(), None)
            .await
            .unwrap();

        // Create real repositories
        let task_repo = Arc::new(TaskRepositoryImpl::new(
            db_pool.clone(),
            Duration::seconds(10),
        ));

        // Create real search engine
        let search_engine = BingSearchEngine::new();

        // Create LLM service (will fail without API key, but we can test error handling)
        let settings = Settings::new().expect("Failed to load settings");
        let llm_service = LLMService::new(&settings);

        let team_id = Uuid::new_v4();

        Self {
            db_pool,
            task_repo,
            search_engine,
            llm_service,
            team_id,
        }
    }

    pub fn create_test_task(&self, id: Uuid, status: TaskStatus, url: &str) -> Task {
        let now = Utc::now();
        let fixed_now = DateTime::<FixedOffset>::from(now);

        Task {
            id,
            task_type: TaskType::Scrape,
            status,
            priority: 0,
            team_id: self.team_id,
            url: url.to_string(),
            payload: json!({}),
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: fixed_now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: fixed_now,
            lock_token: None,
            lock_expires_at: None,
        }
    }
}

/// Test real task creation and retrieval
#[tokio::test]
async fn test_real_task_lifecycle() {
    let ctx = RealTestContext::new().await;

    let task_id = Uuid::new_v4();
    let test_task = ctx.create_test_task(task_id, TaskStatus::Queued, "https://example.com");

    // Create task
    let created_task = ctx.task_repo.create(&test_task).await.unwrap();
    assert_eq!(created_task.id, task_id);
    assert_eq!(created_task.status, TaskStatus::Queued);

    // Find task by ID
    let found_task = ctx.task_repo.find_by_id(task_id).await.unwrap().unwrap();
    assert_eq!(found_task.id, task_id);
    assert_eq!(found_task.url, "https://example.com");

    // Update task status
    let mut updated_task = found_task.clone();
    updated_task.status = TaskStatus::Active;
    updated_task.started_at = Some(DateTime::<FixedOffset>::from(Utc::now()));

    let saved_updated_task = ctx.task_repo.update(&updated_task).await.unwrap();
    assert_eq!(saved_updated_task.status, TaskStatus::Active);
    assert!(saved_updated_task.started_at.is_some());

    // Mark as completed
    ctx.task_repo.mark_completed(task_id).await.unwrap();

    // Verify completion
    let completed_task = ctx.task_repo.find_by_id(task_id).await.unwrap().unwrap();
    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert!(completed_task.completed_at.is_some());
}

/// Test real search engine functionality
#[tokio::test]
async fn test_real_search_engine_parsing() {
    let ctx = RealTestContext::new().await;

    // Test with realistic Bing HTML structure
    let realistic_html = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head><title>test query - Bing</title></head>
    <body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://example.com/result1">Test Result 1</a></h2>
                <div class="b_caption">
                    <p>This is a test result description for the first result.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://example.com</cite>
                </div>
            </li>
            <li class="b_algo">
                <h2><a href="https://example.com/result2">Test Result 2</a></h2>
                <div class="b_caption">
                    <p>This is a test result description for the second result.</p>
                </div>
                <div class="b_attribution">
                    <cite>https://example.com</cite>
                </div>
            </li>
        </ol>
    </body>
    </html>
    "#;

    let results = ctx
        .search_engine
        .parse_search_results(realistic_html, "test query")
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Test Result 1");
    assert_eq!(results[0].url, "https://example.com/result1");
    assert_eq!(results[0].engine, "bing");

    assert_eq!(results[1].title, "Test Result 2");
    assert_eq!(results[1].url, "https://example.com/result2");
    assert_eq!(results[1].engine, "bing");
}

/// Test real LLM service error handling
#[tokio::test]
async fn test_real_llm_service_error_handling() {
    let ctx = RealTestContext::new().await;

    let test_text = "The product costs $29.99 and has 4.5 stars.";
    let schema = json!({
        "type": "object",
        "properties": {
            "price": {"type": "number"},
            "rating": {"type": "number"}
        },
        "required": ["price", "rating"]
    });

    // This should fail because no API key is configured
    let result = ctx.llm_service.extract_data(test_text, &schema).await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    println!("Actual error: {}", error.to_string());

    // Check if the error message contains our expected text
    let error_string = error.to_string();
    assert!(
        error_string.contains("LLM API key not configured"),
        "Expected error to contain 'LLM API key not configured', but got: {}",
        error_string
    );
}

/// Test real task querying with multiple criteria
#[tokio::test]
async fn test_real_task_querying() {
    let ctx = RealTestContext::new().await;

    // Create multiple tasks with different statuses
    let task_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

    for (i, &task_id) in task_ids.iter().enumerate() {
        let status = match i % 3 {
            0 => TaskStatus::Queued,
            1 => TaskStatus::Active,
            _ => TaskStatus::Completed,
        };

        let task = ctx.create_test_task(task_id, status, &format!("https://example.com/page{}", i));
        ctx.task_repo.create(&task).await.unwrap();
    }

    // Query all tasks
    let all_tasks_params = TaskQueryParams {
        team_id: ctx.team_id,
        task_ids: None,
        task_types: Some(vec![TaskType::Scrape]),
        statuses: None,
        created_after: None,
        created_before: None,
        crawl_id: None,
        limit: 10,
        offset: 0,
    };

    let (all_tasks, total_count) = ctx.task_repo.query_tasks(all_tasks_params).await.unwrap();
    assert_eq!(total_count, 5);
    assert_eq!(all_tasks.len(), 5);

    // Query only completed tasks
    let completed_tasks_params = TaskQueryParams {
        team_id: ctx.team_id,
        task_ids: None,
        task_types: Some(vec![TaskType::Scrape]),
        statuses: Some(vec![TaskStatus::Completed]),
        created_after: None,
        created_before: None,
        crawl_id: None,
        limit: 10,
        offset: 0,
    };

    let (completed_tasks, completed_count) = ctx
        .task_repo
        .query_tasks(completed_tasks_params)
        .await
        .unwrap();
    assert_eq!(completed_count, 1); // Only one task should be completed
    assert_eq!(completed_tasks.len(), 1);
    assert_eq!(completed_tasks[0].status, TaskStatus::Completed);
}

/// Test real task batch operations
#[tokio::test]
async fn test_real_task_batch_operations() {
    let ctx = RealTestContext::new().await;

    // Create multiple tasks
    let task_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

    for &task_id in &task_ids {
        let task = ctx.create_test_task(task_id, TaskStatus::Queued, "https://example.com");
        ctx.task_repo.create(&task).await.unwrap();
    }

    // Batch cancel tasks
    let (cancelled_ids, errors) = ctx
        .task_repo
        .batch_cancel(task_ids.clone(), ctx.team_id, false)
        .await
        .unwrap();

    assert_eq!(cancelled_ids.len(), 3);
    assert_eq!(errors.len(), 0);

    // Verify tasks are cancelled
    for &task_id in &task_ids {
        let task = ctx.task_repo.find_by_id(task_id).await.unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Cancelled);
    }
}

/// Test real search engine cookie and URL construction
#[tokio::test]
async fn test_real_search_engine_configuration() {
    let ctx = RealTestContext::new().await;

    // Test cookie construction
    let cookies = ctx.search_engine.get_bing_cookies("en", "US");
    assert_eq!(cookies.get("_EDGE_CD"), Some(&"m=US&u=en".to_string()));
    assert_eq!(cookies.get("_EDGE_S"), Some(&"mkt=US&ui=en".to_string()));

    // Test URL construction for different pages
    let url_page_1 = ctx.search_engine.build_bing_url("rust programming", 1);
    assert!(url_page_1.contains("q=rust+programming"));
    assert!(!url_page_1.contains("first="));

    let url_page_3 = ctx.search_engine.build_bing_url("rust programming", 3);
    assert!(url_page_3.contains("first=21"));
    assert!(url_page_3.contains("FORM=PERE1"));
}
