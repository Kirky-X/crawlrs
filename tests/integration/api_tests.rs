// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;
use crawlrs::infrastructure::database::entities::task;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;

use uuid::Uuid;

/// 测试成功创建抓取任务
///
/// 验证当提供有效的负载和API密钥时，/v1/scrape端点能否成功创建一个新的抓取任务。
///
/// 对应文档章节：3.1.1
#[tokio::test]
async fn test_create_scrape_task_success() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "task_type": "scrape",
            "payload": {}
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::CREATED);

    let task_response: serde_json::Value = response.json();
    let task_id_str = task_response["id"].as_str().unwrap();
    let task_id = Uuid::parse_str(task_id_str).unwrap();

    // Verify the task was created in the database
    let task = task::Entity::find()
        .filter(task::Column::Id.eq(task_id))
        .one(app.db_pool.as_ref())
        .await
        .unwrap();

    assert!(task.is_some());
    let task = task.unwrap();
    assert_eq!(task.url, "https://example.com");
}

/// 测试抓取速率限制
///
/// 验证API是否对超出限制的请求强制执行速率限制。
///
/// 对应文档章节：3.1.2
#[tokio::test]
async fn test_scrape_rate_limit() {
    let app = create_test_app().await;

    // The rate limiter is configured to 100 RPM in tests.
    // We send 101 requests to ensure the limit is triggered.
    for _ in 0..101 {
        let _ = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": "https://example.com",
                "task_type": "scrape",
                "payload": {}
            }))
            .await;
    }

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "task_type": "scrape",
            "payload": {}
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::TOO_MANY_REQUESTS);
}
