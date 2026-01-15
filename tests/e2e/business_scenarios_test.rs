// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::common::constants::timeouts::CRAWL_TASK_TIMEOUT;
use crate::integration::helpers::create_test_app;
use axum::http::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

async fn wait_for_tasks_completion(
    app: &crate::integration::helpers::test_app::TestApp,
    task_ids: &[String],
    max_retries: u32,
) {
    let mut retries = 0;

    loop {
        let all_completed = task_ids.iter().all(|task_id| {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;

            if status_response.status_code() != StatusCode::OK {
                return false;
            }

            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"]
                .as_str()
                .expect("Missing 'status' field in task response");

            match status {
                "queued" | "active" => false,
                "failed" => panic!("Task {} failed", task_id),
                _ => true,
            }
        });

        if all_completed {
            break;
        }

        if retries >= max_retries {
            panic!("Tasks did not complete within timeout");
        }

        sleep(Duration::from_millis(1000)).await;
        retries += 1;
    }
}

#[tokio::test]
async fn test_ecommerce_product_monitoring_scenario() {
    let app = create_test_app().await;
    let product_urls = ["https://httpbin.org/html", "https://httpbin.org/json"];
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

        assert_eq!(create_response.status_code(), StatusCode::ACCEPTED);
        let task_data: serde_json::Value = create_response.json();
        task_ids.push(
            task_data["id"]
                .as_str()
                .expect("Missing 'id' field in task response")
                .to_string(),
        );
    }

    wait_for_tasks_completion(&app, &task_ids, 90).await;

    let results: Vec<serde_json::Value> = task_ids
        .iter()
        .map(|task_id| {
            let final_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;

            assert_eq!(final_response.status_code(), StatusCode::OK);
            final_response.json()
        })
        .collect();

    assert_eq!(results.len(), product_urls.len());

    for result in &results {
        assert_eq!(result["status"], "completed");
        assert!(!result["result"]["content"]
            .as_str()
            .expect("Missing 'content' field in result")
            .is_empty());
        assert!(result["metadata"]["product_id"].is_string());
        assert!(result["metadata"]["monitoring_type"].is_string());
    }
}

#[tokio::test]
async fn test_content_aggregation_scenario() {
    let app = create_test_app().await;
    let news_sources = vec![
        "https://httpbin.org/html",
        "https://httpbin.org/json",
        "https://httpbin.org/xml",
    ];

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

    assert_eq!(crawl_response.status_code(), StatusCode::ACCEPTED);
    let crawl_data: serde_json::Value = crawl_response.json();
    let crawl_id = crawl_data["id"]
        .as_str()
        .expect("Missing 'id' field in crawl response")
        .to_string();

    let mut retries = 0;
    const MAX_RETRIES: u32 = 90;

    loop {
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
        let status = status_data["status"]
            .as_str()
            .expect("Missing 'status' field in crawl status response");

        if status != "pending" && status != "running" {
            break;
        }

        sleep(Duration::from_millis(1000)).await;
        retries += 1;
    }

    let final_response = app
        .server
        .get(&format!("/v1/crawl/{}", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(final_response.status_code(), StatusCode::OK);
    let final_data: serde_json::Value = final_response.json();

    assert!(final_data["results"].is_array());
    let results = final_data["results"]
        .as_array()
        .expect("Missing 'results' array in crawl response");
    assert_eq!(results.len(), news_sources.len());

    for (index, result) in results.iter().enumerate() {
        assert!(result["url"].is_string());
        assert!(result["content"].is_string());
        assert!(
            result["status"]
                .as_str()
                .expect("Missing 'status' field in crawl result")
                == "completed"
                || result["status"]
                    .as_str()
                    .expect("Missing 'status' field in crawl result")
                    == "partial"
        );
        assert!(
            !result["content"]
                .as_str()
                .expect("Missing 'content' field in crawl result")
                .is_empty(),
            "Source {} should have content",
            index
        );
    }

    let metadata = &final_data["metadata"];
    assert_eq!(metadata["aggregation_type"], "news_digest");
    assert_eq!(metadata["target_audience"], "business_analysts");
}

#[tokio::test]
async fn test_competitive_analysis_scenario() {
    let app = create_test_app().await;
    let competitor_sites = ["https://httpbin.org/html", "https://httpbin.org/json"];
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

        assert_eq!(create_response.status_code(), StatusCode::ACCEPTED);
        let task_data: serde_json::Value = create_response.json();
        analysis_tasks.push(
            task_data["id"]
                .as_str()
                .expect("Missing 'id' field in task response")
                .to_string(),
        );
    }

    let start_time = std::time::Instant::now();
    let timeout = CRAWL_TASK_TIMEOUT;

    loop {
        if start_time.elapsed() > timeout {
            panic!("Competitive analysis timed out after {:?}", timeout);
        }

        let all_done = analysis_tasks.iter().all(|task_id| {
            let status_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;

            if status_response.status_code() != StatusCode::OK {
                return false;
            }

            let status_data: serde_json::Value = status_response.json();
            let status = status_data["status"]
                .as_str()
                .expect("Missing 'status' field in task response");

            match status {
                "pending" | "running" => false,
                "failed" => panic!("Task {} failed", task_id),
                _ => true,
            }
        });

        if all_done {
            break;
        }

        sleep(Duration::from_millis(1000)).await;
    }

    let analysis_results: Vec<serde_json::Value> = analysis_tasks
        .iter()
        .map(|task_id| {
            let result_response = app
                .server
                .get(&format!("/v1/scrape/{}", task_id))
                .add_header("Authorization", format!("Bearer {}", app.api_key))
                .await;

            assert_eq!(result_response.status_code(), StatusCode::OK);
            result_response.json()
        })
        .collect();

    assert_eq!(analysis_results.len(), competitor_sites.len());

    for result in &analysis_results {
        assert_eq!(result["status"], "completed");
        assert!(result["metadata"]["competitor_id"].is_string());
        assert_eq!(
            result["metadata"]["analysis_type"],
            "competitive_intelligence"
        );
        assert!(
            result["result"]["content"]
                .as_str()
                .expect("Missing 'content' field in analysis result")
                .len()
                > 100,
            "Analysis should extract substantial content"
        );
    }
}
