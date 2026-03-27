// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl use case tests
//!
//! Tests for the CrawlUseCase including crawl creation, validation, and querying

use std::sync::Arc;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crawlrs::application::dto::crawl_request::{CrawlConfigDto, CrawlRequestDto};
use crawlrs::application::use_cases::crawl_use_case::{CrawlUseCase, CrawlUseCaseError};
use crawlrs::domain::models::crawl::{Crawl, CrawlStatus};
use crawlrs::domain::models::scrape_result::ScrapeResult;
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crawlrs::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crawlrs::domain::repositories::task_repository::{RepositoryError, TaskRepository};
use crawlrs::domain::repositories::webhook_repository::WebhookRepository;
use crawlrs::domain::services::team_service::{GeoRestrictionResult, TeamService};

// === Mock Repositories and Services ===

struct MockCrawlRepository {
    crawls: Arc<std::sync::Mutex<Vec<Crawl>>>,
}

impl MockCrawlRepository {
    fn new() -> Self {
        Self {
            crawls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl CrawlRepository for MockCrawlRepository {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, anyhow::Error> {
        let mut crawls = self.crawls.lock().unwrap();
        crawls.push(crawl.clone());
        Ok(crawl.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, anyhow::Error> {
        let crawls = self.crawls.lock().unwrap();
        Ok(crawls.iter().find(|c| c.id == id).cloned())
    }

    async fn update(&self, crawl: &Crawl) -> Result<Crawl, anyhow::Error> {
        let mut crawls = self.crawls.lock().unwrap();
        if let Some(c) = crawls.iter_mut().find(|c| c.id == crawl.id) {
            *c = crawl.clone();
            Ok(crawl.clone())
        } else {
            Err(anyhow::anyhow!("Crawl not found"))
        }
    }

    async fn delete(&self, _id: Uuid) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

struct MockTaskRepository {
    tasks: Arc<std::sync::Mutex<Vec<crate::domain::models::task::Task>>>,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &crate::domain::models::task::Task) -> Result<crate::domain::models::task::Task, RepositoryError> {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.push(task.clone());
        Ok(task.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<crate::domain::models::task::Task>, RepositoryError> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.iter().find(|t| t.id == id).cloned())
    }

    async fn update(&self, task: &crate::domain::models::task::Task) -> Result<crate::domain::models::task::Task, RepositoryError> {
        Ok(task.clone())
    }

    async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<crate::domain::models::task::Task>, RepositoryError> {
        Ok(None)
    }

    async fn mark_completed(&self, _id: Uuid) -> Result<crate::domain::models::task::Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn mark_failed(&self, _id: Uuid) -> Result<crate::domain::models::task::Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn mark_cancelled(&self, _id: Uuid) -> Result<crate::domain::models::task::Task, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn query_tasks(&self, _filters: &crate::application::dto::task_query_request::TaskQueryRequest) -> Result<Vec<crate::domain::models::task::Task>, RepositoryError> {
        Ok(vec![])
    }

    async fn batch_cancel(&self, _ids: &[Uuid]) -> Result<usize, RepositoryError> {
        Ok(0)
    }

    async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<crate::domain::models::task::Task>, RepositoryError> {
        Ok(vec![])
    }

    async fn reset_stuck_tasks(&self, _duration: chrono::Duration) -> Result<usize, RepositoryError> {
        Ok(0)
    }

    async fn expire_tasks(&self) -> Result<usize, RepositoryError> {
        Ok(0)
    }
}

struct MockTeamService;

impl MockTeamService {
    fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl TeamService for MockTeamService {
    async fn validate_geographic_restriction(
        &self,
        _team_id: Uuid,
        _client_ip: &str,
        _restrictions: &[crate::domain::models::geo_restriction::GeoRestriction],
    ) -> Result<GeoRestrictionResult, anyhow::Error> {
        Ok(GeoRestrictionResult::Allowed)
    }
}

struct MockWebhookRepository;

#[async_trait::async_trait]
impl WebhookRepository for MockWebhookRepository {
    async fn create(&self, _webhook: &crate::domain::models::webhook_model::Webhook) -> Result<crate::domain::models::webhook_model::Webhook, anyhow::Error> {
        Err(anyhow::anyhow!("Not implemented"))
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<Option<crate::domain::models::webhook_model::Webhook>, anyhow::Error> {
        Ok(None)
    }

    async fn update(&self, _webhook: &crate::domain::models::webhook_model::Webhook) -> Result<crate::domain::models::webhook_model::Webhook, anyhow::Error> {
        Err(anyhow::anyhow!("Not implemented"))
    }

    async fn delete(&self, _id: Uuid) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<crate::domain::models::webhook_model::Webhook>, anyhow::Error> {
        Ok(vec![])
    }
}

struct MockScrapeResultRepository;

#[async_trait::async_trait]
impl ScrapeResultRepository for MockScrapeResultRepository {
    async fn create(&self, _result: &ScrapeResult) -> Result<ScrapeResult, anyhow::Error> {
        Err(anyhow::anyhow!("Not implemented"))
    }

    async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> Result<Vec<ScrapeResult>, anyhow::Error> {
        Ok(vec![])
    }
}

struct MockGeoRestrictionRepository;

#[async_trait::async_trait]
impl GeoRestrictionRepository for MockGeoRestrictionRepository {
    async fn get_team_restrictions(&self, _team_id: Uuid) -> Result<Vec<crate::domain::models::geo_restriction::GeoRestriction>, anyhow::Error> {
        Ok(vec![])
    }

    async fn log_geo_restriction_action(&self, _team_id: Uuid, _client_ip: &str, _country_code: &str, _action: &str, _reason: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

// === Helper Functions ===

fn create_test_crawl_request() -> CrawlRequestDto {
    CrawlRequestDto {
        url: "https://example.com".to_string(),
        name: Some("Test Crawl".to_string()),
        config: CrawlConfigDto {
            max_depth: 2,
            max_concurrency: Some(5),
            follow_links: true,
            respect_robots_txt: true,
            allowed_domains: None,
            proxy: None,
            expires_at: None,
        },
    }
}

fn create_test_use_case() -> CrawlUseCase {
    let crawl_repo = Arc::new(MockCrawlRepository::new());
    let task_repo = Arc::new(MockTaskRepository::new());
    let webhook_repo = Arc::new(MockWebhookRepository);
    let scrape_result_repo = Arc::new(MockScrapeResultRepository);
    let geo_restriction_repo = Arc::new(MockGeoRestrictionRepository);
    let team_service = Arc::new(MockTeamService::new());

    CrawlUseCase::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        geo_restriction_repo,
        team_service,
    )
}

// === Unit Tests ===

#[tokio::test]
async fn test_create_crawl_success() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let dto = create_test_crawl_request();
    let client_ip = "127.0.0.1";

    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_ok());
    let crawl = result.unwrap();
    assert_eq!(crawl.status, CrawlStatus::Queued);
}

#[tokio::test]
async fn test_create_crawl_invalid_max_depth() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let mut dto = create_test_crawl_request();
    dto.config.max_depth = 10; // Invalid: > 5

    let client_ip = "127.0.0.1";
    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CrawlUseCaseError::ValidationError(_)
    ));
}

#[tokio::test]
async fn test_create_crawl_invalid_max_concurrency() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let mut dto = create_test_crawl_request();
    dto.config.max_concurrency = Some(101); // Invalid: > 100

    let client_ip = "127.0.0.1";
    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_crawl_results() {
    let use_case = create_test_use_case();

    let crawl_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = use_case.get_crawl_results(crawl_id, team_id).await;

    // Should return empty results when crawl not found
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), CrawlUseCaseError::NotFound));
}

#[tokio::test]
async fn test_get_crawl_success() {
    let use_case = create_test_use_case();

    let crawl_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = use_case.get_crawl(crawl_id, team_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_cancel_crawl_not_found() {
    let use_case = create_test_use_case();

    let crawl_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let result = use_case.cancel_crawl(crawl_id, team_id).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), CrawlUseCaseError::NotFound));
}

// === Error Handling Tests ===

#[test]
fn test_crawl_use_case_error_display() {
    let error = CrawlUseCaseError::ValidationError("test error".to_string());
    assert_eq!(format!("{}", error), "Validation failed: test error");

    let error = CrawlUseCaseError::NotFound;
    assert_eq!(format!("{}", error), "Crawl not found");
}

#[test]
fn test_crawl_use_case_error_from_repository() {
    let repo_error = RepositoryError::Database("db error".to_string());
    let use_case_error: CrawlUseCaseError = repo_error.into();
    assert!(matches!(
        use_case_error,
        CrawlUseCaseError::Repository(_)
    ));
}

// === Integration-like Tests ===

#[tokio::test]
async fn test_create_crawl_with_custom_name() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let mut dto = create_test_crawl_request();
    dto.name = Some("Custom Crawl Name".to_string());

    let client_ip = "127.0.0.1";
    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_ok());
    let crawl = result.unwrap();
    assert_eq!(crawl.name, "Custom Crawl Name");
}

#[tokio::test]
async fn test_create_crawl_default_name() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let mut dto = create_test_crawl_request();
    dto.name = None;

    let client_ip = "127.0.0.1";
    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_ok());
    let crawl = result.unwrap();
    assert_eq!(crawl.name, "Untitled Crawl");
}

#[tokio::test]
async fn test_create_crawl_with_expires_at() {
    let use_case = create_test_use_case();

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let mut dto = create_test_crawl_request();
    dto.config.expires_at = Some(Utc::now() + chrono::Duration::hours(1));

    let client_ip = "127.0.0.1";
    let result = use_case.create_crawl(team_id, api_key_id, dto, client_ip).await;

    assert!(result.is_ok());
}
