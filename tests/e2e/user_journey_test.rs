use crate::integration::helpers::create_test_app;
use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_new_user_onboarding_journey() {
    let app = create_test_app().await;
    
    // Journey: New user discovers the service and tries basic functionality
    
    // Step 1: User checks service health
    let health_response = app
        .server
        .get("/health")
        .await;
    
    assert_eq!(health_response.status_code(), StatusCode::OK);
    
    // Step 2: User checks API version
    let version_response = app
        .server
        .get("/v1/version")
        .await;
    
    assert_eq!(version_response.status_code(), StatusCode::OK);
    let version_data: serde_json::Value = version_response.json();
    assert!(version_data["version"].is_string());
    
    // Step 3: User tries to scrape without authentication (should fail)
    let no_auth_response = app
        .server
        .post("/v1/scrape")
        .json(&json!({
            "url": "https://example.com"
        }))
        .await;
    
    assert_eq!(no_auth_response.status_code(), StatusCode::UNAUTHORIZED);
    
    // Step 4: User gets API key (simulated - in real scenario this would be registration)
    // For this test, we use the existing test API key
    
    // Step 5: User makes first successful scrape request
    let first_scrape_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "options": {
                "wait_for": 1000,
                "timeout": 10
            }
        }))
        .await;
    
    assert_eq!(first_scrape_response.status_code(), StatusCode::CREATED);
    let scrape_data: serde_json::Value = first_scrape_response.json();
    let task_id = scrape_data["id"].as_str().unwrap().to_string();
    
    // Step 6: User checks task status
    let mut status = String::from("pending");
    let mut retries = 0;
    const MAX_RETRIES: u32 = 30;
    
    while status == "pending" && retries < MAX_RETRIES {
        let status_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        assert_eq!(status_response.status_code(), StatusCode::OK);
        let status_data: serde_json::Value = status_response.json();
        status = status_data["status"].as_str().unwrap().to_string();
        
        sleep(Duration::from_millis(500)).await;
        retries += 1;
    }
    
    assert!(status == "completed" || status == "running");
    
    // Step 7: User explores advanced features - batch processing
    let batch_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": ["https://example.com", "https://httpbin.org/html"],
            "options": {
                "wait_for": "body",
                "timeout": 15000
            }
        }))
        .await;
    
    assert_eq!(batch_response.status_code(), StatusCode::CREATED);
    
    // Step 8: User checks metrics to understand service usage
    let metrics_response = app
        .server
        .get("/metrics")
        .await;
    
    assert_eq!(metrics_response.status_code(), StatusCode::OK);
    let metrics_content = metrics_response.text();
    assert!(metrics_content.contains("crawlrs_"));
    
    // Journey completed successfully
}

#[tokio::test]
async fn test_developer_integration_journey() {
    let app = create_test_app().await;
    
    // Journey: Developer integrates the API into their application
    
    // Step 1: Developer tests basic connectivity
    let health_response = app
        .server
        .get("/health")
        .await;
    
    assert_eq!(health_response.status_code(), StatusCode::OK);
    
    // Step 2: Developer tests authentication
    let auth_test_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://httpbin.org/status/200"
        }))
        .await;
    
    assert_eq!(auth_test_response.status_code(), StatusCode::CREATED);
    
    // Step 3: Developer tests error handling
    let error_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "invalid-url"
        }))
        .await;
    
    assert_eq!(error_response.status_code(), StatusCode::BAD_REQUEST);
    
    // Step 4: Developer tests rate limiting
    let mut rate_limit_test_passed = false;
    
    for _i in 0..20 {
        let response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": format!("https://httpbin.org/delay/1"),
                "options": {
                    "timeout": 5000
                }
            }))
            .await;
        
        if response.status_code() == StatusCode::TOO_MANY_REQUESTS {
            rate_limit_test_passed = true;
            break;
        }
        
        // Small delay between requests
        sleep(Duration::from_millis(100)).await;
    }
    
    assert!(rate_limit_test_passed, "Rate limiting should eventually trigger");
    
    // Step 5: Developer tests webhook functionality
    let webhook_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": ["https://httpbin.org/html"],
            "webhook": "https://httpbin.org/post",
            "options": {
                "wait_for": "body",
                "timeout": 10000
            }
        }))
        .await;
    
    assert_eq!(webhook_response.status_code(), StatusCode::CREATED);
    
    // Step 6: Developer tests task cancellation
    let cancel_test_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://httpbin.org/delay/10",
            "options": {
                "timeout": 15000
            }
        }))
        .await;
    
    assert_eq!(cancel_test_response.status_code(), StatusCode::CREATED);
    let cancel_data: serde_json::Value = cancel_test_response.json();
    let cancel_task_id = cancel_data["id"].as_str().unwrap().to_string();
    
    // Immediately try to cancel (may or may not succeed depending on timing)
    let cancel_response = app
        .server
        .delete(&format!("/v1/scrape/{}", cancel_task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;
    
    // Should either succeed or return appropriate error
    assert!(cancel_response.status_code() == StatusCode::OK || 
            cancel_response.status_code() == StatusCode::BAD_REQUEST);
    
    // Integration journey completed successfully
}

#[tokio::test]
async fn test_power_user_advanced_features_journey() {
    let app = create_test_app().await;
    
    // Journey: Power user explores advanced features
    
    // Step 1: Power user tests complex extraction rules
    let complex_extraction_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://httpbin.org/html",
            "options": {
                "wait_for": "body",
                "timeout": 20000,
                "extract_rules": {
                    "title": "title",
                    "headers": "h1, h2, h3",
                    "links": "a[href]",
                    "images": "img[src]",
                    "structured_data": "script[type='application/ld+json']"
                },
                "javascript_enabled": true,
                "viewport": {
                    "width": 1920,
                    "height": 1080
                }
            }
        }))
        .await;
    
    assert_eq!(complex_extraction_response.status_code(), StatusCode::CREATED);
    
    // Step 2: Power user tests different engines
    let engines = vec!["fire_engine_cdp", "fire_engine_tls", "playwright"];
    
    for engine in engines {
        let engine_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": "https://httpbin.org/html",
                "options": {
                    "engine": engine,
                    "wait_for": "body",
                    "timeout": 15000
                }
            }))
            .await;
        
        assert_eq!(engine_response.status_code(), StatusCode::CREATED);
    }
    
    // Step 3: Power user tests concurrent processing
    let mut concurrent_tasks = Vec::new();
    
    for i in 0..5 {
        let task_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": format!("https://httpbin.org/delay/{}", i),
                "options": {
                    "timeout": 10000
                }
            }))
            .await;
        
        assert_eq!(task_response.status_code(), StatusCode::CREATED);
        let task_data: serde_json::Value = task_response.json();
        concurrent_tasks.push(task_data["id"].as_str().unwrap().to_string());
    }
    
    // Step 4: Power user monitors all concurrent tasks
    let mut all_completed = false;
    let start_time = std::time::Instant::now();
    let timeout = Duration::from_secs(60);
    
    while !all_completed && start_time.elapsed() < timeout {
        all_completed = true;
        
        for task_id in &concurrent_tasks {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;
            
            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"].as_str().unwrap().to_string();
            
            if status == "pending" || status == "running" {
                all_completed = false;
                break;
            }
        }
        
        if !all_completed {
            sleep(Duration::from_millis(1000)).await;
        }
    }
    
    assert!(all_completed, "All concurrent tasks should complete");
    
    // Advanced features journey completed successfully
}