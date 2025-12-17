// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;
use crawlrs::domain::models::task::{TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;

#[tokio::test]
async fn test_create_scrape_handler_real_queue() {
    let app = create_test_app().await;

    let payload = serde_json::json!({
        "url": "https://example.com",
        "options": {
            "timeout": 30,
            "screenshot": true
        }
    });

    let response = app
        .server
        .post("/v1/scrape")
        .json(&payload)
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    response.assert_status(StatusCode::CREATED);
    let json_response = response.json::<serde_json::Value>();
    assert_eq!(json_response["success"], true);

    let task_id_str = json_response["id"]
        .as_str()
        .expect("Task ID should be string");
    let task_id = uuid::Uuid::parse_str(task_id_str).expect("Valid UUID");

    let task = app
        .task_repo
        .find_by_id(task_id)
        .await
        .expect("DB error")
        .expect("Task should exist");
    assert_eq!(task.url, "https://example.com");
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.task_type, TaskType::Scrape);
}
