// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Concurrency controller tests
//!
//! Tests for the ConcurrencyController including limit extraction,
//! effective limit calculation, and semaphore operations.

use chrono::Utc;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::workers::concurrency_controller::ConcurrencyController;
use uuid::Uuid;

/// Helper function to create a test task
fn create_test_task(task_type: TaskType, payload: serde_json::Value) -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload,
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: None,
        lock_expires_at: None,
    }
}

// === Static Method Tests ===

#[test]
fn test_extract_payload_limit_crawl_task_with_limit() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 10
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, Some(10));
}

#[test]
fn test_extract_payload_limit_crawl_task_without_config() {
    let payload = serde_json::json!({});

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_crawl_task_without_max_concurrency() {
    let payload = serde_json::json!({
        "config": {
            "depth": 3
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_scrape_task() {
    // Scrape tasks don't use payload limit
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 5
        }
    });

    let task = create_test_task(TaskType::Scrape, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_extract_task() {
    // Extract tasks don't use payload limit
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 5
        }
    });

    let task = create_test_task(TaskType::Extract, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_invalid_max_concurrency_type() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": "invalid"
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_max_concurrency_as_float() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 10.5
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    // as_u64() should fail for float values
    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_nested_config() {
    let payload = serde_json::json!({
        "config": {
            "nested": {
                "max_concurrency": 15
            }
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

// === Effective Limit Calculation Tests ===

#[test]
fn test_get_effective_limit_from_payload() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 20
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let controller = ConcurrencyController::new(
        crawlrs::infrastructure::cache::redis_client::RedisClient::new(
            "redis://localhost:6379".to_string(),
        )
        .unwrap(),
        10,
    );

    let effective_limit = controller.get_effective_limit(&task);

    assert_eq!(effective_limit, 20);
}

#[test]
fn test_get_effective_limit_default_from_payload() {
    let payload = serde_json::json!({});

    let task = create_test_task(TaskType::Crawl, payload);

    let controller = ConcurrencyController::new(
        crawlrs::infrastructure::cache::redis_client::RedisClient::new(
            "redis://localhost:6379".to_string(),
        )
        .unwrap(),
        10,
    );

    let effective_limit = controller.get_effective_limit(&task);

    assert_eq!(effective_limit, 10);
}

#[test]
fn test_get_effective_limit_non_crawl_task() {
    // Non-crawl tasks should use default limit
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 20
        }
    });

    let task = create_test_task(TaskType::Scrape, payload);

    let controller = ConcurrencyController::new(
        crawlrs::infrastructure::cache::redis_client::RedisClient::new(
            "redis://localhost:6379".to_string(),
        )
        .unwrap(),
        10,
    );

    let effective_limit = controller.get_effective_limit(&task);

    assert_eq!(effective_limit, 10);
}

#[test]
fn test_get_effective_limit_zero_payload_limit() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 0
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let controller = ConcurrencyController::new(
        crawlrs::infrastructure::cache::redis_client::RedisClient::new(
            "redis://localhost:6379".to_string(),
        )
        .unwrap(),
        10,
    );

    let effective_limit = controller.get_effective_limit(&task);

    assert_eq!(effective_limit, 0);
}

#[test]
fn test_get_effective_limit_large_payload_limit() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 10000
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let controller = ConcurrencyController::new(
        crawlrs::infrastructure::cache::redis_client::RedisClient::new(
            "redis://localhost:6379".to_string(),
        )
        .unwrap(),
        10,
    );

    let effective_limit = controller.get_effective_limit(&task);

    assert_eq!(effective_limit, 10000);
}

// === Controller Creation Tests ===

#[test]
fn test_concurrency_controller_creation() {
    let redis_client = crawlrs::infrastructure::cache::redis_client::RedisClient::new(
        "redis://localhost:6379".to_string(),
    )
    .unwrap();

    let controller = ConcurrencyController::new(redis_client, 10);

    // Controller is created successfully
    assert_eq!(controller.default_concurrency_limit, 10);
}

#[test]
fn test_concurrency_controller_default_limit() {
    let redis_client = crawlrs::infrastructure::cache::redis_client::RedisClient::new(
        "redis://localhost:6379".to_string(),
    )
    .unwrap();

    let controller = ConcurrencyController::new(redis_client, 50);

    assert_eq!(controller.default_concurrency_limit, 50);
}

// === Task Key Generation Tests ===

#[test]
fn test_task_key_generation_consistency() {
    use crawlrs::workers::concurrency_controller::generate_task_key;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let key1 = generate_task_key(team_id, task_id);
    let key2 = generate_task_key(team_id, task_id);

    assert_eq!(key1, key2);
}

#[test]
fn test_task_key_generation_format() {
    use crawlrs::workers::concurrency_controller::generate_task_key;

    let team_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let key = generate_task_key(team_id, task_id);

    assert!(key.contains(&team_id.to_string()));
    assert!(key.contains(&task_id.to_string()));
    assert!(key.contains(':'));
}

#[test]
fn test_task_key_generation_uniqueness() {
    use crawlrs::workers::concurrency_controller::generate_task_key;

    let team_id = Uuid::new_v4();
    let task_id1 = Uuid::new_v4();
    let task_id2 = Uuid::new_v4();

    let key1 = generate_task_key(team_id, task_id1);
    let key2 = generate_task_key(team_id, task_id2);

    assert_ne!(key1, key2);
}

// === Edge Cases and Boundary Tests ===

#[test]
fn test_extract_payload_limit_with_null_config() {
    let payload = serde_json::json!({
        "config": null
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_with_empty_config() {
    let payload = serde_json::json!({
        "config": {}
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_with_negative_value() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": -5
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    // as_u64() should fail for negative numbers
    assert_eq!(limit, None);
}

#[test]
fn test_extract_payload_limit_very_large_value() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 18446744073709551615 // u64::MAX
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    let limit = ConcurrencyController::extract_payload_limit(&task);

    assert_eq!(limit, Some(u64::MAX));
}

// === Integration-like Tests (without actual Redis) ===

#[test]
fn test_concurrency_controller_cloned() {
    let redis_client = crawlrs::infrastructure::cache::redis_client::RedisClient::new(
        "redis://localhost:6379".to_string(),
    )
    .unwrap();

    let controller = ConcurrencyController::new(redis_client, 10);
    let _cloned = controller.clone();

    // Should be able to clone the controller
    // (it implements Clone trait)
}

#[test]
fn test_concurrency_controller_send_sync() {
    // Verify that ConcurrencyController implements Send and Sync
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<ConcurrencyController>();
}

// === Task Type Variations ===

#[test]
fn test_all_task_types_for_limit_extraction() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 5
        }
    });

    // Only Crawl type should extract limit
    let crawl_task = create_test_task(TaskType::Crawl, payload.clone());
    assert_eq!(
        ConcurrencyController::extract_payload_limit(&crawl_task),
        Some(5)
    );

    // All other types should return None
    let scrape_task = create_test_task(TaskType::Scrape, payload.clone());
    assert_eq!(
        ConcurrencyController::extract_payload_limit(&scrape_task),
        None
    );

    let extract_task = create_test_task(TaskType::Extract, payload.clone());
    assert_eq!(
        ConcurrencyController::extract_payload_limit(&extract_task),
        None
    );
}

// === Performance Tests (basic) ===

#[test]
fn test_extract_payload_limit_performance() {
    let payload = serde_json::json!({
        "config": {
            "max_concurrency": 10
        }
    });

    let task = create_test_task(TaskType::Crawl, payload);

    // Test that extraction is fast (should be O(1))
    let start = std::time::Instant::now();

    for _ in 0..1000 {
        let _ = ConcurrencyController::extract_payload_limit(&task);
    }

    let duration = start.elapsed();

    // Should complete 1000 extractions in less than 10ms
    assert!(duration.as_millis() < 10, "Extraction took too long: {:?}", duration);
}
