// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::Duration;
use chrono::{DateTime, FixedOffset, Utc};
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::infrastructure::search::bing::BingSearchEngine;
use crawlrs::presentation::handlers::task_handler::wait_for_tasks_completion;
use migration::MigratorTrait;
use sea_orm::Database;
use std::sync::Arc;
use uuid::Uuid;

/// Integration test that demonstrates real interactions using real dependencies
#[tokio::test]
async fn test_real_task_lifecycle_with_search_integration() {
    // Setup real database
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // Run migrations
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    // Create real task repository
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    // Create real search engine
    let search_engine = BingSearchEngine::new();

    let team_id = Uuid::new_v4();

    // Test 1: Create tasks and verify they can be queried
    let task1_id = Uuid::new_v4();
    let task2_id = Uuid::new_v4();

    let task1 = Task {
        id: task1_id,
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 1,
        team_id,
        url: "https://example.com/page1".to_string(),
        payload: serde_json::json!({
            "search_query": "rust programming",
            "engine": "bing"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: DateTime::<FixedOffset>::from(Utc::now()),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: DateTime::<FixedOffset>::from(Utc::now()),
        lock_token: None,
        lock_expires_at: None,
    };

    let task2 = Task {
        id: task2_id,
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 2,
        team_id,
        url: "https://example.com/page2".to_string(),
        payload: serde_json::json!({
            "search_query": "python tutorial",
            "engine": "bing"
        }),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: DateTime::<FixedOffset>::from(Utc::now()),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: DateTime::<FixedOffset>::from(Utc::now()),
        lock_token: None,
        lock_expires_at: None,
    };

    // Save tasks to real database
    let saved_task1 = task_repo.create(&task1).await.unwrap();
    let saved_task2 = task_repo.create(&task2).await.unwrap();

    // Verify tasks were saved correctly
    assert_eq!(saved_task1.id, task1_id);
    assert_eq!(saved_task2.id, task2_id);

    // Test 2: Query tasks using real repository
    let query_params = TaskQueryParams {
        team_id,
        task_ids: Some(vec![task1_id, task2_id]),
        task_types: Some(vec![TaskType::Scrape]),
        statuses: None,
        created_after: None,
        created_before: None,
        crawl_id: None,
        limit: 10,
        offset: 0,
    };

    let (tasks, total_count) = task_repo.query_tasks(query_params).await.unwrap();
    assert_eq!(total_count, 2);
    assert_eq!(tasks.len(), 2);

    // Test 3: Update task status and test sync wait
    let mut updated_task1 = saved_task1.clone();
    updated_task1.status = TaskStatus::Completed;
    updated_task1.completed_at = Some(DateTime::<FixedOffset>::from(Utc::now()));

    let _updated = task_repo.update(&updated_task1).await.unwrap();

    // Test sync wait with real repository
    let start = std::time::Instant::now();
    let result =
        wait_for_tasks_completion(task_repo.as_ref(), &[task1_id], team_id, 2000, 500).await;
    let elapsed = start.elapsed();

    // Should complete immediately since task1 is already completed
    assert!(result.is_ok());
    assert!(elapsed.as_millis() < 1000);

    // Test 4: Test search engine parsing with real HTML structure
    let real_search_html = r#"
    <!DOCTYPE html>
    <html lang="en">
    <head><title>rust programming - Bing</title></head>
    <body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://doc.rust-lang.org/book/">The Rust Programming Language Book</a></h2>
                <div class="b_caption">
                    <p>A comprehensive guide to Rust programming with practical examples and exercises.</p>
                </div>
            </li>
            <li class="b_algo">
                <h2><a href="https://rust-lang.org/">Rust Programming Language</a></h2>
                <div class="b_caption">
                    <p>Official website for Rust programming language with documentation and resources.</p>
                </div>
            </li>
        </ol>
    </body>
    </html>
    "#;

    let search_results = search_engine
        .parse_search_results(real_search_html, "rust programming")
        .await
        .unwrap();
    assert_eq!(search_results.len(), 2);

    // Verify search results
    assert_eq!(
        search_results[0].title,
        "The Rust Programming Language Book"
    );
    assert_eq!(search_results[0].url, "https://doc.rust-lang.org/book/");
    assert_eq!(search_results[0].engine, "bing");

    assert_eq!(search_results[1].title, "Rust Programming Language");
    assert_eq!(search_results[1].url, "https://rust-lang.org/");
    assert_eq!(search_results[1].engine, "bing");
}

/// Test real error handling using real dependencies
#[tokio::test]
async fn test_real_task_error_handling() {
    // Setup real database
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // Run migrations
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    // Create real task repository
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let team_id = Uuid::new_v4();
    let non_existent_task_id = Uuid::new_v4();

    // Test sync wait with non-existent task (should timeout)
    let start = std::time::Instant::now();
    let result = wait_for_tasks_completion(
        task_repo.as_ref(),
        &[non_existent_task_id],
        team_id,
        1000, // 1 second timeout
        200,  // 200ms polling interval
    )
    .await;
    let elapsed = start.elapsed();

    // Should timeout successfully (not an error condition)
    assert!(result.is_ok());
    assert!(elapsed.as_millis() >= 1000);
}

/// Test real concurrent task processing
#[tokio::test]
async fn test_real_concurrent_task_processing() {
    // Setup real database
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // Run migrations
    migration::Migrator::up(db_pool.as_ref(), None)
        .await
        .unwrap();

    // Create real task repository
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db_pool.clone(),
        Duration::seconds(10),
    ));

    let team_id = Uuid::new_v4();
    let task_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

    // Create multiple tasks
    for (i, &task_id) in task_ids.iter().enumerate() {
        let task = Task {
            id: task_id,
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: i as i32,
            team_id,
            url: format!("https://example.com/page{}", i),
            payload: serde_json::json!({
                "page_number": i,
                "query": "concurrent test"
            }),
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: DateTime::<FixedOffset>::from(Utc::now()),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: DateTime::<FixedOffset>::from(Utc::now()),
            lock_token: None,
            lock_expires_at: None,
        };

        task_repo.create(&task).await.unwrap();
    }

    // Simulate concurrent task completion
    let task_repo_clone = task_repo.clone();
    let task_ids_clone = task_ids.clone();
    let completion_handle = tokio::spawn(async move {
        // Complete tasks with slight delays to simulate real processing
        for (i, &task_id) in task_ids_clone.iter().enumerate() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100 * i as u64)).await;

            if let Ok(Some(task)) = task_repo_clone.find_by_id(task_id).await {
                let mut updated_task = task;
                updated_task.status = TaskStatus::Completed;
                updated_task.completed_at = Some(DateTime::<FixedOffset>::from(Utc::now()));

                let _ = task_repo_clone.update(&updated_task).await;
            }
        }
    });

    // Wait for all tasks to complete
    let result = wait_for_tasks_completion(
        task_repo.as_ref(),
        &task_ids,
        team_id,
        5000, // 5 second timeout
        200,  // 200ms polling interval
    )
    .await;

    // Wait for completion task to finish
    let _ = completion_handle.await;

    // Should complete successfully
    assert!(result.is_ok());
}
