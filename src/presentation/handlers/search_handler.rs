// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::IntoResponse,
};
use log::error;
use std::sync::Arc;

use crate::{
    application::dto::search_request::{SearchRequestDto, SearchResponseDto, SearchResultDto},
    common::constants::crawl_task,
    domain::{
        repositories::task_repository::TaskRepository,
        services::rate_limiting_service::RateLimitingService,
        services::search_service::{SearchQuery, SearchServiceError, SearchServiceTrait},
    },
    presentation::handlers::response_builder::{error_response, success_response},
    presentation::handlers::task_handler::wait_for_tasks_completion,
    presentation::helpers::rate_limit_helper::check_rate_limit,
    presentation::middleware::auth_middleware::AuthState,
};

/// 处理搜索请求
pub async fn search(
    Extension(search_service): Extension<Arc<dyn SearchServiceTrait>>,
    Extension(task_repo): Extension<Arc<dyn TaskRepository>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<SearchRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key_id = auth_state.api_key_id;
    let api_key = api_key_id.to_string();

    // 1. 检查限流
    if let Err(response) =
        check_rate_limit(rate_limiting_service.as_ref(), &api_key, "/v1/search").await
    {
        return response;
    }

    // 2. 检查配额 (SearchService 内部已经处理了配额检查)

    let sync_wait_ms = payload
        .sync_wait_ms
        .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);

    // 将 DTO 转换为领域参数
    let search_query = SearchQuery {
        query: payload.query,
        limit: payload.limit,
        lang: payload.lang,
        country: payload.country,
        engine: payload.engine,
        sources: payload.sources,
        crawl_results: payload.crawl_results,
        crawl_config: payload.crawl_config.map(|c| {
            crate::domain::services::search_service::SearchCrawlConfig {
                max_depth: c.max_depth,
                include_patterns: c.include_patterns,
                exclude_patterns: c.exclude_patterns,
                strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                crawl_delay_ms: c.crawl_delay_ms,
                max_concurrency: c.max_concurrency.unwrap_or(10),
                headers: c.headers,
                proxy: c.proxy,
                extraction_rules: c.extraction_rules,
            }
        }),
    };

    // 使用注入的SearchService
    match search_service
        .search(team_id, api_key_id, search_query)
        .await
    {
        Ok(response) => {
            // 如果启用了爬取结果并且有crawl_id，则等待任务完成
            if sync_wait_ms > 0 {
                if let Some(crawl_id) = response.crawl_id {
                    match task_repo.find_by_crawl_id(crawl_id).await {
                        Ok(tasks) => {
                            if !tasks.is_empty() {
                                let task_ids: Vec<uuid::Uuid> =
                                    tasks.iter().map(|task| task.id).collect();
                                match wait_for_tasks_completion(
                                    task_repo.as_ref(),
                                    &task_ids,
                                    team_id,
                                    sync_wait_ms,
                                    crawl_task::BASE_POLL_INTERVAL_MS,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        // 等待成功，可以返回响应
                                    }
                                    Err(e) => {
                                        error!("Failed to wait for task completion: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to find tasks for crawl {}: {:?}", crawl_id, e);
                        }
                    }
                }
            }

            // 将领域响应转换为 DTO
            let response_dto = SearchResponseDto {
                query: response.query,
                results: response
                    .results
                    .into_iter()
                    .map(|r| SearchResultDto {
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        engine: Some(r.engine),
                    })
                    .collect(),
                crawl_id: response.crawl_id,
                credits_used: response.credits_used,
            };

            success_response(StatusCode::OK, response_dto)
        }
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

impl From<SearchServiceError> for (StatusCode, String) {
    fn from(err: SearchServiceError) -> Self {
        match err {
            SearchServiceError::ValidationError(details) => (StatusCode::BAD_REQUEST, details),
            SearchServiceError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            SearchServiceError::CreditsRepository(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
            SearchServiceError::InsufficientCredits {
                available,
                required,
            } => {
                let details = format!(
                    "Insufficient credits: available {}, required {}",
                    available, required
                );
                (StatusCode::PAYMENT_REQUIRED, details)
            }
            SearchServiceError::SearchEngine(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::credits_repository::CreditsRepositoryError;
    use crate::domain::repositories::task_repository::RepositoryError;

    // ========== From<SearchServiceError> mapping tests ==========

    #[test]
    fn test_validation_error_maps_to_bad_request() {
        let err = SearchServiceError::ValidationError("invalid query".to_string());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(msg, "invalid query");
    }

    #[test]
    fn test_repository_error_maps_to_internal_server_error() {
        let err = SearchServiceError::Repository(RepositoryError::Database(anyhow::anyhow!(
            "db connection failed"
        )));
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("db connection failed"));
    }

    #[test]
    fn test_repository_not_found_maps_to_internal_server_error() {
        let err = SearchServiceError::Repository(RepositoryError::NotFound);
        let (status, _msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_credits_repository_error_maps_to_internal_server_error() {
        let err = SearchServiceError::CreditsRepository(CreditsRepositoryError::DatabaseError(
            "credits db error".to_string(),
        ));
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("credits db error"));
    }

    #[test]
    fn test_credits_not_found_maps_to_internal_server_error() {
        let team_id = uuid::Uuid::new_v4();
        let err =
            SearchServiceError::CreditsRepository(CreditsRepositoryError::CreditsNotFound(team_id));
        let (status, _msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_insufficient_credits_maps_to_payment_required() {
        let err = SearchServiceError::InsufficientCredits {
            available: 5,
            required: 10,
        };
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert!(msg.contains("Insufficient credits"));
        assert!(msg.contains("available 5"));
        assert!(msg.contains("required 10"));
    }

    #[test]
    fn test_insufficient_credits_zero_available() {
        let err = SearchServiceError::InsufficientCredits {
            available: 0,
            required: 1,
        };
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert!(msg.contains("available 0"));
    }

    #[test]
    fn test_search_engine_error_maps_to_internal_server_error() {
        let err = SearchServiceError::SearchEngine("rate limited by Google".to_string());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(msg, "rate limited by Google");
    }

    // ========== SearchRequestDto construction tests ==========

    #[test]
    fn test_search_request_dto_minimal() {
        let json = r#"{"query":"rust web scraping"}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.query, "rust web scraping");
        assert!(dto.engine.is_none());
        assert!(dto.sources.is_none());
        assert!(dto.limit.is_none());
        assert!(dto.lang.is_none());
        assert!(dto.country.is_none());
        assert!(dto.crawl_config.is_none());
        assert!(dto.crawl_results.is_none());
        assert!(dto.sync_wait_ms.is_none());
    }

    #[test]
    fn test_search_request_dto_full() {
        let json = r#"{
            "query": "rust async",
            "engine": "google",
            "sources": ["google", "bing"],
            "limit": 10,
            "lang": "en",
            "country": "US",
            "crawl_results": true,
            "sync_wait_ms": 5000,
            "crawl_config": {
                "max_depth": 2,
                "include_patterns": ["/blog/*"],
                "exclude_patterns": ["/admin/*"],
                "strategy": "bfs",
                "crawl_delay_ms": 1000,
                "max_concurrency": 5,
                "proxy": "http://proxy:8080",
                "headers": {"X-Custom": "value"}
            }
        }"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.query, "rust async");
        assert_eq!(dto.engine.as_deref(), Some("google"));
        assert!(dto.sources.is_some());
        assert_eq!(dto.sources.as_ref().unwrap().len(), 2);
        assert_eq!(dto.limit, Some(10));
        assert_eq!(dto.lang.as_deref(), Some("en"));
        assert_eq!(dto.country.as_deref(), Some("US"));
        assert_eq!(dto.crawl_results, Some(true));
        assert_eq!(dto.sync_wait_ms, Some(5000));
        assert!(dto.crawl_config.is_some());
    }

    #[test]
    fn test_search_request_dto_deny_unknown_fields() {
        let json = r#"{"query":"test","unknown_field":42}"#;
        let result: Result<SearchRequestDto, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn test_search_request_dto_with_sources_alias() {
        let json = r#"{"query":"test","sources":["google","bing","baidu"]}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.sources.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_search_request_dto_with_crawl_results_false() {
        let json = r#"{"query":"test","crawl_results":false}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.crawl_results, Some(false));
    }

    #[test]
    fn test_search_request_dto_with_crawl_config_minimal() {
        let json = r#"{"query":"test","crawl_config":{"max_depth":1}}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        let config = dto.crawl_config.unwrap();
        assert_eq!(config.max_depth, 1);
        assert!(config.strategy.is_none());
        assert!(config.max_concurrency.is_none());
    }

    // ========== SearchResponseDto construction ==========

    #[test]
    fn test_search_response_dto_construction() {
        let response = SearchResponseDto {
            query: "test query".to_string(),
            results: vec![],
            crawl_id: None,
            credits_used: 5,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["query"], "test query");
        assert_eq!(parsed["results"].as_array().unwrap().len(), 0);
        assert!(parsed["crawl_id"].is_null());
        assert_eq!(parsed["credits_used"], 5);
    }

    #[test]
    fn test_search_response_dto_with_results() {
        let response = SearchResponseDto {
            query: "rust".to_string(),
            results: vec![
                SearchResultDto {
                    title: "Rust Programming".to_string(),
                    url: "https://rust-lang.org".to_string(),
                    description: Some("Official site".to_string()),
                    engine: Some("google".to_string()),
                },
                SearchResultDto {
                    title: "Learn Rust".to_string(),
                    url: "https://doc.rust-lang.org".to_string(),
                    description: None,
                    engine: Some("bing".to_string()),
                },
            ],
            crawl_id: Some(uuid::Uuid::new_v4()),
            credits_used: 1,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["results"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["results"][0]["title"], "Rust Programming");
        assert!(parsed["results"][1]["description"].is_null());
    }

    // ========== SearchResultDto tests ==========

    #[test]
    fn test_search_result_dto_serialization() {
        let result = SearchResultDto {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            description: Some("A test result".to_string()),
            engine: Some("google".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["title"], "Test");
        assert_eq!(parsed["url"], "https://example.com");
        assert_eq!(parsed["description"], "A test result");
        assert_eq!(parsed["engine"], "google");
    }

    #[test]
    fn test_search_result_dto_clone() {
        let result = SearchResultDto {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            description: None,
            engine: None,
        };
        let cloned = result.clone();
        assert_eq!(result.title, cloned.title);
        assert_eq!(result.url, cloned.url);
        assert_eq!(result.description, cloned.description);
        assert_eq!(result.engine, cloned.engine);
    }

    #[test]
    fn test_search_result_dto_debug() {
        let result = SearchResultDto {
            title: "Debug Test".to_string(),
            url: "https://debug.com".to_string(),
            description: Some("debugging".to_string()),
            engine: Some("baidu".to_string()),
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("SearchResultDto"));
        assert!(debug.contains("Debug Test"));
        assert!(debug.contains("https://debug.com"));
    }

    #[test]
    fn test_search_result_dto_deserialization() {
        let json = r#"{"title":"Test","url":"https://example.com","description":"desc","engine":"google"}"#;
        let result: SearchResultDto = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "Test");
        assert_eq!(result.url, "https://example.com");
        assert_eq!(result.description.as_deref(), Some("desc"));
        assert_eq!(result.engine.as_deref(), Some("google"));
    }

    #[test]
    fn test_search_result_dto_deserialization_without_optional_fields() {
        let json = r#"{"title":"Test","url":"https://example.com"}"#;
        let result: SearchResultDto = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "Test");
        assert!(result.description.is_none());
        assert!(result.engine.is_none());
    }

    // ========== SearchQuery construction ==========

    #[test]
    fn test_search_query_from_dto_no_crawl_config() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: Some("google".to_string()),
            sources: Some(vec!["google".to_string()]),
            limit: Some(10),
            lang: Some("en".to_string()),
            country: Some("US".to_string()),
            crawl_config: None,
            crawl_results: Some(true),
            sync_wait_ms: Some(5000),
        };
        // Simulate the handler's conversion to SearchQuery
        let search_query = SearchQuery {
            query: dto.query,
            limit: dto.limit,
            lang: dto.lang,
            country: dto.country,
            engine: dto.engine,
            sources: dto.sources,
            crawl_results: dto.crawl_results,
            crawl_config: dto.crawl_config.map(|c| {
                crate::domain::services::search_service::SearchCrawlConfig {
                    max_depth: c.max_depth,
                    include_patterns: c.include_patterns,
                    exclude_patterns: c.exclude_patterns,
                    strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                    crawl_delay_ms: c.crawl_delay_ms,
                    max_concurrency: c.max_concurrency.unwrap_or(10),
                    headers: c.headers,
                    proxy: c.proxy,
                    extraction_rules: c.extraction_rules,
                }
            }),
        };
        assert_eq!(search_query.query, "test");
        assert!(search_query.crawl_config.is_none());
    }

    #[test]
    fn test_search_query_from_dto_with_crawl_config_defaults() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: Some(crate::application::dto::crawl_request::CrawlConfigDto {
                max_depth: 3,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None, // Should default to "bfs"
                crawl_delay_ms: None,
                max_concurrency: None, // Should default to 10
                proxy: None,
                headers: None,
                extraction_rules: None,
            }),
            crawl_results: None,
            sync_wait_ms: None,
        };
        let search_query = SearchQuery {
            query: dto.query,
            limit: dto.limit,
            lang: dto.lang,
            country: dto.country,
            engine: dto.engine,
            sources: dto.sources,
            crawl_results: dto.crawl_results,
            crawl_config: dto.crawl_config.map(|c| {
                crate::domain::services::search_service::SearchCrawlConfig {
                    max_depth: c.max_depth,
                    include_patterns: c.include_patterns,
                    exclude_patterns: c.exclude_patterns,
                    strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                    crawl_delay_ms: c.crawl_delay_ms,
                    max_concurrency: c.max_concurrency.unwrap_or(10),
                    headers: c.headers,
                    proxy: c.proxy,
                    extraction_rules: c.extraction_rules,
                }
            }),
        };
        let config = search_query.crawl_config.unwrap();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.strategy, "bfs"); // default
        assert_eq!(config.max_concurrency, 10); // default
    }

    #[test]
    fn test_search_query_from_dto_with_crawl_config_explicit_values() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: Some(crate::application::dto::crawl_request::CrawlConfigDto {
                max_depth: 5,
                include_patterns: Some(vec!["/api/*".to_string()]),
                exclude_patterns: Some(vec!["/admin/*".to_string()]),
                strategy: Some("dfs".to_string()),
                crawl_delay_ms: Some(2000),
                max_concurrency: Some(20),
                proxy: Some("http://proxy:8080".to_string()),
                headers: Some(serde_json::json!({"Accept": "text/html"})),
                extraction_rules: None,
            }),
            crawl_results: None,
            sync_wait_ms: None,
        };
        let search_query = SearchQuery {
            query: dto.query,
            limit: dto.limit,
            lang: dto.lang,
            country: dto.country,
            engine: dto.engine,
            sources: dto.sources,
            crawl_results: dto.crawl_results,
            crawl_config: dto.crawl_config.map(|c| {
                crate::domain::services::search_service::SearchCrawlConfig {
                    max_depth: c.max_depth,
                    include_patterns: c.include_patterns,
                    exclude_patterns: c.exclude_patterns,
                    strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                    crawl_delay_ms: c.crawl_delay_ms,
                    max_concurrency: c.max_concurrency.unwrap_or(10),
                    headers: c.headers,
                    proxy: c.proxy,
                    extraction_rules: c.extraction_rules,
                }
            }),
        };
        let config = search_query.crawl_config.unwrap();
        assert_eq!(config.max_depth, 5);
        assert_eq!(config.strategy, "dfs"); // explicit, not default
        assert_eq!(config.max_concurrency, 20); // explicit, not default
        assert_eq!(config.crawl_delay_ms, Some(2000));
        assert!(config.include_patterns.is_some());
        assert!(config.exclude_patterns.is_some());
        assert!(config.proxy.is_some());
        assert!(config.headers.is_some());
    }

    // ========== Additional From<SearchServiceError> tests ==========

    #[test]
    fn test_validation_error_empty_message() {
        let err = SearchServiceError::ValidationError(String::new());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_insufficient_credits_large_numbers() {
        let err = SearchServiceError::InsufficientCredits {
            available: 999999,
            required: 1000000,
        };
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert!(msg.contains("999999"));
        assert!(msg.contains("1000000"));
    }

    #[test]
    fn test_insufficient_credits_negative_available() {
        let err = SearchServiceError::InsufficientCredits {
            available: -5,
            required: 10,
        };
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert!(msg.contains("-5"));
    }

    #[test]
    fn test_search_engine_error_empty_string() {
        let err = SearchServiceError::SearchEngine(String::new());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_repository_error_not_found_variant() {
        let err = SearchServiceError::Repository(RepositoryError::NotFound);
        let (status, _msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_credits_repository_insufficient_credits_variant() {
        let team_id = uuid::Uuid::new_v4();
        let err =
            SearchServiceError::CreditsRepository(CreditsRepositoryError::InsufficientCredits {
                available: 0,
                required: 10,
            });
        let (status, _msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let _ = team_id; // suppress unused warning
    }

    // ========== sync_wait_ms default logic ==========

    #[test]
    fn test_sync_wait_ms_default_when_none() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: None,
            crawl_results: None,
            sync_wait_ms: None,
        };
        let sync_wait_ms = dto
            .sync_wait_ms
            .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);
        assert_eq!(sync_wait_ms, 5000);
    }

    #[test]
    fn test_sync_wait_ms_custom_when_some() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: None,
            crawl_results: None,
            sync_wait_ms: Some(10000),
        };
        let sync_wait_ms = dto
            .sync_wait_ms
            .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);
        assert_eq!(sync_wait_ms, 10000);
    }

    #[test]
    fn test_sync_wait_ms_zero() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: None,
            crawl_results: None,
            sync_wait_ms: Some(0),
        };
        let sync_wait_ms = dto
            .sync_wait_ms
            .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);
        assert_eq!(sync_wait_ms, 0);
    }

    // ========== SearchResponseDto deserialization ==========

    #[test]
    fn test_search_response_dto_deserialization() {
        let json = format!(r#"{{"query":"test","results":[],"crawl_id":null,"credits_used":0}}"#);
        let dto: SearchResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.query, "test");
        assert!(dto.results.is_empty());
        assert!(dto.crawl_id.is_none());
        assert_eq!(dto.credits_used, 0);
    }

    #[test]
    fn test_search_response_dto_with_crawl_id() {
        let crawl_id = uuid::Uuid::new_v4();
        let json = format!(
            r#"{{"query":"test","results":[],"crawl_id":"{}","credits_used":3}}"#,
            crawl_id
        );
        let dto: SearchResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.crawl_id, Some(crawl_id));
        assert_eq!(dto.credits_used, 3);
    }

    #[test]
    fn test_search_response_dto_with_results_deserialization() {
        let json = r#"{
            "query": "rust",
            "results": [
                {"title":"Result 1","url":"https://1.com","description":"desc 1","engine":"google"},
                {"title":"Result 2","url":"https://2.com","description":null,"engine":"bing"}
            ],
            "crawl_id": null,
            "credits_used": 2
        }"#;
        let dto: SearchResponseDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.query, "rust");
        assert_eq!(dto.results.len(), 2);
        assert_eq!(dto.results[0].title, "Result 1");
        assert_eq!(dto.results[1].description, None);
    }

    // ========== SearchResponseDto serialization roundtrip ==========

    #[test]
    fn test_search_response_dto_serialization_roundtrip() {
        let original = SearchResponseDto {
            query: "roundtrip test".to_string(),
            results: vec![SearchResultDto {
                title: "Title".to_string(),
                url: "https://example.com".to_string(),
                description: Some("Desc".to_string()),
                engine: Some("google".to_string()),
            }],
            crawl_id: Some(uuid::Uuid::new_v4()),
            credits_used: 7,
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: SearchResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.query, original.query);
        assert_eq!(deserialized.results.len(), 1);
        assert_eq!(deserialized.results[0].title, "Title");
        assert_eq!(deserialized.credits_used, 7);
    }

    // ========== SearchServiceError Display trait ==========

    #[test]
    fn test_search_service_error_validation_display() {
        let err = SearchServiceError::ValidationError("bad query".to_string());
        let display = format!("{}", err);
        assert!(display.contains("bad query"));
    }

    #[test]
    fn test_search_service_error_insufficient_credits_display() {
        let err = SearchServiceError::InsufficientCredits {
            available: 5,
            required: 10,
        };
        let display = format!("{}", err);
        // The error message should contain information about credits
        assert!(display.contains("5") || display.contains("10") || display.contains("credit"));
    }

    #[test]
    fn test_search_service_error_search_engine_display() {
        let err = SearchServiceError::SearchEngine("engine timeout".to_string());
        let display = format!("{}", err);
        assert!(display.contains("engine timeout"));
    }

    // ========== SearchRequestDto with empty query ==========

    #[test]
    fn test_search_request_dto_empty_query() {
        let json = r#"{"query":""}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.query, "");
    }

    #[test]
    fn test_search_request_dto_long_query() {
        let long_query = "a".repeat(1000);
        let json = serde_json::json!({ "query": long_query }).to_string();
        let dto: SearchRequestDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.query.len(), 1000);
    }

    // ========== SearchRequestDto with special characters ==========

    #[test]
    fn test_search_request_dto_with_unicode_query() {
        let json = r#"{"query":"中文搜索 日本語 한국어"}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.query.contains("中文"));
        assert!(dto.query.contains("日本語"));
    }

    #[test]
    fn test_search_request_dto_with_special_chars_query() {
        let json = r#"{"query":"test \"quotes\" & <html>"}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.query.contains("quotes"));
        assert!(dto.query.contains("<html>"));
    }

    // ========== SearchRequestDto with limit edge cases ==========

    #[test]
    fn test_search_request_dto_with_limit_zero() {
        let json = r#"{"query":"test","limit":0}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.limit, Some(0));
    }

    #[test]
    fn test_search_request_dto_with_large_limit() {
        let json = r#"{"query":"test","limit":4294967295}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.limit, Some(u32::MAX));
    }

    // ========== SearchQuery with all fields None ==========

    #[test]
    fn test_search_query_all_none() {
        let search_query = SearchQuery {
            query: "minimal".to_string(),
            limit: None,
            lang: None,
            country: None,
            engine: None,
            sources: None,
            crawl_results: None,
            crawl_config: None,
        };
        assert_eq!(search_query.query, "minimal");
        assert!(search_query.limit.is_none());
        assert!(search_query.crawl_config.is_none());
    }

    // ========== crawl_config conversion with extraction_rules ==========

    #[test]
    fn test_search_query_from_dto_with_crawl_config_extraction_rules() {
        let dto = SearchRequestDto {
            query: "test".to_string(),
            engine: None,
            sources: None,
            limit: None,
            lang: None,
            country: None,
            crawl_config: Some(crate::application::dto::crawl_request::CrawlConfigDto {
                max_depth: 2,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: Some(std::collections::HashMap::new()),
            }),
            crawl_results: None,
            sync_wait_ms: None,
        };
        let search_query = SearchQuery {
            query: dto.query,
            limit: dto.limit,
            lang: dto.lang,
            country: dto.country,
            engine: dto.engine,
            sources: dto.sources,
            crawl_results: dto.crawl_results,
            crawl_config: dto.crawl_config.map(|c| {
                crate::domain::services::search_service::SearchCrawlConfig {
                    max_depth: c.max_depth,
                    include_patterns: c.include_patterns,
                    exclude_patterns: c.exclude_patterns,
                    strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                    crawl_delay_ms: c.crawl_delay_ms,
                    max_concurrency: c.max_concurrency.unwrap_or(10),
                    headers: c.headers,
                    proxy: c.proxy,
                    extraction_rules: c.extraction_rules,
                }
            }),
        };
        let config = search_query.crawl_config.unwrap();
        assert_eq!(config.max_depth, 2);
        assert_eq!(config.strategy, "bfs"); // default
        assert_eq!(config.max_concurrency, 10); // default
        assert!(config.extraction_rules.is_some());
    }

    // ========== SearchRequestDto with multiple sources ==========

    #[test]
    fn test_search_request_dto_with_multiple_sources() {
        let json = r#"{
            "query": "test",
            "sources": ["google", "bing", "baidu", "sogou"]
        }"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.sources.as_ref().unwrap().len(), 4);
    }

    // ========== SearchRequestDto with crawl_results false ==========

    #[test]
    fn test_search_request_dto_crawl_results_false_no_crawl_config() {
        let json = r#"{"query":"test","crawl_results":false}"#;
        let dto: SearchRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.crawl_results, Some(false));
        assert!(dto.crawl_config.is_none());
    }

    // ========== From<SearchServiceError> with specific messages ==========

    #[test]
    fn test_validation_error_with_special_characters() {
        let err = SearchServiceError::ValidationError("error with <>&\"".to_string());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(msg.contains("<"));
        assert!(msg.contains(">"));
    }

    #[test]
    fn test_search_engine_error_with_url() {
        let err =
            SearchServiceError::SearchEngine("failed to fetch https://google.com".to_string());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("https://google.com"));
    }

    // ========== SearchResponseDto with empty results ==========

    #[test]
    fn test_search_response_dto_empty_results() {
        let response = SearchResponseDto {
            query: "empty".to_string(),
            results: vec![],
            crawl_id: None,
            credits_used: 0,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["results"].as_array().unwrap().len(), 0);
    }

    // ========== SearchRequestDto serialization ==========

    #[test]
    fn test_search_request_dto_serialization() {
        let dto = SearchRequestDto {
            query: "serialize test".to_string(),
            engine: Some("google".to_string()),
            sources: Some(vec!["google".to_string()]),
            limit: Some(10),
            lang: Some("en".to_string()),
            country: Some("US".to_string()),
            crawl_config: None,
            crawl_results: Some(true),
            sync_wait_ms: Some(3000),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["query"], "serialize test");
        assert_eq!(parsed["engine"], "google");
        assert_eq!(parsed["limit"], 10);
        assert_eq!(parsed["crawl_results"], true);
        assert_eq!(parsed["sync_wait_ms"], 3000);
    }
}
