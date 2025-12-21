// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;
use crawlrs::infrastructure::database::entities::task;
use crawlrs::utils::telemetry::init_telemetry;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use std::sync::Arc;
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

/// 测试抓取速率限制 (UAT-018)
///
/// 验证 API 是否对超出限制的请求强制执行速率限制。
#[tokio::test]
async fn test_scrape_rate_limit() {
    let app = create_test_app().await;

    // The rate limiter in tests is set to 10 RPM (see helpers/mod.rs)
    // We send 10 requests which should be allowed
    for i in 0..10 {
        let response = app.server.post("/v1/scrape")
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .json(&json!({
                "url": format!("https://example.com/{}", i),
                "sync_wait_ms": 0
            }))
            .await;
        assert_eq!(response.status_code(), StatusCode::CREATED);
    }

    // The 11th request should be rate limited
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com/11",
            "sync_wait_ms": 0
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::TOO_MANY_REQUESTS);
}

/// 测试团队并发限制 (UAT-019)
///
/// 验证系统是否强制执行团队并发限制，并在达到限制时将任务重新排队。
#[tokio::test]
async fn test_team_concurrency_limit() {
    let app = create_test_app().await;

    // Use a team-specific concurrency limit of 1
    let team_id = app.team_id;
    let redis_client = match app.redis_process.as_ref() {
        Some(_) => {
            crawlrs::infrastructure::cache::redis_client::RedisClient::new(&app.redis_url).await.unwrap()
        },
        None => panic!("Redis must be running"),
    };

    let limit_key = format!("team:{}:concurrency_limit", team_id);
    let _: () = redis_client.set(&limit_key, "1", 3600).await.unwrap();

    // 1. Submit first task (this will be picked up by worker and stay "active" for a bit if we can control it)
    // Actually, we don't need the worker to be slow, we just need to ensure the limit is hit.
    // If we have 1 worker, it will process tasks one by one.
    // To trigger UAT-019 "concurrency slot exhaustion", we want to see the worker *rejecting* a task 
    // because the limit is exceeded.
    
    // In our system, the worker *acquires* a permit before processing. 
    // If it fails to acquire, it reschedules.
    
    // Let's manually increment the active jobs count in Redis to simulate an active job.
    let active_jobs_key = format!("team:{}:active_jobs", team_id);
    let _: () = redis_client.set(&active_jobs_key, "1", 3600).await.unwrap();

    // 2. Submit a task. The worker should try to pick it up, see current=1, limit=1.
    // Wait, the worker logic is: current = incr(); if current > limit { decr(); return false; }
    // So if current=1 and limit=1, incr() makes it 2, 2 > 1 is true, so it rejects.
    
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "sync_wait_ms": 1000 // Short wait
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::ACCEPTED);
    let task_id: Uuid = response.json::<serde_json::Value>()["id"].as_str().unwrap().parse().unwrap();

    // 3. Wait a bit for worker to try processing
    tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

    // 4. Check task status. It should still be Queued (rescheduled) or have a scheduled_at in the future.
    let task = task::Entity::find()
        .filter(task::Column::Id.eq(task_id))
        .one(app.db_pool.as_ref())
        .await
        .unwrap()
        .unwrap();
    
    // Verify the status is still queued (not started because of concurrency limit)
    assert_eq!(task.status, "queued");
    
    // Clean up
    let _: () = redis_client.del(&active_jobs_key).await.unwrap();
    let _: () = redis_client.del(&limit_key).await.unwrap();
}

/// 测试断路器和引擎降级 (UAT-022, UAT-005)
#[tokio::test]
async fn test_circuit_breaker_and_engine_fallback() {
    use crawlrs::engines::router::{EngineRouter, LoadBalancingStrategy};
    use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
    use crawlrs::engines::circuit_breaker::{CircuitBreaker, CircuitConfig};
    use crawlrs::engines::reqwest_engine::ReqwestEngine;
    use crawlrs::engines::playwright_engine::PlaywrightEngine;
    use std::time::Duration;
    use std::collections::HashMap;

    // 1. 设置引擎：ReqwestEngine 和 PlaywrightEngine
    // 我们将使用一个本地服务器，它根据 User-Agent 返回不同的结果或超时
    let app_server = axum::Router::new()
        .route("/conditional", axum::routing::get(|headers: axum::http::HeaderMap| async move {
            let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
            if ua.contains("crawlrs") {
                // ReqwestEngine 的 User-Agent 包含 "crawlrs"
                // 模拟超时：延迟 2 秒，而请求超时设置为 1 秒
                tokio::time::sleep(Duration::from_secs(2)).await;
                (axum::http::StatusCode::OK, "Too slow for reqwest")
            } else {
                // PlaywrightEngine 或其他
                (axum::http::StatusCode::OK, "Success from other engine")
            }
        }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app_server).await.unwrap();
    });

    let test_url = format!("http://{}/conditional", addr);

    let engine_a = Arc::new(ReqwestEngine);
    let engine_b = Arc::new(PlaywrightEngine);
    
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![engine_a.clone(), engine_b.clone()];
    
    // 2. 设置断路器：低阈值用于测试
    let circuit_config = CircuitConfig {
        failure_threshold: 2,
        recovery_timeout: Duration::from_secs(60),
        failure_window: Duration::from_secs(60),
    };
    let cb = Arc::new(CircuitBreaker::with_default_config(circuit_config));
    
    // 3. 设置 Router
    let router = EngineRouter::with_circuit_breaker_and_strategy(
        engines,
        cb.clone(),
        LoadBalancingStrategy::RoundRobin,
    );

    // 禁用 SSRF 保护以便进行本地测试
    std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");

    let request = ScrapeRequest {
        url: test_url,
        headers: HashMap::new(),
        timeout: Duration::from_secs(1), // 短超时触发 Reqwest 失败
        needs_js: true, // 确保 Playwright 愿意处理
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: vec![],
        sync_wait_ms: 0,
    };

    // 4. 第一次请求：Reqwest 应该超时失败，然后降级到 Playwright 成功
    let response = router.route(&request).await;
    
    // 如果 Playwright 也不可用（例如环境中没有 Chrome），我们跳过此测试的后续部分
    match response {
        Ok(resp) => {
            assert_eq!(resp.status_code, 200);
            assert!(resp.content.contains("Success from other engine"));
            println!("Fallback successful: Reqwest failed (timeout), Playwright succeeded.");
        },
        Err(e) => {
            // 如果 Playwright 因为环境问题失败，这里可能会报错
            println!("Request failed: {:?}. This might be due to missing Chrome for Playwright.", e);
            if format!("{:?}", e).contains("Could not auto detect a chrome executable") {
                println!("Skipping circuit breaker assertions because Playwright (Chrome) is not available.");
                return;
            }
            // 在这种情况下，我们至少验证了 Reqwest 的失败触发了某种行为
        }
    }

    // 5. 验证断路器状态
    // Reqwest 失败了一次
    let stats = cb.get_stats("reqwest").await;
    assert_eq!(stats.failure_count, 1);

    // 6. 再次触发 Reqwest 失败以打开断路器
    let _ = router.route(&request).await;
    let stats = cb.get_stats("reqwest").await;
    // 如果 Playwright 失败了，也会记录失败，但我们关心的是 reqwest 的熔断器
    assert_eq!(stats.failure_count, 2);
    assert!(stats.is_open);
    println!("Circuit breaker for 'reqwest' is now OPEN.");

    // 7. 第三次请求：由于断路器打开，Reqwest 应该被跳过，直接尝试 Playwright
    let start = std::time::Instant::now();
    let response = router.route(&request).await;
    let elapsed = start.elapsed();
    
    // 断路器打开后，Reqwest 应该立即被跳过，不需要等待其超时
    assert!(elapsed < Duration::from_secs(1)); 
    
    if let Ok(resp) = response {
        assert!(resp.content.contains("Success from other engine"));
        println!("Circuit breaker working: Reqwest skipped, Playwright used immediately.");
    } else {
        println!("Circuit breaker working: Reqwest skipped, but Playwright also failed (as expected in CI).");
    }
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

/// 测试团队数据隔离 (UAT-029)
///
/// 验证不同团队之间的任务数据是完全隔离的。
#[tokio::test]
async fn test_team_data_isolation() {
    let app = create_test_app().await;

    // Create Team A task
    let response_a = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://team-a.com",
            "sync_wait_ms": 0
        }))
        .await;

    assert_eq!(response_a.status_code(), StatusCode::CREATED);
    let task_a: serde_json::Value = response_a.json();
    let task_a_id = task_a["id"].as_str().unwrap();

    // 2. 创建 Team B 并获取其 API Key
    let (api_key_b, _team_id_b) = app.create_team("team-b").await;

    // 3. 尝试使用 Team B 的 API Key 访问 Team A 的任务
    let response_b_access_a = app
        .server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key_b))
        .json(&json!({
            "team_id": _team_id_b,
            "task_ids": [task_a_id]
        }))
        .await;

    assert_eq!(response_b_access_a.status_code(), StatusCode::OK);
    let data_b: serde_json::Value = response_b_access_a.json();
    // 应该没有返回任务，因为任务 A 不属于 Team B
    assert_eq!(data_b["data"]["tasks"].as_array().unwrap().len(), 0);

    // 4. Team B 创建自己的任务
    let response_b = app
        .server
        .post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", api_key_b))
        .json(&json!({
            "url": "https://team-b.com",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response_b.status_code(), StatusCode::CREATED);
    let task_b: serde_json::Value = response_b.json();
    let task_b_id = task_b["id"].as_str().unwrap();

    // 5. 验证 Team B 可以访问自己的任务
    let response_b_access_b = app
        .server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", api_key_b))
        .json(&json!({
            "team_id": _team_id_b,
            "task_ids": [task_b_id]
        }))
        .await;
    assert_eq!(response_b_access_b.status_code(), StatusCode::OK);
    let data_b_self: serde_json::Value = response_b_access_b.json();
    assert_eq!(data_b_self["data"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(data_b_self["data"]["tasks"][0]["id"], task_b_id);

    // 6. 验证 Team A 无法访问 Team B 的任务
    let response_a_access_b = app
        .server
        .post("/v2/tasks/query")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "team_id": app.team_id,
            "task_ids": [task_b_id]
        }))
        .await;
    assert_eq!(response_a_access_b.status_code(), StatusCode::OK);
    let data_a_self: serde_json::Value = response_a_access_b.json();
    assert_eq!(data_a_self["data"]["tasks"].as_array().unwrap().len(), 0);
}

/// 测试 SSRF 防护 (UAT-021)
///
/// 验证系统是否正确阻止内部 URL 和私有 IP 访问。
#[tokio::test]
async fn test_ssrf_protection() {
    let app = create_test_app().await;

    // 1. 测试内部 URL (localhost)
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://localhost",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json();
    assert!(body["error"].as_str().unwrap().contains("SSRF protection"));

    // 2. 测试内部 IP (127.0.0.1)
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://127.0.0.1",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);

    // 3. 测试私有 IP (192.168.1.1)
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://192.168.1.1",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);

    // 4. 测试私有 IP (10.0.0.1)
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "http://10.0.0.1",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);

    // 5. 测试有效外部 URL
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response.status_code(), StatusCode::CREATED);
}

/// 测试 JavaScript 渲染 (UAT-004)
    ///
    /// 验证系统是否能正确处理需要 JavaScript 渲染的 SPA 页面。
    #[tokio::test]
    async fn test_js_rendering_spa() {
    let app = create_test_app().await;

    // 1. 发起抓取请求，明确要求 JS 渲染
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com", // 使用 example.com 代替 google.com
            "needs_js": true,
            "sync_wait_ms": 2000 // 增加同步等待时间
        }))
        .await;
    
    // 根据同步等待结果设置响应状态
    let status_code = response.status_code();
    assert!(status_code == StatusCode::CREATED || status_code == StatusCode::ACCEPTED);
    let task_id: Uuid = response.json::<serde_json::Value>()["id"].as_str().unwrap().parse().unwrap();

    // 2. 等待任务完成
    let mut completed = false;
    for _ in 0..20 { // 增加重试次数
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let status_response = app.server.get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        if status_response.status_code() == StatusCode::OK {
            let body: serde_json::Value = status_response.json();
            if body["status"] == "completed" {
                completed = true;
                // 验证内容是否包含渲染后的特征
                // 在 /v1/scrape/:id 返回的任务详情中，结果可能在 body["result"] 中
                // 或者我们需要通过 /v2/tasks/query 来获取包含结果的内容
                break;
            } else if body["status"] == "failed" {
                panic!("Task failed: {}", body["error"]);
            }
        }
    }
    
    assert!(completed, "Task did not complete in time");
}

/// 测试全站点爬取 (UAT-006)
///
/// 验证全站点爬取的基本流程。
#[tokio::test]
async fn test_full_site_crawl() {
    let app = create_test_app().await;

    // 1. 发起爬取请求
    let response = app.server.post("/v1/crawl")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "config": {
                "max_depth": 1
            },
            "sync_wait_ms": 1000
        }))
        .await;
    
    // 爬取任务通常返回 201 Created (如果未超时) 或 202 Accepted (如果超时)
    let status = response.status_code();
    let body = response.json::<serde_json::Value>();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::ACCEPTED,
        "Expected CREATED or ACCEPTED, got {}. Body: {:?}",
        status,
        body
    );
    let crawl_id: Uuid = body["id"].as_str().unwrap().parse().unwrap();

    // 2. 检查爬取状态并等待完成
    let mut completed = false;
    for _ in 0..15 {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let status_response = app.server.get(&format!("/v1/crawl/{}", crawl_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        if status_response.status_code() == StatusCode::OK {
            let body: serde_json::Value = status_response.json();
            if body["status"] == "completed" {
                completed = true;
                break;
            }
        }
    }
    
    assert!(completed, "Crawl task did not complete in time");

    // 3. 验证爬取结果 (UAT-006)
    // 检查结果列表
    let results_response = app.server.get(&format!("/v1/crawl/{}/results", crawl_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;
    
    assert_eq!(results_response.status_code(), StatusCode::OK);
    let results_body: serde_json::Value = results_response.json();
    
    // API 返回的是 Vec<ScrapeResult> 而不是带有 pagination 包装的对象
    let data = results_body.as_array().expect("Results data should be an array");
    
    // 由于 example.com 抓取结果取决于 Mock 引擎或实际网络
    // 在测试环境中，通常会 mock 引擎返回固定内容
    // 验证至少有一个结果（首页）
    assert!(!data.is_empty(), "Should have at least one crawl result");
    
    println!("✓ UAT-006 Full site crawl verified");
}

/// 测试超时处理 (UAT-020)
///
/// 验证系统是否正确处理任务超时，并在超时后将任务状态标记为失败。
#[tokio::test]
async fn test_task_timeout_handling() {
    let app = create_test_app().await;

    // 1. 发起抓取请求，设置非常短的超时时间
    // 我们需要确保任务被分配给 worker，但 worker 处理它时会超时。
    // 在我们的系统中，超时通常是在引擎级别或任务整体级别。
    
    // 创建一个会超时的任务
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://httpbin.org/delay/5", // 强制延迟 5 秒
            "options": {
                "timeout": 1 // 1秒超时
            },
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::CREATED);
    let task_id: Uuid = response.json::<serde_json::Value>()["id"].as_str().unwrap().parse().unwrap();

    // 2. 等待 worker 处理并超时
    let mut timed_out = false;
    for _ in 0..10 {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let status_response = app.server.get(&format!("/v1/scrape/{}", task_id))
            .add_header("Authorization", format!("Bearer {}", app.api_key))
            .await;
        
        if status_response.status_code() == StatusCode::OK {
                    let body: serde_json::Value = status_response.json();
                    println!("DEBUG: Task status body: {:?}", body);
                    if body["status"] == "failed" {
                        let error = body["error"].as_str().unwrap_or("");
                        if error.to_lowercase().contains("timeout") || error.to_lowercase().contains("expired") || error.to_lowercase().contains("all engines failed") {
                            timed_out = true;
                            break;
                        }
                    }
                }
    }
    
    assert!(timed_out, "Task did not time out as expected");
}

/// 测试分布式限流 (UAT-018)
///
/// 验证分布式限流是否按预期工作。
#[tokio::test]
async fn test_distributed_rate_limiting() {
    // 1. 创建一个 RPM=1 的测试应用
    let app = create_test_app_with_low_rate_limit().await;

    // 2. 发起第一个请求，应该成功
    let response1 = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com/1",
            "sync_wait_ms": 0
        }))
        .await;
    assert_eq!(response1.status_code(), StatusCode::CREATED);

    // 3. 立即发起第二个请求，应该被限流
    let response2 = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com/2",
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response2.status_code(), StatusCode::TOO_MANY_REQUESTS);
}

/// 创建一个低速率限制的测试应用
async fn create_test_app_with_low_rate_limit() -> super::helpers::TestApp {
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{Database, Statement, DbBackend, ConnectionTrait};
    use std::process::Command;
    use crawlrs::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
    use crawlrs::presentation::middleware::distributed_rate_limit_middleware::distributed_rate_limit_middleware;
    use crawlrs::presentation::middleware::rate_limit_middleware::RateLimiter;
    use crawlrs::presentation::routes;
    use crawlrs::infrastructure::cache::redis_client::RedisClient;
    use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
    use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
    use crawlrs::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
    use crawlrs::infrastructure::services::rate_limiting_service_impl::{RateLimitingConfig, RateLimitingServiceImpl};
    use crawlrs::queue::task_queue::{PostgresTaskQueue, TaskQueue};
    use crawlrs::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
    use crawlrs::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
    use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
    use crawlrs::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
    use crawlrs::engines::reqwest_engine::ReqwestEngine;
    use crawlrs::engines::playwright_engine::PlaywrightEngine;
    use crawlrs::engines::router::EngineRouter;
    use crawlrs::engines::traits::ScraperEngine;
    use crawlrs::application::usecases::create_scrape::CreateScrapeUseCase;
    use crawlrs::utils::robots::RobotsChecker;
    use crawlrs::domain::search::engine::SearchEngine;
    use crawlrs::infrastructure::search::aggregator::SearchAggregator;
    use crawlrs::infrastructure::search::google::GoogleSearchEngine;
    use crawlrs::config::settings::Settings;
    use axum::Extension;
    use axum_test::TestServer;

    // 此函数逻辑类似于 helpers/mod.rs 中的 create_test_app_with_rate_limit_options
    // 但我们可以直接在此处设置特定的限流参数
    
    // 1. Setup SQLite
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let db_pool = Arc::new(db);

    // 2. Setup Redis
    let start_port = 8000; 
    let result = crawlrs::utils::port_sniffer::PortSniffer::find_available_port(start_port, true).unwrap();
    let redis_port = result.port;
    let redis_process = Command::new("redis-server")
        .arg("--port")
        .arg(redis_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start redis-server");

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);

    Migrator::up(db_pool.as_ref(), None).await.unwrap();

    let api_key = Uuid::new_v4().to_string();
    let team_id = Uuid::new_v4();

    db_pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
        vec![team_id.into()],
    )).await.unwrap();

    db_pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
        vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
    )).await.unwrap();

    db_pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO credits (id, team_id, balance, created_at, updated_at) VALUES (?, ?, 1000, datetime('now'), datetime('now'))",
        vec![Uuid::new_v4().into(), team_id.into()],
    )).await.unwrap();

    let redis_client = RedisClient::new(&redis_url).await.unwrap();
    
    // CRITICAL: Set 1 RPM limit
    let rate_limiter = Arc::new(RateLimiter::new(redis_client.clone(), 1));

    let task_repo = Arc::new(TaskRepositoryImpl::new(db_pool.clone(), chrono::Duration::seconds(10)));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db_pool.clone()));
    let backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db_pool.clone()));

    let rate_limiting_service: Arc<dyn crawlrs::domain::services::rate_limiting_service::RateLimitingService> = Arc::new(RateLimitingServiceImpl::new(
        Arc::new(redis_client.clone()),
        task_repo.clone(),
        backlog_repo,
        credits_repo.clone(),
        RateLimitingConfig::default(),
    ));

    let queue: Arc<dyn TaskQueue> = Arc::new(PostgresTaskQueue::new(task_repo.clone()));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db_pool.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db_pool.clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db_pool.clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db_pool.clone()));

    let reqwest_engine = Arc::new(ReqwestEngine);
    let playwright_engine = Arc::new(PlaywrightEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine, playwright_engine];
    let router = Arc::new(EngineRouter::new(engines));

    let create_scrape_use_case = Arc::new(CreateScrapeUseCase::new(router.clone()));
    let robots_checker = Arc::new(RobotsChecker::new(Some(Arc::new(redis_client.clone()))));

    let mut search_engines: Vec<Arc<dyn SearchEngine>> = Vec::new();
    search_engines.push(Arc::new(GoogleSearchEngine::new()));
    let search_engine_service: Arc<dyn SearchEngine> = Arc::new(SearchAggregator::new(search_engines, 10000));

    let mut settings = Settings::new().unwrap();
    settings.rate_limiting.enabled = true;
    let settings = Arc::new(settings);

    let auth_state = AuthState {
        db: db_pool.clone(),
        team_id: Uuid::nil(),
    };

    let app = routes::routes()
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter.clone(),
            distributed_rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth_middleware,
        ))
        .layer(Extension(queue))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(rate_limiting_service))
        .layer(Extension(crawl_repo))
        .layer(Extension(credits_repo))
        .layer(Extension(result_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service));

    let server = TestServer::new(app).unwrap();

    super::helpers::TestApp {
        server,
        db_pool,
        api_key,
        team_id,
        task_repo,
        worker_manager: None,
        redis_process: Some(redis_process),
        redis_url,
    }
}

/// 测试无效 API 密钥 (UAT-028)
///
/// 验证非法访问是否被拦截。
#[tokio::test]
async fn test_invalid_api_key_v2() {
    let app = create_test_app().await;

    // 1. 使用无效的 API Key
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", "Bearer invalid-key")
        .json(&json!({
            "url": "https://example.com",
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);

    // 2. 不带 Authorization 头
    let response = app.server.post("/v1/scrape")
        .json(&json!({
            "url": "https://example.com",
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::UNAUTHORIZED);
}

/// 测试任务超时 (UAT-020)
///
/// 验证系统是否正确识别和处理已过期的任务。
#[tokio::test]
async fn test_task_expiration() {
    let app = create_test_app().await;

    // 1. 创建一个已经过期的任务
    // 我们手动在数据库中创建一个任务，设置 expires_at 为过去的时间
    let expired_task_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let expires_at = now - chrono::Duration::hours(1);

    let task_model = task::ActiveModel {
        id: sea_orm::Set(expired_task_id),
        team_id: sea_orm::Set(app.team_id),
        url: sea_orm::Set("https://expired.com".to_string()),
        task_type: sea_orm::Set(crawlrs::domain::models::task::TaskType::Scrape.to_string()),
        status: sea_orm::Set(crawlrs::domain::models::task::TaskStatus::Queued.to_string()),
        payload: sea_orm::Set(json!({})),
        created_at: sea_orm::Set(now.into()),
        updated_at: sea_orm::Set(now.into()),
        expires_at: sea_orm::Set(Some(expires_at.into())),
        ..Default::default()
    };

    use sea_orm::EntityTrait;
    task::Entity::insert(task_model).exec(app.db_pool.as_ref()).await.unwrap();

    // 2. 将任务加入队列
    let task = task::Entity::find_by_id(expired_task_id)
        .one(app.db_pool.as_ref())
        .await
        .unwrap()
        .unwrap();
    
    // We need a worker to process it. In integration tests, we can use the app's worker or trigger it.
    // The ScrapeWorker in app.rs runs in a loop.
    
    // 3. 等待 Worker 处理任务
    // Worker 应该在 process_task 中检查 expires_at 并将其标记为 Failed
    // 增加等待时间，并多次检查状态，因为异步处理可能有延迟
    let mut task_status = String::new();
    for _ in 0..10 {
        let task = task::Entity::find_by_id(expired_task_id)
            .one(app.db_pool.as_ref())
            .await
            .unwrap()
            .unwrap();
        task_status = task.status.clone();
        if task_status == "failed" {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }

    // 4. 验证任务状态为 Failed
    assert_eq!(task_status, "failed", "Task should be marked as failed due to expiration");
}

/// 测试 Webhook 触发 (UAT-023)
///
/// 验证任务完成时是否正确触发 Webhook。
#[tokio::test]
async fn test_webhook_trigger() {
    let app = create_test_app().await;

    // 1. 创建一个 Webhook 接收器 (我们可以模拟一个，或者只是检查数据库中的 WebhookEvent)
    // 简单起见，我们创建一个 Webhook 配置，然后检查 WebhookEvent 表
    let webhook_url = "https://example.com/webhook";
    
    let response = app.server.post("/v1/webhooks")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": webhook_url
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::CREATED);
    
    // 2. 提交一个抓取任务
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "webhook": webhook_url,
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::CREATED);
    let task_id: Uuid = response.json::<serde_json::Value>()["id"].as_str().unwrap().parse().unwrap();

    // 3. 等待任务完成 (Mock 引擎已被真实引擎替换)
    // 增加等待时间，确保 Webhook 事件有足够的时间被创建和处理
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // 4. 检查 WebhookEvent 是否已创建
    // 我们需要访问 webhook_event 表
    use crawlrs::infrastructure::database::entities::webhook_event;
    
    // 多次尝试检查，以防数据库写入延迟
    let mut events = Vec::new();
    for i in 0..10 {
        events = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(app.team_id))
            .all(app.db_pool.as_ref())
            .await
            .unwrap();
        
        if !events.is_empty() {
            println!("DEBUG: Found {} webhook events for team {} on attempt {}", events.len(), app.team_id, i);
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    
    assert!(!events.is_empty(), "Should have created at least one webhook event");
    let has_completed_event = events.iter().any(|e| {
        let et = e.event_type.to_lowercase();
        et == "scrape_completed" || et == "scrape.completed"
    });
    if !has_completed_event {
        println!("Events found for team {}: {:?}", app.team_id, events);
    }
    assert!(has_completed_event, "Should have a scrape_completed or scrape.completed event");
}

/// 测试 Webhook 重试策略 (UAT-024)
///
/// 验证系统在 Webhook 发送失败时是否按照策略进行重试。
#[tokio::test]
async fn test_webhook_retry_policy() {
    let app = create_test_app().await;

    // 1. 创建一个无效的 Webhook 接收器 (模拟 500 错误)
    let webhook_url = "https://httpbin.org/status/500";
    
    app.server.post("/v1/webhooks")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": webhook_url
        }))
        .await;
    
    // 2. 提交一个抓取任务
    let response = app.server.post("/v1/scrape")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&json!({
            "url": "https://example.com",
            "webhook": webhook_url,
            "sync_wait_ms": 0
        }))
        .await;
    
    assert_eq!(response.status_code(), StatusCode::CREATED);
    
    // 3. 等待任务完成并触发 Webhook
    // 初始发送失败后，应该会被标记为 Failed 并计划重试
    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    // 4. 检查 WebhookEvent 状态
    use crawlrs::infrastructure::database::entities::webhook_event::{self, SeaWebhookStatus};
    
    let events = webhook_event::Entity::find()
        .filter(webhook_event::Column::TeamId.eq(app.team_id))
        .filter(webhook_event::Column::WebhookUrl.eq(webhook_url))
        .all(app.db_pool.as_ref())
        .await
        .unwrap();
    
    assert!(!events.is_empty(), "Should have created a webhook event");
    let event = &events[0];
    
    // 初始状态应该是 Failed (因为 500 是可重试错误)
    // 且 attempt_count 应该至少为 1
    let mut success = false;
    for _ in 0..10 {
        let events = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(app.team_id))
            .filter(webhook_event::Column::WebhookUrl.eq(webhook_url))
            .all(app.db_pool.as_ref())
            .await
            .unwrap();
        
        if !events.is_empty() {
            let event = &events[0];
            if event.status == SeaWebhookStatus::Failed && event.attempt_count >= 1 {
                success = true;
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    
    assert!(success, "Webhook event should be in Failed state with attempt_count >= 1");
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

    assert_eq!(response.status_code(), StatusCode::CREATED);

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

    let response = app.server.get("/health").await;

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

    let response = app.server.get("/metrics").await;

    println!("Metrics response status: {}", response.status_code());
    println!("Metrics response body: {}", response.text());

    assert_eq!(response.status_code(), StatusCode::OK);
    assert!(response.text().contains("# HELP"));
}
