// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::integration::helpers::create_test_app;
use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_ecommerce_product_monitoring_scenario() {
    let app = create_test_app().await;
    
    // Scenario: Monitor product prices across multiple e-commerce sites
    let product_urls = vec![
        "https://httpbin.org/html",  // Simulating product page
        "https://httpbin.org/json",  // Simulating API endpoint
    ];
    
    // Step 1: Create monitoring tasks with specific selectors
    let mut task_ids = Vec::new();
    
    for (index, url) in product_urls.iter().enumerate() {
        let create_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": url,
                "options": {
                    "wait_for": 1000,
                    "timeout": 15,
                    "extract_rules": {
                        "price": ".price",
                        "title": "h1",
                        "availability": ".stock-status"
                    }
                },
                "metadata": {
                    "product_id": format!("product_{}", index + 1),
                    "monitoring_type": "price_check"
                }
            }))
            .await;
        
        assert_eq!(create_response.status_code(), StatusCode::CREATED);
        let task_data: serde_json::Value = create_response.json();
        task_ids.push(task_data["id"].as_str().unwrap().to_string());
    }
    
    // Step 2: Wait for all monitoring tasks to complete
    let mut all_completed = false;
    let mut retries = 0;
    const MAX_RETRIES: u32 = 45;
    
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
                panic!("Monitoring task {} failed", task_id);
            }
        }
        
        if !all_completed {
            sleep(Duration::from_millis(1000)).await;
            retries += 1;
        }
    }
    
    assert!(all_completed, "All monitoring tasks should complete");
    
    // Step 3: Analyze results and extract structured data
    let mut results = Vec::new();
    
    for task_id in &task_ids {
        let final_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        assert_eq!(final_response.status_code(), StatusCode::OK);
        let final_data: serde_json::Value = final_response.json();
        results.push(final_data);
    }
    
    // Step 4: Validate business logic
    assert_eq!(results.len(), product_urls.len());
    
    for result in &results {
        assert_eq!(result["status"], "completed");
        assert!(result["result"]["content"].as_str().unwrap().len() > 0);
        
        // Verify metadata is preserved
        let metadata = &result["metadata"];
        assert!(metadata["product_id"].is_string());
        assert!(metadata["monitoring_type"].is_string());
    }
}

#[tokio::test]
async fn test_content_aggregation_scenario() {
    let app = create_test_app().await;
    
    // Scenario: Aggregate content from multiple news sources
    let news_sources = vec![
        "https://httpbin.org/html",
        "https://httpbin.org/json",
        "https://httpbin.org/xml",
    ];
    
    // Step 1: Create crawl job for content aggregation
    let crawl_response = app
        .server
        .post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "urls": news_sources,
            "options": {
                "wait_for": 1000,
                "timeout": 20,
                "extract_rules": {
                    "headlines": "h1, h2, h3",
                    "content": "article, .content, .post",
                    "metadata": "meta[name='description']"
                }
            },
            "metadata": {
                "aggregation_type": "news_digest",
                "target_audience": "business_analysts"
            }
        }))
        .await;
    
    assert_eq!(crawl_response.status_code(), StatusCode::CREATED);
    let crawl_data: serde_json::Value = crawl_response.json();
    let crawl_id = crawl_data["id"].as_str().unwrap().to_string();
    
    // Step 2: Monitor crawl progress
    let mut status = String::from("pending");
    let mut retries = 0;
    const MAX_RETRIES: u32 = 60;
    
    while status == "pending" || status == "running" {
        if retries >= MAX_RETRIES {
            panic!("Content aggregation crawl did not complete in time");
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
    
    assert_eq!(status, "completed");
    
    // Step 3: Validate aggregated content
    let final_response = app
        .server
        .get(&format!("/v1/crawl/{}", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;
    
    assert_eq!(final_response.status_code(), StatusCode::OK);
    let final_data: serde_json::Value = final_response.json();
    
    assert!(final_data["results"].is_array());
    let results = final_data["results"].as_array().unwrap();
    assert_eq!(results.len(), news_sources.len());
    
    // Verify each source was processed
    for (index, result) in results.iter().enumerate() {
        assert!(result["url"].is_string());
        assert!(result["content"].is_string());
        assert!(result["status"].as_str().unwrap() == "completed" || 
                result["status"].as_str().unwrap() == "partial");
        
        // Verify content extraction
        let content = result["content"].as_str().unwrap().to_string();
        assert!(content.len() > 0, "Source {} should have content", index);
    }
    
    // Step 4: Validate metadata preservation
    let metadata = &final_data["metadata"];
    assert_eq!(metadata["aggregation_type"], "news_digest");
    assert_eq!(metadata["target_audience"], "business_analysts");
}

#[tokio::test]
async fn test_competitive_analysis_scenario() {
    let app = create_test_app().await;
    
    // Scenario: Competitive analysis of multiple competitor websites
    let competitor_sites = vec![
        "https://httpbin.org/html",
        "https://httpbin.org/json",
    ];
    
    // Step 1: Create parallel analysis tasks
    let mut analysis_tasks = Vec::new();
    
    for (index, site) in competitor_sites.iter().enumerate() {
        let create_response = app
            .server
            .post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": site,
                "options": {
                    "wait_for": 1000,
                    "timeout": 15,
                    "extract_rules": {
                        "product_count": ".product",
                        "pricing_info": ".price, .pricing",
                        "features": ".feature, .highlight",
                        "contact_info": ".contact, .support"
                    }
                },
                "metadata": {
                    "competitor_id": format!("comp_{}", index + 1),
                    "analysis_type": "competitive_intelligence",
                    "analysis_date": "2025-01-01"
                }
            }))
            .await;
        
        assert_eq!(create_response.status_code(), StatusCode::CREATED);
        let task_data: serde_json::Value = create_response.json();
        analysis_tasks.push(task_data["id"].as_str().unwrap().to_string());
    }
    
    // Step 2: Wait for analysis completion with timeout
    let start_time = std::time::Instant::now();
    let timeout = Duration::from_secs(120);
    
    loop {
        if start_time.elapsed() > timeout {
            panic!("Competitive analysis timed out");
        }
        
        let mut all_done = true;
        
        for task_id in &analysis_tasks {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;
            
            assert_eq!(status_response.status_code(), StatusCode::OK);
            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"].as_str().unwrap().to_string();
            
            if status == "pending" || status == "running" {
                all_done = false;
                break;
            } else if status == "failed" {
                panic!("Analysis task {} failed", task_id);
            }
        }
        
        if all_done {
            break;
        }
        
        sleep(Duration::from_millis(1000)).await;
    }
    
    // Step 3: Collect and analyze results
    let mut analysis_results = Vec::new();
    
    for task_id in &analysis_tasks {
        let result_response = app
            .server
            .get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        assert_eq!(result_response.status_code(), StatusCode::OK);
        let result_data: serde_json::Value = result_response.json();
        analysis_results.push(result_data);
    }
    
    // Step 4: Generate competitive insights
    assert_eq!(analysis_results.len(), competitor_sites.len());
    
    for result in &analysis_results {
        assert_eq!(result["status"], "completed");
        
        // Verify competitive metadata
        let metadata = &result["metadata"];
        assert!(metadata["competitor_id"].is_string());
        assert_eq!(metadata["analysis_type"], "competitive_intelligence");
        
        // Verify content extraction for analysis
        let content = result["result"]["content"].as_str().unwrap().to_string();
        assert!(content.len() > 100, "Analysis should extract substantial content");
    }
}