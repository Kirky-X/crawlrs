#![cfg(test)]
// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.
#![cfg(test)]

//! Tests for worker concurrency control with Lua script optimization

use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::infrastructure::cache::redis_client::RedisClient;
use crawlrs::infrastructure::database::entities::task as task_entity;
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use sea_orm::EntityTrait;
use std::sync::Arc;
use uuid::Uuid;

/// Lua script for atomic concurrency control (same as in scrape_worker.rs)
const CONCURRENCY_CONTROL_LUA: &str = r#"
local active_key = KEYS[1]
local limit_key = KEYS[2]
local task_id = ARGV[1]
local score = tonumber(ARGV[2])
local stale_threshold = tonumber(ARGV[3])
local default_limit = tonumber(ARGV[4])

redis.call('ZREMRANGEBYSCORE', active_key, '-inf', stale_threshold)
local limit = tonumber(redis.call('GET', limit_key) or default_limit)

if redis.call('ZSCORE', active_key, task_id) then
    redis.call('ZADD', active_key, score, task_id)
    return 1
end

local count = redis.call('ZCARD', active_key)
if count < limit then
    redis.call('ZADD', active_key, score, task_id)
    return 1
else
    return 0
end
"#;

/// Test Lua script concurrency control with multiple workers
#[tokio::test]
#[ignore]  # Skip: Integration test requiring full environment
async fn test_lua_concurrency_control_single_worker() {
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis = RedisClient::new(&redis_url)
        .await
        .expect("Failed to create Redis client");
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let active_key = format!("team:{}:active_tasks", team_id);
    let limit_key = format!("team:{}:concurrency_limit", team_id);

    // Cleanup
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");

    // Set limit to 5
    redis
        .set_forever(&limit_key, "5")
        .await
        .expect("Failed to set concurrency limit");

    let now = Utc::now().timestamp() as f64;
    let stale_threshold = now - 3600.0;

    // Acquire permit - should succeed
    let result = redis
        .eval(
            CONCURRENCY_CONTROL_LUA,
            &[&active_key, &limit_key],
            &[
                &task_id.to_string(),
                &now.to_string(),
                &stale_threshold.to_string(),
                "5",
            ],
        )
        .await
        .expect("Failed to execute Lua script");

    assert_eq!(result, "1", "First acquisition should succeed");

    // Verify task is in the set
    let count = redis
        .zcard(&active_key)
        .await
        .expect("Failed to get active task count");
    assert_eq!(count, 1, "Should have 1 active task");

    // Cleanup
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");
}

/// Test Lua script concurrency control with multiple concurrent workers
#[tokio::test]
#[ignore]  # Skip: Integration test requiring full environment
async fn test_lua_concurrency_control_multiple_workers() {
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis = RedisClient::new(&redis_url)
        .await
        .expect("Failed to create Redis client");
    let team_id = Uuid::new_v4();
    let limit_key = format!("team:{}:concurrency_limit", team_id);

    // Cleanup
    let active_key = format!("team:{}:active_tasks", team_id);
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");

    // Set limit to 3
    redis
        .set_forever(&limit_key, "3")
        .await
        .expect("Failed to set concurrency limit");

    let now = Utc::now().timestamp() as f64;
    let stale_threshold = now - 3600.0;

    // Spawn 5 concurrent workers
    let mut handles = Vec::new();
    for i in 0..5 {
        let redis = redis.clone();
        let active_key = active_key.clone();
        let limit_key = limit_key.clone();
        let task_id = Uuid::new_v4();
        let now = now;
        let stale_threshold = stale_threshold;

        handles.push(tokio::spawn(async move {
            let result = redis
                .eval(
                    CONCURRENCY_CONTROL_LUA,
                    &[&active_key, &limit_key],
                    &[
                        &task_id.to_string(),
                        &now.to_string(),
                        &stale_threshold.to_string(),
                        "3",
                    ],
                )
                .await
                .expect("Failed to execute Lua script");
            result
        }));
    }

    // Collect results
    let results: Vec<String> = futures::future::join_all(handles)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect worker results");

    // Count successes
    let success_count = results.iter().filter(|r| *r == "1").count();
    let failure_count = results.iter().filter(|r| *r == "0").count();

    assert_eq!(success_count, 3, "Exactly 3 workers should succeed");
    assert_eq!(failure_count, 2, "Exactly 2 workers should fail");

    // Verify final count
    let count = redis
        .zcard(&active_key)
        .await
        .expect("Failed to get active task count");
    assert_eq!(count, 3, "Should have exactly 3 active tasks");

    // Cleanup
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");
}

/// Test stale task cleanup in Lua script
#[tokio::test]
#[ignore]  # Skip: Integration test requiring full environment
async fn test_lua_concurrency_control_stale_cleanup() {
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis = RedisClient::new(&redis_url)
        .await
        .expect("Failed to create Redis client");
    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let active_key = format!("team:{}:active_tasks", team_id);
    let limit_key = format!("team:{}:concurrency_limit", team_id);

    // Cleanup
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");

    // Add a stale task (2 hours old)
    let stale_score = (Utc::now().timestamp() - 7200) as f64;
    redis
        .zadd(&active_key, &Uuid::new_v4().to_string(), stale_score)
        .await
        .expect("Failed to add stale task");

    let count_before = redis
        .zcard(&active_key)
        .await
        .expect("Failed to get active task count");
    assert_eq!(count_before, 1, "Should have 1 stale task before cleanup");

    // Set limit and acquire
    redis
        .set_forever(&limit_key, "5")
        .await
        .expect("Failed to set concurrency limit");
    let now = Utc::now().timestamp() as f64;
    let stale_threshold = now - 3600.0;

    let result = redis
        .eval(
            CONCURRENCY_CONTROL_LUA,
            &[&active_key, &limit_key],
            &[
                &task_id.to_string(),
                &now.to_string(),
                &stale_threshold.to_string(),
                "5",
            ],
        )
        .await
        .expect("Failed to execute Lua script");

    assert_eq!(result, "1", "Acquisition should succeed");

    // Stale task should be cleaned up, new task added
    let count_after = redis
        .zcard(&active_key)
        .await
        .expect("Failed to get active task count");
    assert_eq!(count_after, 1, "Should have 1 task after stale cleanup");

    // Cleanup
    redis
        .del(&active_key)
        .await
        .expect("Failed to delete active key");
    redis
        .del(&limit_key)
        .await
        .expect("Failed to delete limit key");
}
