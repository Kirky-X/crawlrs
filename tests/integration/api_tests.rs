// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::{create_test_app, create_test_app_with_rate_limit_options};
use crawlrs::utils::telemetry::init_telemetry;
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
            "url": "https://example.com"
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
    let app = create_test_app_with_rate_limit_options(true, true).await;

    // The rate limiter is configured to 100 RPM in tests.
    // We send 101 requests to ensure the limit is triggered.
    for _ in 0..101 {
        let _ = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": "https://example.com"
            }))
            .await;
    }

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::TOO_MANY_REQUESTS);
}

/// 测试创建抓取任务时的参数验证
///
/// 验证API对无效参数的验证和错误响应格式
#[tokio::test]
async fn test_create_scrape_task_validation() {
    let app = create_test_app().await;

    // 测试缺少URL参数
    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({}))
        .await;

    assert_eq!(response.status_code(), StatusCode::UNPROCESSABLE_ENTITY);

    // 测试无效URL格式
    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "not-a-valid-url"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// 测试搜索功能
///
/// 验证/v1/search端点的基本功能
#[tokio::test]
async fn test_search_basic() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/search")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "query": "rust programming",
            "sources": ["web"],
            "limit": 10
        }))
        .await;

    println!("Search response status: {}", response.status_code());
    println!("Search response body: {}", response.text());

    assert_eq!(response.status_code(), StatusCode::OK);
    
    let search_response: serde_json::Value = response.json();
    assert!(search_response.get("data").is_some());
    assert!(search_response["data"].get("web").is_some());
}

/// 测试爬取功能
///
/// 验证/v1/crawl端点的基本功能
#[tokio::test]
async fn test_crawl_basic() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "config": {
                "max_depth": 2
            }
        }))
        .await;

    println!("Crawl response status: {}", response.status_code());
    println!("Crawl response body: {}", response.text());

    assert_eq!(response.status_code(), StatusCode::CREATED);
    
    let crawl_response: serde_json::Value = response.json();
    assert!(crawl_response.get("id").is_some());
    assert!(crawl_response.get("status").is_some());
}

/// 测试提取功能
///
/// 验证/v1/extract端点的基本功能
#[tokio::test]
async fn test_extract_basic() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/extract")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": ["https://example.com/product"],
            "prompt": "Extract product name, price, and availability"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::ACCEPTED);
    
    let extract_response: serde_json::Value = response.json();
    assert!(extract_response.get("id").is_some());
    assert!(extract_response.get("status").is_some());
}

/// 测试任务状态查询
///
/// 验证/v1/scrape/:id端点的任务状态查询功能
#[tokio::test]
async fn test_get_task_status() {
    let app = create_test_app().await;

    // 首先创建一个任务
    let create_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::CREATED);
    let task_response: serde_json::Value = create_response.json();
    let task_id = task_response["id"].as_str().unwrap();

    // 查询任务状态
    let status_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(status_response.status_code(), StatusCode::OK);
    
    let status_data: serde_json::Value = status_response.json();
    assert_eq!(status_data["id"].as_str().unwrap(), task_id);
    assert!(status_data.get("status").is_some());
}

/// 测试任务取消功能
///
/// 验证DELETE /v1/scrape/:id端点的任务取消功能
#[tokio::test]
async fn test_cancel_task() {
    let app = create_test_app().await;

    // 首先创建一个任务
    let create_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com"
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::CREATED);
    let task_response: serde_json::Value = create_response.json();
    let task_id = task_response["id"].as_str().unwrap();

    // 取消任务
    let cancel_response = app
        .server
        .delete(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(cancel_response.status_code(), StatusCode::NO_CONTENT);
}

/// 测试爬取取消功能
///
/// 验证DELETE /v1/crawl/:id端点的爬取取消功能
#[tokio::test]
async fn test_cancel_crawl() {
    let app = create_test_app().await;

    // 首先创建一个爬取任务
    let create_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "config": {
                "max_depth": 2
            }
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::CREATED);
    let crawl_response: serde_json::Value = create_response.json();
    let crawl_id = crawl_response["id"].as_str().unwrap();

    // 取消爬取
    let cancel_response = app
        .server
        .delete(&format!("/v1/crawl/{}", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(cancel_response.status_code(), StatusCode::NO_CONTENT);
}

/// 测试SSRF防护
///
/// 验证系统对内部网络地址的防护机制
#[tokio::test]
async fn test_ssrf_protection() {
    let app = create_test_app().await;

    // 测试内网地址
    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://192.168.1.1"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    
    let error_response: serde_json::Value = response.json();
    assert!(error_response["error"].as_str().unwrap().contains("SSRF"));
}

/// 测试认证失败
///
/// 验证无效API密钥的处理
#[tokio::test]
async fn test_invalid_api_key() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", "Bearer invalid-api-key")
        .json(&json!({
            "url": "https://example.com"
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);
}

/// 测试缺少认证头
///
/// 验证未提供认证信息的处理
#[tokio::test]
async fn test_missing_auth_header() {
    let app = create_test_app().await;

    let response = app
        .server
        .post("/v1/scrape")
        .json(&json!({
            "url": "https://example.com",
            "task_type": "scrape",
            "payload": {}
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);
}

/// 测试健康检查端点
///
/// 验证/health端点的基本功能
#[tokio::test]
async fn test_health_check() {
    let app = create_test_app().await;

    let response = app
        .server
        .get("/health")
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    
    let health_response: serde_json::Value = response.json();
    assert_eq!(health_response["status"].as_str().unwrap(), "healthy");
}

/// 测试指标端点
///
/// 验证/metrics端点的基本功能
#[tokio::test]
async fn test_metrics_endpoint() {
    // Initialize telemetry for debugging
    init_telemetry();
    
    let app = create_test_app().await;

    let response = app
        .server
        .get("/metrics")
        .await;

    println!("Metrics response status: {}", response.status_code());
    println!("Metrics response body: {}", response.text());

    assert_eq!(response.status_code(), StatusCode::OK);
    assert!(response.text().contains("# HELP"));
}