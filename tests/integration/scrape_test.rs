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
    let app = create_test_app().await;

    let target_url = "https://example.com";

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

    if response.status_code() != StatusCode::CREATED
        && response.status_code() != StatusCode::ACCEPTED
    {
        panic!(
            "Failed to create task. Status: {}, Body: {:?}",
            response.status_code(),
            response.json::<serde_json::Value>()
        );
    }

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    let mut task_completed = false;
    let mut last_status = String::new();
    let mut consecutive_failures = 0;

    for i in 0..45 {
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        last_status = task.status.clone();

        if last_status == crawlrs::domain::models::task::TaskStatus::Completed.to_string() {
            task_completed = true;
            break;
        }

        if last_status == crawlrs::domain::models::task::TaskStatus::Failed.to_string() {
            consecutive_failures += 1;
            if consecutive_failures >= 3 {
                panic!("Task failed after {} status checks. Last status: {}. This might be due to network issues or SSRF protection.", i, last_status);
            }
        } else {
            consecutive_failures = 0;
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(
        task_completed,
        "Task did not complete in time (45s). Last status: {}",
        last_status
    );
}
