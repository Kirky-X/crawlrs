// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::integration::helpers::create_test_app;
use axum::http::StatusCode;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[tokio::test]
async fn test_performance_single_url_benchmark() {
    let app = create_test_app().await;
    
    // Benchmark: Single URL scraping performance
    let test_urls = vec![
        "https://httpbin.org/html",
        "https://httpbin.org/json",
        "https://httpbin.org/xml",
        "https://example.com",
    ];
    
    let mut results = Vec::new();
    
    for url in test_urls {
        let start_time = Instant::now();
        
        // Create task
        let create_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
            "url": url,
            "options": {
                "wait_for": 1000,
                "timeout": 30
            }
        }))
            .await;
        
        assert_eq!(create_response.status_code(), StatusCode::CREATED);
        let task_data: serde_json::Value = create_response.json();
        let task_id = task_data["id"].as_str().unwrap().to_string();
        
        // Wait for completion
        let mut status = String::from("pending");
        let mut wait_time = Duration::ZERO;
        
        while status == "pending" || status == "running" {
            sleep(Duration::from_millis(500)).await;
            wait_time += Duration::from_millis(500);
            
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;
            
            let status_data: serde_json::Value = status_response.json();
            status = status_data["status"].as_str().unwrap().to_string();
            
            if wait_time > Duration::from_secs(60) {
                panic!("Task timeout for URL: {}", url);
            }
        }
        
        let total_time = start_time.elapsed();
        
        // Get final result
        let final_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        let final_data: serde_json::Value = final_response.json();
        let content_length = final_data["result"]["content"]
            .as_str()
            .unwrap_or("")
            .len();
        
        results.push((url, total_time, content_length, status == "completed"));
    }
    
    // Performance assertions
    for (url, duration, content_length, success) in results {
        println!("URL: {}, Duration: {:?}, Content Length: {}, Success: {}", 
                 url, duration, content_length, success);
        
        assert!(success, "Task should complete successfully for {}", url);
        assert!(duration < Duration::from_secs(45), 
                "Scraping should complete within 45 seconds for {}", url);
        assert!(content_length > 0, "Should extract content from {}", url);
    }
}

#[tokio::test]
async fn test_performance_concurrent_scraping() {
    let app = create_test_app().await;
    
    // Benchmark: Concurrent scraping performance
    let concurrent_tasks = 10;
    let start_time = Instant::now();
    
    // Create multiple tasks concurrently
    let mut task_ids = Vec::new();
    
    for i in 0..concurrent_tasks {
        let create_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": format!("https://httpbin.org/delay/{}", i % 3),
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
    
    // Wait for all tasks to complete
    let mut completed_tasks = 0;
    let mut retries = 0;
    const MAX_RETRIES: u32 = 60;
    
    while completed_tasks < concurrent_tasks && retries < MAX_RETRIES {
        completed_tasks = 0;
        
        for task_id in &task_ids {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;
            
            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"].as_str().unwrap().to_string();
            
            if status == "completed" {
                completed_tasks += 1;
            } else if status == "failed" {
                panic!("Concurrent task {} failed", task_id);
            }
        }
        
        if completed_tasks < concurrent_tasks {
            sleep(Duration::from_millis(1000)).await;
            retries += 1;
        }
    }
    
    let total_time = start_time.elapsed();
    
    // Performance assertions
    assert_eq!(completed_tasks, concurrent_tasks, "All concurrent tasks should complete");
    assert!(total_time < Duration::from_secs(90), 
            "Concurrent scraping should complete within 90 seconds");
    
    println!("Concurrent scraping completed: {} tasks in {:?}", concurrent_tasks, total_time);
}

#[tokio::test]
async fn test_performance_batch_crawl() {
    let app = create_test_app().await;
    
    // Benchmark: Batch crawl performance
    let urls = vec![
        "https://httpbin.org/html",
        "https://httpbin.org/json",
        "https://httpbin.org/xml",
        "https://example.com",
        "https://httpbin.org/delay/1",
    ];
    
    let start_time = Instant::now();
    
    // Create batch crawl
    let crawl_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": urls,
            "options": {
                "wait_for": 1000,
                "timeout": 30
            }
        }))
        .await;
    
    assert_eq!(crawl_response.status_code(), StatusCode::CREATED);
    let crawl_data: serde_json::Value = crawl_response.json();
    let crawl_id = crawl_data["id"].as_str().unwrap().to_string();
    
    // Monitor progress
    let mut status = String::from("pending");
    let mut retries = 0;
    const MAX_RETRIES: u32 = 90;
    
    while status == "pending" || status == "running" {
        if retries >= MAX_RETRIES {
            panic!("Batch crawl did not complete in time");
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
    
    let total_time = start_time.elapsed();
    
    // Get final results
    let final_response = app
        .server
        .get(&format!("/v1/crawl/{}", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;
    
    let final_data: serde_json::Value = final_response.json();
    let results = final_data["results"].as_array().unwrap();
    
    // Performance assertions
    assert_eq!(status, "completed", "Batch crawl should complete successfully");
    assert!(results.len() >= urls.len() * 3 / 4, 
            "At least 75% of URLs should be processed successfully");
    assert!(total_time < Duration::from_secs(120), 
            "Batch crawl should complete within 120 seconds");
    
    println!("Batch crawl completed: {} URLs in {:?}", results.len(), total_time);
}

#[tokio::test]
async fn test_performance_extract_endpoint() {
    let app = create_test_app().await;
    
    // Benchmark: Extract endpoint performance
    let test_content = r#"
    <html>
        <head><title>Test Page</title></head>
        <body>
            <h1>Main Title</h1>
            <div class="content">
                <h2>Subtitle</h2>
                <p>This is a test paragraph with <a href="/link1">link 1</a> and <a href="/link2">link 2</a>.</p>
                <div class="price">$99.99</div>
                <div class="description">Product description here</div>
            </div>
            <footer>Copyright 2025</footer>
        </body>
    </html>
    "#;
    
    let extract_rules = json!({
        "title": "title",
        "headings": "h1, h2",
        "links": "a[href]",
        "price": ".price",
        "description": ".description",
        "content": ".content"
    });
    
    let iterations = 5;
    let mut durations = Vec::new();
    
    for _ in 0..iterations {
        let start_time = Instant::now();
        
        let extract_response = app
            .server
            .post("/v1/extract")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "content": test_content,
                "rules": extract_rules
            }))
            .await;
        
        let duration = start_time.elapsed();
        durations.push(duration);
        
        assert_eq!(extract_response.status_code(), StatusCode::ACCEPTED);
    }
    
    // Calculate average duration
    let avg_duration = durations.iter().sum::<Duration>() / iterations;
    
    // Performance assertions
    assert!(avg_duration < Duration::from_millis(500), 
            "Extract endpoint should respond within 500ms on average");
    
    println!("Extract endpoint average response time: {:?}", avg_duration);
}

#[tokio::test]
async fn test_performance_error_recovery() {
    let app = create_test_app().await;
    
    // Benchmark: Error handling and recovery performance
    let start_time = Instant::now();
    
    // Test 1: Invalid URL (should fail fast)
    let invalid_url_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "not-a-valid-url"
        }))
        .await;
    
    assert_eq!(invalid_url_response.status_code(), StatusCode::BAD_REQUEST);
    let invalid_duration = start_time.elapsed();
    
    // Test 2: SSRF protection (should fail fast)
    let ssrf_start = Instant::now();
    let ssrf_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://localhost:8080"
        }))
        .await;
    
    assert_eq!(ssrf_response.status_code(), StatusCode::BAD_REQUEST);
    let ssrf_duration = ssrf_start.elapsed();
    
    // Test 3: Timeout handling
    let timeout_start = Instant::now();
    let timeout_response = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://httpbin.org/delay/10",
            "options": {
                "timeout": 2000  // 2 second timeout
            }
        }))
        .await;
    
    assert_eq!(timeout_response.status_code(), StatusCode::CREATED);
    let timeout_task_data: serde_json::Value = timeout_response.json();
    let timeout_task_id = timeout_task_data["id"].as_str().unwrap().to_string();
    
    // Wait for timeout to occur
    sleep(Duration::from_millis(3000)).await;
    
    let timeout_status_response = app
        .server
        .get(&format!("/v1/scrape/{}", timeout_task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;
    
    let timeout_status_data: serde_json::Value = timeout_status_response.json();
    assert_eq!(timeout_status_data["status"], "failed");
    let timeout_total_duration = timeout_start.elapsed();
    
    // Performance assertions for error handling
    assert!(invalid_duration < Duration::from_secs(2), 
            "Invalid URL validation should be fast");
    assert!(ssrf_duration < Duration::from_secs(2), 
            "SSRF protection should be fast");
    assert!(timeout_total_duration < Duration::from_secs(5), 
            "Timeout handling should complete within 5 seconds");
    
    println!("Error recovery performance - Invalid URL: {:?}, SSRF: {:?}, Timeout: {:?}", 
             invalid_duration, ssrf_duration, timeout_total_duration);
}