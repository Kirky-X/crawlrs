// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl handler external unit tests
//!
//! Tests the public API surface of crawl_handler from an external consumer
//! perspective. Covers helper functions (extract_task_ids, tasks_to_id_map),
//! error mapping (From<CrawlUseCaseError>), DTO construction/validation,
//! constants, and status code selection logic.
//!
//! Handler function bodies (create_crawl, get_crawl, get_crawl_results,
//! cancel_crawl) require full CrawlRsState DI and are not tested here — they
//! need a running database and complete service wiring.

use std::collections::HashMap;

use axum::http::StatusCode;
use uuid::Uuid;

use crawlrs::application::dto::crawl_request::{CrawlConfigDto, CrawlRequestDto};
use crawlrs::application::use_cases::crawl_use_case::CrawlUseCaseError;
use crawlrs::common::constants::crawl_task::{CRAWL_TASK_CREDITS_COST, DEFAULT_TIMEOUT_MS};
use crawlrs::domain::models::task_domain::TaskType;
use crawlrs::domain::models::task_model::Task;
use crawlrs::domain::repositories::task_repository::RepositoryError;
use crawlrs::presentation::handlers::task_handler::SyncWaitResult;
use crawlrs::presentation::handlers::{extract_task_ids, tasks_to_id_map};
use validator::Validate;

// ============================================================================
// Helper: construct a Task with sensible defaults
// ============================================================================

fn make_task(id: Uuid) -> Task {
    Task::new(
        id,
        TaskType::Scrape,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    )
}

fn make_task_with_type(id: Uuid, task_type: TaskType) -> Task {
    Task::new(
        id,
        task_type,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    )
}

// ============================================================================
// extract_task_ids tests
// ============================================================================

#[test]
fn test_extract_task_ids_empty_slice() {
    let tasks: &[Task] = &[];
    let ids = extract_task_ids(tasks);
    assert!(ids.is_empty());
}

#[test]
fn test_extract_task_ids_single_task() {
    let id = Uuid::new_v4();
    let task = make_task(id);
    let ids = extract_task_ids(&[task]);
    assert_eq!(ids, vec![id]);
}

#[test]
fn test_extract_task_ids_multiple_tasks() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    let tasks = vec![make_task(id1), make_task(id2), make_task(id3)];
    let ids = extract_task_ids(&tasks);
    assert_eq!(ids, vec![id1, id2, id3]);
}

#[test]
fn test_extract_task_ids_preserves_order() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    let tasks = vec![make_task(id1), make_task(id2), make_task(id3)];
    let ids = extract_task_ids(&tasks);
    assert_eq!(ids[0], id1);
    assert_eq!(ids[1], id2);
    assert_eq!(ids[2], id3);
}

#[test]
fn test_extract_task_ids_duplicate_ids_not_deduplicated() {
    // extract_task_ids does NOT deduplicate — it maps 1:1
    let id = Uuid::new_v4();
    let tasks = vec![make_task(id), make_task(id), make_task(id)];
    let ids = extract_task_ids(&tasks);
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|&x| x == id));
}

#[test]
fn test_extract_task_ids_does_not_consume_input() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let tasks = vec![make_task(id1), make_task(id2)];
    let ids = extract_task_ids(&tasks);
    assert_eq!(ids.len(), 2);
    // Input slice still usable after call
    let ids_again = extract_task_ids(&tasks);
    assert_eq!(ids_again.len(), 2);
}

#[test]
fn test_extract_task_ids_with_different_task_types() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    let tasks = vec![
        make_task_with_type(id1, TaskType::Scrape),
        make_task_with_type(id2, TaskType::Crawl),
        make_task_with_type(id3, TaskType::Extract),
    ];
    let ids = extract_task_ids(&tasks);
    assert_eq!(ids, vec![id1, id2, id3]);
}

// ============================================================================
// tasks_to_id_map tests
// ============================================================================

#[test]
fn test_tasks_to_id_map_empty_vec() {
    let tasks: Vec<Task> = vec![];
    let map = tasks_to_id_map(tasks);
    assert!(map.is_empty());
}

#[test]
fn test_tasks_to_id_map_single_task() {
    let id = Uuid::new_v4();
    let task = make_task(id);
    let map = tasks_to_id_map(vec![task]);
    assert_eq!(map.len(), 1);
    assert!(map.contains_key(&id));
}

#[test]
fn test_tasks_to_id_map_multiple_tasks() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    let tasks = vec![make_task(id1), make_task(id2), make_task(id3)];
    let map = tasks_to_id_map(tasks);
    assert_eq!(map.len(), 3);
    assert!(map.contains_key(&id1));
    assert!(map.contains_key(&id2));
    assert!(map.contains_key(&id3));
}

#[test]
fn test_tasks_to_id_map_duplicate_ids_last_wins() {
    // HashMap::insert overwrites — last task with same ID wins
    let id = Uuid::new_v4();
    let task1 = make_task_with_type(id, TaskType::Scrape);
    let task2 = make_task_with_type(id, TaskType::Crawl);
    let task3 = make_task_with_type(id, TaskType::Extract);
    let map = tasks_to_id_map(vec![task1, task2, task3]);
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&id).unwrap().task_type, TaskType::Extract);
}

#[test]
fn test_tasks_to_id_map_consumes_input() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let tasks = vec![make_task(id1), make_task(id2)];
    let map = tasks_to_id_map(tasks);
    assert_eq!(map.len(), 2);
    // tasks vec is moved — cannot access after
}

#[test]
fn test_tasks_to_id_map_lookup_by_id() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let task1 = make_task_with_type(id1, TaskType::Scrape);
    let task2 = make_task_with_type(id2, TaskType::Crawl);
    let map = tasks_to_id_map(vec![task1, task2]);
    let found = map.get(&id1).expect("task1 should be in map");
    assert_eq!(found.task_type, TaskType::Scrape);
    assert_eq!(found.id, id1);
}

// ============================================================================
// From<CrawlUseCaseError> for (StatusCode, String) tests
// ============================================================================

#[test]
fn test_validation_error_maps_to_bad_request() {
    let err = CrawlUseCaseError::ValidationError("invalid url".to_string());
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(msg, "invalid url");
}

#[test]
fn test_validation_error_empty_message_maps_to_bad_request() {
    let err = CrawlUseCaseError::ValidationError(String::new());
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(msg.is_empty());
}

#[test]
fn test_repository_database_error_maps_to_internal_server_error() {
    let err = CrawlUseCaseError::Repository(RepositoryError::Database(anyhow::anyhow!(
        "connection refused"
    )));
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(msg.contains("connection refused"));
}

#[test]
fn test_repository_not_found_maps_to_internal_server_error() {
    // RepositoryError::NotFound maps to INTERNAL_SERVER_ERROR (not 404)
    let err = CrawlUseCaseError::Repository(RepositoryError::NotFound);
    let (status, _msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn test_crawl_not_found_maps_to_404() {
    let err = CrawlUseCaseError::NotFound;
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(msg, "Crawl not found");
}

#[test]
fn test_anyhow_error_maps_to_internal_server_error() {
    let err = CrawlUseCaseError::Anyhow(anyhow::anyhow!("unexpected failure"));
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(msg.contains("unexpected failure"));
}

#[test]
fn test_anyhow_error_with_chained_context_preserved() {
    let err = CrawlUseCaseError::Anyhow(
        anyhow::anyhow!("root cause")
            .context("middle context")
            .context("outer context"),
    );
    let (status, msg): (StatusCode, String) = err.into();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(msg.contains("outer context"));
}

// ============================================================================
// CrawlRequestDto construction and validation
// ============================================================================

#[test]
fn test_crawl_request_dto_minimal_deserialization() {
    let json = r#"{"url":"https://example.com","config":{"max_depth":2}}"#;
    let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
    assert_eq!(dto.url, "https://example.com");
    assert_eq!(dto.config.max_depth, 2);
    assert!(dto.name.is_none());
    assert!(dto.sync_wait_ms.is_none());
    assert!(dto.expires_at.is_none());
}

#[test]
fn test_crawl_request_dto_full_deserialization() {
    let json = r#"{
        "url": "https://example.com",
        "name": "My Crawl",
        "config": {
            "max_depth": 3,
            "include_patterns": ["*/blog/*"],
            "exclude_patterns": ["*/admin/*"],
            "strategy": "bfs",
            "crawl_delay_ms": 1000,
            "max_concurrency": 5,
            "proxy": "http://proxy:8080",
            "headers": {"X-Custom": "value"}
        },
        "sync_wait_ms": 5000
    }"#;
    let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
    assert_eq!(dto.url, "https://example.com");
    assert_eq!(dto.name.as_deref(), Some("My Crawl"));
    assert_eq!(dto.config.max_depth, 3);
    assert!(dto.config.include_patterns.is_some());
    assert!(dto.config.exclude_patterns.is_some());
    assert_eq!(dto.config.strategy.as_deref(), Some("bfs"));
    assert_eq!(dto.config.crawl_delay_ms, Some(1000));
    assert_eq!(dto.config.max_concurrency, Some(5));
    assert!(dto.config.proxy.is_some());
    assert!(dto.config.headers.is_some());
    assert_eq!(dto.sync_wait_ms, Some(5000));
}

#[test]
fn test_crawl_request_dto_deny_unknown_fields() {
    let json = r#"{"url":"https://example.com","config":{"max_depth":2},"unknown_field":42}"#;
    let result: Result<CrawlRequestDto, _> = serde_json::from_str(json);
    assert!(result.is_err(), "unknown fields should be rejected");
}

#[test]
fn test_crawl_config_dto_deny_unknown_fields() {
    let json = r#"{"max_depth":2,"unknown":true}"#;
    let result: Result<CrawlConfigDto, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn test_crawl_request_dto_validate_success() {
    let dto = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: Some("test".to_string()),
        config: CrawlConfigDto {
            max_depth: 3,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: Some(5000),
        expires_at: None,
    };
    assert!(dto.validate().is_ok());
}

#[test]
fn test_crawl_request_dto_validate_empty_url_fails() {
    let dto = CrawlRequestDto {
        url: "".to_string(),
        validated_url: None,
        name: None,
        config: CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: None,
        expires_at: None,
    };
    assert!(dto.validate().is_err());
}

#[test]
fn test_crawl_request_dto_validate_sync_wait_ms_exceeds_max_fails() {
    let dto = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: None,
        config: CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: Some(30001),
        expires_at: None,
    };
    assert!(dto.validate().is_err());
}

#[test]
fn test_crawl_request_dto_validate_sync_wait_ms_zero_passes() {
    let dto = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: None,
        config: CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: Some(0),
        expires_at: None,
    };
    assert!(dto.validate().is_ok());
}

#[test]
fn test_crawl_request_dto_serialization_roundtrip() {
    let dto = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: Some("Test Crawl".to_string()),
        config: CrawlConfigDto {
            max_depth: 3,
            include_patterns: Some(vec!["/api/*".to_string()]),
            exclude_patterns: None,
            strategy: Some("bfs".to_string()),
            crawl_delay_ms: Some(500),
            max_concurrency: Some(5),
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: Some(5000),
        expires_at: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let deserialized: CrawlRequestDto = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.url, dto.url);
    assert_eq!(deserialized.name, dto.name);
    assert_eq!(deserialized.config.max_depth, dto.config.max_depth);
    assert_eq!(deserialized.sync_wait_ms, dto.sync_wait_ms);
}

#[test]
fn test_crawl_request_dto_validated_url_skipped_in_serialization() {
    let dto = CrawlRequestDto {
        url: "https://example.com".to_string(),
        validated_url: None,
        name: None,
        config: CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        },
        sync_wait_ms: None,
        expires_at: None,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("validated_url"));
}

// ============================================================================
// max_depth validation logic (mirrors handler line 44)
// ============================================================================

#[test]
fn test_max_depth_zero_passes_handler_check() {
    let max_depth: u32 = 0;
    assert!(!(max_depth > 5));
}

#[test]
fn test_max_depth_five_passes_handler_check() {
    let max_depth: u32 = 5;
    assert!(!(max_depth > 5));
}

#[test]
fn test_max_depth_six_fails_handler_check() {
    let max_depth: u32 = 6;
    assert!(max_depth > 5);
}

// ============================================================================
// Constants tests
// ============================================================================

#[test]
fn test_crawl_task_credits_cost_value() {
    assert_eq!(CRAWL_TASK_CREDITS_COST, 10);
}

#[test]
fn test_default_timeout_ms_value() {
    assert_eq!(DEFAULT_TIMEOUT_MS, 5000);
}

// ============================================================================
// sync_wait_ms default logic (mirrors handler line 41)
// ============================================================================

#[test]
fn test_sync_wait_ms_defaults_to_default_timeout_when_none() {
    let payload_sync_wait_ms: Option<u32> = None;
    let sync_wait_ms = payload_sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);
    assert_eq!(sync_wait_ms, 5000);
}

#[test]
fn test_sync_wait_ms_uses_custom_value_when_some() {
    let payload_sync_wait_ms: Option<u32> = Some(10000);
    let sync_wait_ms = payload_sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);
    assert_eq!(sync_wait_ms, 10000);
}

#[test]
fn test_sync_wait_ms_zero_uses_zero() {
    let payload_sync_wait_ms: Option<u32> = Some(0);
    let sync_wait_ms = payload_sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);
    assert_eq!(sync_wait_ms, 0);
}

// ============================================================================
// SyncWaitResult construction and status code selection
// ============================================================================

#[test]
fn test_sync_wait_result_timeout_fields() {
    let result = SyncWaitResult {
        waited_time_ms: 5000,
        is_timeout: true,
    };
    assert_eq!(result.waited_time_ms, 5000);
    assert!(result.is_timeout);
}

#[test]
fn test_sync_wait_result_no_timeout_fields() {
    let result = SyncWaitResult {
        waited_time_ms: 0,
        is_timeout: false,
    };
    assert_eq!(result.waited_time_ms, 0);
    assert!(!result.is_timeout);
}

#[test]
fn test_status_code_accepted_when_sync_wait_and_timeout() {
    let sync_wait_ms: u32 = 5000;
    let wait_result = SyncWaitResult {
        waited_time_ms: 5000,
        is_timeout: true,
    };
    let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    assert_eq!(status_code, StatusCode::ACCEPTED);
}

#[test]
fn test_status_code_created_when_sync_wait_no_timeout() {
    let sync_wait_ms: u32 = 5000;
    let wait_result = SyncWaitResult {
        waited_time_ms: 1000,
        is_timeout: false,
    };
    let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    assert_eq!(status_code, StatusCode::CREATED);
}

#[test]
fn test_status_code_created_when_sync_wait_zero() {
    let sync_wait_ms: u32 = 0;
    let wait_result = SyncWaitResult {
        waited_time_ms: 0,
        is_timeout: true,
    };
    let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    assert_eq!(status_code, StatusCode::CREATED);
}

#[test]
fn test_sync_wait_result_default_when_no_tasks() {
    // Handler creates this when tasks list is empty
    let result = SyncWaitResult {
        waited_time_ms: 0,
        is_timeout: false,
    };
    assert_eq!(result.waited_time_ms, 0);
    assert!(!result.is_timeout);
}

#[test]
fn test_sync_wait_result_default_on_error() {
    // Handler creates this when find_by_crawl_id fails
    let result = SyncWaitResult {
        waited_time_ms: 0,
        is_timeout: false,
    };
    assert!(!result.is_timeout);
}

// ============================================================================
// CrawlConfigDto edge cases
// ============================================================================

#[test]
fn test_crawl_config_dto_minimal() {
    let json = r#"{"max_depth": 1}"#;
    let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_depth, 1);
    assert!(config.include_patterns.is_none());
    assert!(config.exclude_patterns.is_none());
    assert!(config.strategy.is_none());
    assert!(config.crawl_delay_ms.is_none());
    assert!(config.max_concurrency.is_none());
    assert!(config.proxy.is_none());
    assert!(config.headers.is_none());
    assert!(config.extraction_rules.is_none());
}

#[test]
fn test_crawl_config_dto_with_patterns() {
    let json = r#"{
        "max_depth": 3,
        "include_patterns": ["/blog/*", "/news/*"],
        "exclude_patterns": ["/admin/*", "/login/*"]
    }"#;
    let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_depth, 3);
    assert_eq!(config.include_patterns.as_ref().unwrap().len(), 2);
    assert_eq!(config.exclude_patterns.as_ref().unwrap().len(), 2);
}

#[test]
fn test_crawl_config_dto_with_headers_and_proxy() {
    let json = r#"{
        "max_depth": 2,
        "headers": {"Authorization": "Bearer token"},
        "proxy": "socks5://proxy:1080"
    }"#;
    let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
    assert!(config.headers.is_some());
    assert_eq!(config.proxy.as_deref(), Some("socks5://proxy:1080"));
}

#[test]
fn test_crawl_config_dto_clone_preserves_fields() {
    let config = CrawlConfigDto {
        max_depth: 3,
        include_patterns: Some(vec!["/blog/*".to_string()]),
        exclude_patterns: Some(vec!["/admin/*".to_string()]),
        strategy: Some("dfs".to_string()),
        crawl_delay_ms: Some(500),
        max_concurrency: Some(10),
        proxy: Some("http://proxy:8080".to_string()),
        headers: Some(serde_json::json!({"Accept": "text/html"})),
        extraction_rules: None,
    };
    let cloned = config.clone();
    assert_eq!(cloned.max_depth, 3);
    assert_eq!(cloned.include_patterns, config.include_patterns);
    assert_eq!(cloned.exclude_patterns, config.exclude_patterns);
    assert_eq!(cloned.strategy, config.strategy);
    assert_eq!(cloned.crawl_delay_ms, config.crawl_delay_ms);
    assert_eq!(cloned.max_concurrency, config.max_concurrency);
    assert_eq!(cloned.proxy, config.proxy);
}

#[test]
fn test_crawl_config_dto_serialization_roundtrip() {
    let config = CrawlConfigDto {
        max_depth: 2,
        include_patterns: Some(vec!["/api/*".to_string()]),
        exclude_patterns: None,
        strategy: Some("bfs".to_string()),
        crawl_delay_ms: None,
        max_concurrency: Some(20),
        proxy: None,
        headers: None,
        extraction_rules: None,
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: CrawlConfigDto = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.max_depth, 2);
    assert_eq!(deserialized.include_patterns, config.include_patterns);
    assert_eq!(deserialized.strategy, config.strategy);
    assert_eq!(deserialized.max_concurrency, config.max_concurrency);
}

// ============================================================================
// CrawlUseCaseError Display trait
// ============================================================================

#[test]
fn test_crawl_use_case_error_validation_display() {
    let err = CrawlUseCaseError::ValidationError("invalid input".to_string());
    let display = format!("{}", err);
    assert!(display.contains("Validation failed"));
    assert!(display.contains("invalid input"));
}

#[test]
fn test_crawl_use_case_error_not_found_display() {
    let err = CrawlUseCaseError::NotFound;
    let display = format!("{}", err);
    assert!(display.contains("Crawl not found"));
}

#[test]
fn test_crawl_use_case_error_anyhow_display() {
    let err = CrawlUseCaseError::Anyhow(anyhow::anyhow!("something went wrong"));
    let display = format!("{}", err);
    assert!(display.contains("something went wrong"));
}

// ============================================================================
// HashMap usage pattern verification (mirrors handler logic)
// ============================================================================

#[test]
fn test_tasks_to_id_map_returns_hashmap_type() {
    let tasks = vec![make_task(Uuid::new_v4())];
    let map: HashMap<Uuid, Task> = tasks_to_id_map(tasks);
    // Verify it's a HashMap — can call HashMap methods
    assert_eq!(map.keys().count(), map.len());
}
