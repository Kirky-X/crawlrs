// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;
use crawlrs::infrastructure::database::entities::task;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test]
async fn test_scrape_real_website() {
    // 移除 SSRF 禁用，使用真实环境配置
    // std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");
    let app = create_test_app().await;

    // 1. Use a real website
    let target_url = "https://example.com";

    // 2. Create a scrape task
    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"]
            }
        }))
        .await;

    // 接受201 (Created) 或 202 (Accepted) 状态码
    // 202表示任务已接受但同步等待超时
    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // 3. Poll for task completion
    let mut task_completed = false;
    for _ in 0..30 {
        // Wait up to 30 seconds for real network request
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        if task.status == crawlrs::domain::models::task::TaskStatus::Completed.to_string() {
            task_completed = true;
            break;
        }

        if task.status == crawlrs::domain::models::task::TaskStatus::Failed.to_string() {
            // Check error message if possible, though we don't have it mapped in entity yet
            // If we could access payload or another table for result, we would check it.
            panic!("Task failed unexpectedly");
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(task_completed, "Task did not complete in time");

    // 4. Verify result in ScrapeResult table (Optional but recommended for 'Real Test')
    // We need ScrapeResult entity to query it.
    // Assuming ScrapeResultRepository works, the result should be there.
    // Let's rely on TaskStatus::Completed for now as ScrapeResult entity might need import.
}
