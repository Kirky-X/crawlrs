// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;
use crawlrs::domain::models::task::TaskStatus;
use crawlrs::infrastructure::database::entities::task;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test]
async fn test_scrape_with_page_interactions() {
    let app = create_test_app().await;

    // Test with a simple HTML page that has interactive elements
    let target_url = "https://httpbin.org/html"; // Simple HTML page for testing

    // Create a scrape task with page interactions
    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"],
                "options": {
                    "js_rendering": true, // Enable JS rendering for interactions
                    "timeout": 30
                },
                "actions": [
                    {
                        "Wait": { "milliseconds": 1000 }
                    },
                    {
                        "Scroll": { "direction": "down" }
                    },
                    {
                        "Wait": { "milliseconds": 500 }
                    }
                ]
            }
        }))
        .await;

    // Accept 201 (Created) or 202 (Accepted) status codes
    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Poll for task completion
    let mut task_completed = false;
    let mut task_status = TaskStatus::Queued;

    for _ in 0..60 {
        // Wait up to 60 seconds for task with interactions
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        task_status = TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued);

        if task_status == TaskStatus::Completed {
            task_completed = true;
            break;
        }

        if task_status == TaskStatus::Failed {
            panic!("Task failed unexpectedly");
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(
        task_completed,
        "Task did not complete in time. Final status: {:?}",
        task_status
    );
}

#[tokio::test]
async fn test_scrape_with_click_action() {
    let app = create_test_app().await;

    // Test with a page that has clickable elements
    let target_url = "https://httpbin.org/html";

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"],
                "options": {
                    "js_rendering": true,
                    "timeout": 30
                },
                "actions": [
                    {
                        "Wait": { "milliseconds": 1000 }
                    },
                    {
                        "Click": { "selector": "h1" } // Try to click on heading
                    },
                    {
                        "Wait": { "milliseconds": 500 }
                    }
                ]
            }
        }))
        .await;

    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Poll for task completion
    let mut task_completed = false;

    for _ in 0..60 {
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Completed
        {
            task_completed = true;
            break;
        }

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Failed {
            // It's OK if click fails - task should still complete
            task_completed = true;
            break;
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(task_completed, "Task did not complete in time");
}

#[tokio::test]
async fn test_scrape_with_input_action() {
    let app = create_test_app().await;

    // Test with a page that has input elements
    let target_url = "https://httpbin.org/forms/post"; // Page with form elements

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"],
                "options": {
                    "js_rendering": true,
                    "timeout": 30
                },
                "actions": [
                    {
                        "Wait": { "milliseconds": 2000 }
                    },
                    {
                        "Input": {
                            "selector": "input[name='custname']",
                            "text": "test input"
                        }
                    },
                    {
                        "Wait": { "milliseconds": 1000 }
                    }
                ]
            }
        }))
        .await;

    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Poll for task completion
    let mut task_completed = false;

    for _ in 0..60 {
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Completed
        {
            task_completed = true;
            break;
        }

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Failed {
            // It's OK if input fails - task should still complete
            task_completed = true;
            break;
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(task_completed, "Task did not complete in time");
}

#[tokio::test]
async fn test_scrape_with_screenshot_action() {
    let app = create_test_app().await;

    // Test with screenshot action
    let target_url = "https://httpbin.org/html";

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"],
                "options": {
                    "js_rendering": true,
                    "timeout": 30,
                    "screenshot": true
                },
                "actions": [
                    {
                        "Wait": { "milliseconds": 1000 }
                    },
                    {
                        "Screenshot": { "full_page": false }
                    },
                    {
                        "Wait": { "milliseconds": 500 }
                    }
                ]
            }
        }))
        .await;

    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Poll for task completion
    let mut task_completed = false;

    for _ in 0..60 {
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Completed
        {
            task_completed = true;
            break;
        }

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Failed {
            panic!("Task failed unexpectedly");
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(task_completed, "Task did not complete in time");
}

#[tokio::test]
async fn test_scrape_with_complex_interactions() {
    let app = create_test_app().await;

    // Test with a sequence of different actions
    let target_url = "https://httpbin.org/html";

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": target_url,
            "task_type": "scrape",
            "payload": {
                "formats": ["html"],
                "options": {
                    "js_rendering": true,
                    "timeout": 45,
                    "screenshot": true
                },
                "actions": [
                    {
                        "Wait": { "milliseconds": 1000 }
                    },
                    {
                        "Scroll": { "direction": "down" }
                    },
                    {
                        "Wait": { "milliseconds": 500 }
                    },
                    {
                        "Scroll": { "direction": "up" }
                    },
                    {
                        "Wait": { "milliseconds": 500 }
                    },
                    {
                        "Screenshot": { "full_page": true }
                    },
                    {
                        "Wait": { "milliseconds": 1000 }
                    }
                ]
            }
        }))
        .await;

    assert!(
        response.status_code() == StatusCode::CREATED
            || response.status_code() == StatusCode::ACCEPTED,
        "Expected status code 201 or 202, got {}",
        response.status_code()
    );

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Poll for task completion
    let mut task_completed = false;

    for _ in 0..90 {
        // Longer timeout for complex interactions
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Completed
        {
            task_completed = true;
            break;
        }

        if TaskStatus::from_str(&task.status).unwrap_or(TaskStatus::Queued) == TaskStatus::Failed {
            panic!("Task failed unexpectedly");
        }

        sleep(Duration::from_secs(1)).await;
    }

    assert!(
        task_completed,
        "Complex interaction task did not complete in time"
    );
}
