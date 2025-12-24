// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crawlrs::application::dto::extract_request::ExtractRequestDto;
use crawlrs::domain::models::credits::CreditsTransactionType;
use crawlrs::domain::repositories::credits_repository::CreditsRepository;
use crawlrs::domain::services::extraction_service::ExtractionRule;
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::sync::Arc;

use super::helpers::create_test_app;

/// 测试提取功能的信用点扣除
///
/// 验证当使用提取规则（包含LLM）时，系统会自动扣除相应的信用点
#[tokio::test]
async fn test_extract_with_rules_credit_deduction() {
    let app = create_test_app().await;

    // 获取初始信用点余额
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(app.db_pool.clone()));
    let credits_repo_ref = credits_repo.as_ref();
    let initial_balance = credits_repo_ref.get_balance(app.team_id).await.unwrap();

    // 设置提取规则（包含LLM使用）
    let mut rules = HashMap::new();
    rules.insert(
        "product_info".to_string(),
        ExtractionRule {
            selector: None,
            attr: None,
            is_array: false,
            use_llm: Some(true),
            llm_prompt: Some("Extract product name, price, and availability".to_string()),
        },
    );

    rules.insert(
        "reviews".to_string(),
        ExtractionRule {
            selector: Some(".review".to_string()),
            attr: None,
            is_array: true,
            use_llm: Some(false), // 传统CSS选择器提取，不使用LLM
            llm_prompt: None,
        },
    );

    // 创建提取请求
    let extract_request = ExtractRequestDto {
        urls: vec!["https://httpbin.org/html".to_string()],
        prompt: None,
        schema: None,
        model: Some("gpt-3.5-turbo".to_string()),
        rules: Some(rules),
        sync_wait_ms: Some(5000),
    };

    // 发送提取请求
    let response = app
        .server
        .post("/v1/extract")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&extract_request)
        .await;

    println!("Response status: {}", response.status_code());

    // 接受201 (Created) 或 202 (Accepted) 状态码
    let status = response.status_code();
    assert!(status == StatusCode::CREATED || status == StatusCode::ACCEPTED);

    let extract_response: serde_json::Value = response.json();
    println!(
        "Response body: {}",
        serde_json::to_string_pretty(&extract_response).unwrap()
    );
    let task_id = extract_response["id"].as_str().unwrap();

    // 等待任务完成（同步等待）
    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    // 打印调试信息
    println!("Task ID: {}", task_id);
    println!("App Team ID: {}", app.team_id);
    println!("App API Key: {}", app.api_key);

    // 检查任务状态
    let status_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    println!("Status response code: {}", status_response.status_code());
    let status_data: serde_json::Value = status_response.json();
    println!(
        "Status response body: {}",
        serde_json::to_string_pretty(&status_data).unwrap()
    );

    assert_eq!(status_response.status_code(), 200);

    // 验证任务状态为已完成
    assert_eq!(status_data["status"], "completed");

    // 验证信用点被扣除
    let final_balance = credits_repo_ref.get_balance(app.team_id).await.unwrap();
    assert!(
        final_balance < initial_balance,
        "Credit balance should decrease after extraction with LLM usage"
    );

    // 验证Redis中的token使用记录
    let redis_client = RedisClient::new(&app.redis_url).await.unwrap();
    let token_usage_key = format!("team:{}:token_usage", app.team_id);
    let token_usage_str: Option<String> = redis_client.get(&token_usage_key).await.unwrap_or(None);
    let token_usage: i64 = token_usage_str.and_then(|s| s.parse().ok()).unwrap_or(0);
    assert!(token_usage > 0, "Token usage should be recorded in Redis");

    // 验证数据库中的交易记录
    let transactions = credits_repo_ref
        .get_transaction_history(app.team_id, Some(10))
        .await
        .unwrap();

    let extract_transactions: Vec<_> = transactions
        .into_iter()
        .filter(|t| matches!(t.transaction_type, CreditsTransactionType::Extract))
        .collect();

    assert!(
        !extract_transactions.is_empty(),
        "Should have extract transaction recorded"
    );

    let latest_extract_transaction = &extract_transactions[0];
    assert!(
        latest_extract_transaction.amount < 0,
        "Extract transaction should be a deduction"
    );
    assert!(latest_extract_transaction
        .description
        .contains("Tokens used"));
}

/// 测试传统CSS选择器提取（无LLM使用）不应扣除信用点
#[tokio::test]
async fn test_extract_css_only_no_credit_deduction() {
    let app = create_test_app().await;

    // 获取初始信用点余额
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(app.db_pool.clone()));
    let credits_repo_ref = credits_repo.as_ref();
    let initial_balance = credits_repo_ref.get_balance(app.team_id).await.unwrap();

    // 设置仅使用CSS选择器的提取规则（无LLM使用）
    let mut rules = HashMap::new();
    rules.insert(
        "title".to_string(),
        ExtractionRule {
            selector: Some("title".to_string()),
            attr: None,
            is_array: false,
            use_llm: Some(false),
            llm_prompt: None,
        },
    );

    rules.insert(
        "headings".to_string(),
        ExtractionRule {
            selector: Some("h1, h2, h3".to_string()),
            attr: None,
            is_array: true,
            use_llm: Some(false), // 传统CSS选择器提取，不使用LLM
            llm_prompt: None,
        },
    );

    // 创建提取请求
    let extract_request = ExtractRequestDto {
        urls: vec!["https://httpbin.org/html".to_string()],
        prompt: None,
        schema: None,
        model: None,
        rules: Some(rules),
        sync_wait_ms: Some(3000),
    };

    // 发送提取请求
    let response = app
        .server
        .post("/v1/extract")
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .json(&extract_request)
        .await;

    let status = response.status_code();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::ACCEPTED,
        "Expected 201 or 202, got {}",
        status
    );

    let extract_response: serde_json::Value = response.json();
    let task_id = extract_response["id"].as_str().unwrap();

    // 等待任务完成（同步等待）
    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

    // 检查任务状态
    let status_response = app
        .server
        .get(&format!("/v1/scrape/{}", task_id))
        .add_header("Authorization", format!("Bearer {}", app.api_key))
        .await;

    assert_eq!(status_response.status_code(), 200);
    let status_data: serde_json::Value = status_response.json();

    // 验证任务状态为已完成
    assert_eq!(status_data["status"], "completed");

    // 验证信用点未被扣除（因为没有使用LLM）
    let final_balance = credits_repo_ref.get_balance(app.team_id).await.unwrap();
    assert_eq!(
        final_balance, initial_balance,
        "Credit balance should not change for CSS-only extraction"
    );

    // 验证Redis中的token使用记录应为0
    let redis_client = RedisClient::new(&app.redis_url).await.unwrap();
    let token_usage_key = format!("team:{}:token_usage", app.team_id);
    let token_usage_str: Option<String> = redis_client.get(&token_usage_key).await.unwrap_or(None);
    let token_usage: i64 = token_usage_str.and_then(|s| s.parse().ok()).unwrap_or(0);
    assert_eq!(
        token_usage, 0,
        "Token usage should be 0 for CSS-only extraction"
    );
}
