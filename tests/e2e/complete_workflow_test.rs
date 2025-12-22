// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 完整工作流端到端测试
///
/// 测试从创建抓取任务到获取结果的完整流程
/// 验证系统的核心功能是否正常集成和工作
use crate::integration::helpers::create_test_app;
use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_complete_scrape_workflow() {
    let app = create_test_app().await;

    // Step 1: Create a scrape task
    let create_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "options": {
                "wait_for": 1000,
                "timeout": 30
            }
        }))
        .await;

    if create_response.status_code() != StatusCode::CREATED {
        println!("Response status: {}", create_response.status_code());
        println!("Response body: {}", create_response.text());
        panic!(
            "Expected 201 CREATED, got {}",
            create_response.status_code()
        );
    }

    let task_data: serde_json::Value = create_response.json();
    let task_id = task_data["id"].as_str().unwrap();

    // Check initial status
    let initial_status_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(initial_status_response.status_code(), StatusCode::OK);
    let initial_data: serde_json::Value = initial_status_response.json();
    println!("Initial task status: {}", initial_data["status"]);

    // Step 2: Monitor task status until completion
    let mut status = String::from("queued");
    let mut retries = 0;
    const MAX_RETRIES: u32 = 60; // Increase timeout to 60 seconds

    while status == "queued" || status == "active" {
        if retries >= MAX_RETRIES {
            println!(
                "Task {} still in status '{}' after {} retries",
                task_id, status, MAX_RETRIES
            );
            panic!("Task did not complete within expected time");
        }

        // Add small delay before polling to allow database updates to propagate
        if retries > 0 {
            sleep(Duration::from_millis(500)).await;
        }

        let status_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;

        assert_eq!(status_response.status_code(), StatusCode::OK);
        let status_data: serde_json::Value = status_response.json();
        status = status_data["status"].as_str().unwrap().to_string();

        println!("Task status: {}, retries: {}", status, retries);
        if let Some(error) = status_data.get("error") {
            println!("Task error: {}", error);
        }
        if let Some(result) = status_data.get("result") {
            if let Some(error_msg) = result.get("error") {
                println!("Task result error: {}", error_msg);
            }
        }

        if status == "completed" || status == "failed" {
            println!("Task {} reached final status: {}", task_id, status);
            break;
        }

        sleep(Duration::from_millis(1000)).await;
        retries += 1;

        // Add extra delay when task has been active for a while
        if retries > 10 && status == "active" {
            println!(
                "Task {} still active after {} retries, waiting extra time...",
                task_id, retries
            );
            sleep(Duration::from_millis(3000)).await;
        }
    }

    assert_eq!(status, "completed", "Task should complete successfully");

    // Step 3: Verify task results
    let final_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(final_response.status_code(), StatusCode::OK);
    let final_data: serde_json::Value = final_response.json();

    assert!(final_data["result"].is_object());
    assert!(final_data["result"]["content"].is_string());
    assert!(final_data["result"]["content"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn test_batch_scrape_workflow() {
    let app = create_test_app().await;

    // Step 1: Create multiple scrape tasks
    let urls = vec![
        "https://example.com",
        "https://httpbin.org/html",
        "https://httpbin.org/json",
    ];

    let mut task_ids = Vec::new();

    for url in urls {
        let create_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": url,
                "options": {
                    "wait_for": 1000,
                    "timeout": 20
                }
            }))
            .await;

        assert_eq!(create_response.status_code(), StatusCode::CREATED);
        let task_data: serde_json::Value = create_response.json();
        task_ids.push(task_data["id"].as_str().unwrap().to_string());
    }

    // Step 2: Monitor all tasks until completion
    let mut all_completed = false;
    let mut retries = 0;
    const MAX_RETRIES: u32 = 60;

    while !all_completed && retries < MAX_RETRIES {
        all_completed = true;

        for task_id in &task_ids {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;

            assert_eq!(status_response.status_code(), StatusCode::OK);
            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"].as_str().unwrap().to_string();

            if status == "queued" || status == "active" {
                all_completed = false;
            } else if status == "failed" {
                panic!("Task {} failed", task_id);
            }
        }

        if !all_completed {
            sleep(Duration::from_millis(1000)).await;
            retries += 1;
        }
    }

    assert!(
        all_completed,
        "All tasks should complete within expected time"
    );

    // Step 3: Verify all results
    for task_id in &task_ids {
        let final_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;

        assert_eq!(final_response.status_code(), StatusCode::OK);
        let final_data: serde_json::Value = final_response.json();

        assert_eq!(final_data["status"], "completed");
        assert!(final_data["result"]["content"].as_str().unwrap().len() > 0);
    }
}

#[tokio::test]
async fn test_crawl_with_webhook_workflow() {
    let app = create_test_app().await;

    // Step 1: Create a crawl task with webhook
    let webhook_url = "https://httpbin.org/post";

    let create_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": ["https://example.com"],
            "webhook": webhook_url,
            "options": {
                "wait_for": 1000,
                "timeout": 30
            }
        }))
        .await;

    assert_eq!(create_response.status_code(), StatusCode::CREATED);
    let crawl_data: serde_json::Value = create_response.json();
    let crawl_id = crawl_data["id"].as_str().unwrap();

    // Step 2: Monitor crawl status
    let mut status = String::from("pending");
    let mut retries = 0;
    const MAX_RETRIES: u32 = 60;

    while status == "pending" || status == "running" {
        if retries >= MAX_RETRIES {
            panic!("Crawl did not complete within expected time");
        }

        let status_response = app
            .server
            .get(&format!("/v1/crawl/{}", crawl_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;

        assert_eq!(status_response.status_code(), StatusCode::OK);
        let status_data: serde_json::Value = status_response.json();
        status = status_data["status"].as_str().unwrap().to_string();

        sleep(Duration::from_millis(1000)).await;
        retries += 1;
    }

    assert_eq!(status, "completed", "Crawl should complete successfully");

    // Step 3: Verify crawl results
    let final_response = app
        .server
        .get(&format!("/v1/crawl/{}", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(final_response.status_code(), StatusCode::OK);
    let final_data: serde_json::Value = final_response.json();

    assert!(final_data["results"].is_array());
    assert!(final_data["results"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn test_error_handling_workflow() {
    let app = create_test_app().await;

    // Test 1: Invalid URL
    let invalid_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "not-a-valid-url"
        }))
        .await;

    assert_eq!(invalid_response.status_code(), StatusCode::BAD_REQUEST);

    // Test 2: SSRF protection
    let ssrf_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://localhost:8080"
        }))
        .await;

    assert_eq!(ssrf_response.status_code(), StatusCode::BAD_REQUEST);

    // Test 3: Non-existent domain
    let domain_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://this-domain-does-not-exist-12345.com"
        }))
        .await;

    assert_eq!(domain_response.status_code(), StatusCode::CREATED);
    let task_data: serde_json::Value = domain_response.json();
    let task_id = task_data["id"].as_str().unwrap();

    // Wait for task to fail
    sleep(Duration::from_millis(5000)).await;

    let status_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(status_response.status_code(), StatusCode::OK);
    let status_data: serde_json::Value = status_response.json();
    assert_eq!(status_data["status"], "failed");
}
