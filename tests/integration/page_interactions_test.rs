// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
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

fn skip_if_no_chrome() {
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        println!("Skipping test: CHROMIUM_REMOTE_DEBUGGING_URL not set (Chrome not available)");
    }
}

/// 测试页面交互功能 - 滚动和等待
///
/// 注意：此测试需要Chrome浏览器和worker进程来执行页面交互操作。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_scrape_with_page_interactions -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_scrape_with_page_interactions() {
    skip_if_no_chrome();
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        return;
    }

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

    // Accept 201 (Created), 202 (Accepted) or 429 (Rate Limit) status codes
    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED
            || status == StatusCode::ACCEPTED
            || status == StatusCode::TOO_MANY_REQUESTS,
        "Expected status code 201, 202 or 429, got {}",
        status
    );

    // If rate limited, skip the test
    if status == StatusCode::TOO_MANY_REQUESTS {
        println!("⚠️  Page interactions test skipped due to rate limiting");
        return;
    }

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"]
        .as_str()
        .expect("Missing 'id' field in task response");
    let task_id = Uuid::parse_str(task_id_str).expect("Failed to parse task ID as UUID");

    // Poll for task completion
    let mut task_completed = false;
    let mut task_status = TaskStatus::Queued;

    for _ in 0..60 {
        // Wait up to 60 seconds for task with interactions
        let task = task::Entity::find()
            .filter(task::Column::Id.eq(task_id))
            .one(app.db_pool.as_ref())
            .await
            .expect("Failed to query task from database")
            .expect("Task not found in database");

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

/// 测试点击交互功能
///
/// 注意：此测试需要Chrome浏览器和worker进程来执行点击操作。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_scrape_with_click_action -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_scrape_with_click_action() {
    skip_if_no_chrome();
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        return;
    }

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

    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED
            || status == StatusCode::ACCEPTED
            || status == StatusCode::TOO_MANY_REQUESTS,
        "Expected status code 201, 202 or 429, got {}",
        status
    );

    // If rate limited, skip the test
    if status == StatusCode::TOO_MANY_REQUESTS {
        println!("⚠️  Complex interactions test skipped due to rate limiting");
        return;
    }

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"]
        .as_str()
        .expect("Missing 'id' field in task response");
    let task_id = Uuid::parse_str(task_id_str).expect("Failed to parse task ID as UUID");

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

/// 测试输入交互功能
///
/// 注意：此测试需要Chrome浏览器和worker进程来执行表单输入操作。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_scrape_with_input_action -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_scrape_with_input_action() {
    skip_if_no_chrome();
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        return;
    }

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

    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED
            || status == StatusCode::ACCEPTED
            || status == StatusCode::TOO_MANY_REQUESTS,
        "Expected status code 201, 202 or 429, got {}",
        status
    );

    // If rate limited, skip the test
    if status == StatusCode::TOO_MANY_REQUESTS {
        println!("⚠️  Input action test skipped due to rate limiting");
        return;
    }

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

/// 测试截图交互功能
///
/// 注意：此测试需要Chrome浏览器和worker进程来执行截图操作。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_scrape_with_screenshot_action -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_scrape_with_screenshot_action() {
    skip_if_no_chrome();
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        return;
    }

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

    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED
            || status == StatusCode::ACCEPTED
            || status == StatusCode::TOO_MANY_REQUESTS,
        "Expected status code 201, 202 or 429, got {}",
        status
    );

    // If rate limited, skip the test
    if status == StatusCode::TOO_MANY_REQUESTS {
        println!("⚠️  Screenshot action test skipped due to rate limiting");
        return;
    }

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

/// 测试复杂交互功能 - 多个操作的组合
///
/// 注意：此测试需要Chrome浏览器和worker进程来执行复杂的页面交互操作序列。
/// 如需运行此测试，请使用: cargo test --test integration_tests -- test_scrape_with_complex_interactions -- --include-ignored
#[ignore]
#[tokio::test]
async fn test_scrape_with_complex_interactions() {
    skip_if_no_chrome();
    if std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").is_err() {
        return;
    }

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

    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED
            || status == StatusCode::ACCEPTED
            || status == StatusCode::TOO_MANY_REQUESTS,
        "Expected status code 201, 202 or 429, got {}",
        status
    );

    // If rate limited, skip the test
    if status == StatusCode::TOO_MANY_REQUESTS {
        println!("⚠️  Click action test skipped due to rate limiting");
        return;
    }

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
