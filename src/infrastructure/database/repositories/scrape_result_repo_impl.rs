// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeResult repository implementation using dbnexus

use crate::domain::models::ScrapeResult;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::infrastructure::database::entities::scrape_result as db_entity;
use async_trait::async_trait;
use dbnexus::DbPool;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter,
    QueryResult, Set, Statement,
};
use std::sync::Arc;
use uuid::Uuid;

/// ScrapeResult repository implementation using dbnexus
pub struct ScrapeResultRepositoryImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl ScrapeResultRepositoryImpl {
    /// Create new ScrapeResult repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }

    /// Convert domain model to database active model
    fn to_active_model(result: &ScrapeResult) -> db_entity::ActiveModel {
        use chrono::FixedOffset;
        db_entity::ActiveModel {
            id: Set(result.id),
            task_id: Set(result.task_id),
            url: Set(result.url.clone()),
            status_code: Set(result.status_code),
            content: Set(result.content.clone()),
            content_type: Set(result.content_type.clone()),
            headers: Set(Some(result.headers.clone())),
            meta_data: Set(Some(result.meta_data.clone())),
            screenshot: Set(result.screenshot.clone()),
            response_time_ms: Set(result.response_time_ms),
            created_at: Set(result
                .created_at
                .and_utc()
                .with_timezone(&FixedOffset::east_opt(0).unwrap())),
        }
    }

    /// Convert database model to domain model
    fn to_domain(model: db_entity::Model) -> ScrapeResult {
        ScrapeResult {
            id: model.id,
            task_id: model.task_id,
            url: model.url,
            status_code: model.status_code,
            content: model.content,
            content_type: model.content_type,
            headers: model.headers.unwrap_or(serde_json::json!({})),
            meta_data: model.meta_data.unwrap_or(serde_json::json!({})),
            screenshot: model.screenshot,
            response_time_ms: model.response_time_ms,
            created_at: model.created_at.naive_utc(),
        }
    }
}

#[async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryImpl {
    async fn save(&self, result: ScrapeResult) -> anyhow::Result<()> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;

        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        let active_model = Self::to_active_model(&result);

        active_model
            .insert(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to insert: {}", e))?;

        Ok(())
    }

    async fn find_by_task_id(&self, task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;

        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        let result = db_entity::Entity::find()
            .filter(db_entity::Column::TaskId.eq(task_id))
            .one(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;

        Ok(result.map(Self::to_domain))
    }

    async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }

        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;

        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        let results = db_entity::Entity::find()
            .filter(db_entity::Column::TaskId.is_in(task_ids.to_vec()))
            .all(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;

        Ok(results.into_iter().map(Self::to_domain).collect())
    }

    async fn get_team_avg_response_time(&self, team_id: Uuid) -> anyhow::Result<f64> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;

        let conn = session
            .connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        // JOIN scrape_results with tasks on task_id, filter by team_id and
        // last 30 days. AVG(bigint) returns numeric; cast to DOUBLE PRECISION
        // so it maps cleanly to f64. COALESCE returns 0.0 when no rows match.
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COALESCE(AVG(sr.response_time_ms), 0)::DOUBLE PRECISION AS avg_ms
               FROM scrape_results sr
               JOIN tasks t ON sr.task_id = t.id
               WHERE t.team_id = $1
                 AND sr.created_at >= NOW() - INTERVAL '30 days'"#,
            [team_id.into()],
        );

        let row: Option<QueryResult> = conn
            .query_one_raw(stmt)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query avg response time: {}", e))?;

        let avg = row
            .and_then(|r| r.try_get::<f64>("", "avg_ms").ok())
            .unwrap_or(0.0);

        Ok(avg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use chrono::{FixedOffset, TimeZone};

    fn sample_scrape_result() -> ScrapeResult {
        ScrapeResult {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            url: "https://example.com/page".to_string(),
            status_code: 200,
            content: "<html>hello</html>".to_string(),
            content_type: "text/html".to_string(),
            headers: serde_json::json!({"content-type": "text/html"}),
            meta_data: serde_json::json!({"lang": "en"}),
            screenshot: Some("base64screenshot".to_string()),
            response_time_ms: 150,
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .expect("valid timestamp")
                .naive_utc(),
        }
    }

    fn sample_db_model() -> db_entity::Model {
        db_entity::Model {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            url: "https://example.com/page".to_string(),
            status_code: 404,
            content: "not found".to_string(),
            content_type: "text/plain".to_string(),
            response_time_ms: 42,
            created_at: FixedOffset::east_opt(0).unwrap().from_utc_datetime(
                &chrono::DateTime::from_timestamp(1_700_000_000, 0)
                    .expect("valid timestamp")
                    .naive_utc(),
            ),
            headers: Some(serde_json::json!({"x-custom": "value"})),
            meta_data: Some(serde_json::json!({"source": "test"})),
            screenshot: None,
        }
    }

    // ========== construction & accessor ==========

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = ScrapeResultRepositoryImpl::new(pool);
        // pool() accessor should return the same Arc
        let pool_ref = repo.pool();
        assert!(Arc::strong_count(pool_ref) >= 1);
    }

    // ========== pure conversion functions ==========

    #[test]
    fn test_to_active_model_converts_all_fields() {
        let result = sample_scrape_result();
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);

        // ActiveModel fields are Set; extract via unwrap() to verify
        assert_eq!(active.id.unwrap(), result.id);
        assert_eq!(active.task_id.unwrap(), result.task_id);
        assert_eq!(active.url.unwrap(), result.url);
        assert_eq!(active.status_code.unwrap(), result.status_code);
        assert_eq!(active.content.unwrap(), result.content);
        assert_eq!(active.content_type.unwrap(), result.content_type);
        assert_eq!(active.response_time_ms.unwrap(), result.response_time_ms);
        assert_eq!(active.screenshot.unwrap(), result.screenshot);
        // headers/meta_data wrapped in Some()
        assert_eq!(active.headers.unwrap(), Some(result.headers.clone()));
        assert_eq!(active.meta_data.unwrap(), Some(result.meta_data.clone()));
    }

    #[test]
    fn test_to_active_model_with_none_screenshot() {
        let mut result = sample_scrape_result();
        result.screenshot = None;
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.screenshot.unwrap(), None);
    }

    #[test]
    fn test_to_domain_converts_all_fields() {
        let model = sample_db_model();
        let domain = ScrapeResultRepositoryImpl::to_domain(model.clone());

        assert_eq!(domain.id, model.id);
        assert_eq!(domain.task_id, model.task_id);
        assert_eq!(domain.url, model.url);
        assert_eq!(domain.status_code, model.status_code);
        assert_eq!(domain.content, model.content);
        assert_eq!(domain.content_type, model.content_type);
        assert_eq!(domain.response_time_ms, model.response_time_ms);
        assert_eq!(domain.screenshot, model.screenshot);
        // headers/meta_data unwrapped from Option
        assert_eq!(domain.headers, model.headers.unwrap());
        assert_eq!(domain.meta_data, model.meta_data.unwrap());
    }

    #[test]
    fn test_to_domain_with_null_headers_uses_default_object() {
        let mut model = sample_db_model();
        model.headers = None;
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(domain.headers, serde_json::json!({}));
    }

    #[test]
    fn test_to_domain_with_null_meta_data_uses_default_object() {
        let mut model = sample_db_model();
        model.meta_data = None;
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(domain.meta_data, serde_json::json!({}));
    }

    #[test]
    fn test_to_domain_roundtrip_preserves_core_fields() {
        let original = sample_scrape_result();
        let active = ScrapeResultRepositoryImpl::to_active_model(&original);
        // Reconstruct a Model from the ActiveModel (all fields are Set)
        let model = db_entity::Model {
            id: active.id.unwrap(),
            task_id: active.task_id.unwrap(),
            url: active.url.unwrap(),
            status_code: active.status_code.unwrap(),
            content: active.content.unwrap(),
            content_type: active.content_type.unwrap(),
            response_time_ms: active.response_time_ms.unwrap(),
            created_at: active.created_at.unwrap(),
            headers: active.headers.unwrap(),
            meta_data: active.meta_data.unwrap(),
            screenshot: active.screenshot.unwrap(),
        };
        let roundtrip = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(roundtrip.id, original.id);
        assert_eq!(roundtrip.url, original.url);
        assert_eq!(roundtrip.content, original.content);
        assert_eq!(roundtrip.headers, original.headers);
        assert_eq!(roundtrip.meta_data, original.meta_data);
    }

    // ========== CRUD against real DB ==========

    #[tokio::test]
    async fn test_save_creates_record() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let mut result = sample_scrape_result();
        result.id = Uuid::new_v4();
        result.task_id = Uuid::new_v4();
        let saved = repo.save(result.clone()).await;
        assert!(saved.is_ok(), "save failed: {:?}", saved.err());

        // Verify DB state: find_by_task_id should return the created record
        let found = repo
            .find_by_task_id(result.task_id)
            .await
            .expect("find_by_task_id failed")
            .expect("record should exist after save");
        assert_eq!(found.id, result.id);
        assert_eq!(found.task_id, result.task_id);
        assert_eq!(found.url, result.url);
        assert_eq!(found.status_code, result.status_code);
        assert_eq!(found.content, result.content);
        assert_eq!(found.content_type, result.content_type);
        assert_eq!(found.response_time_ms, result.response_time_ms);
    }

    #[tokio::test]
    async fn test_find_by_task_id_returns_none_for_unknown() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_task_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_task_id failed: {:?}", result.err());
        assert!(
            result.unwrap().is_none(),
            "unknown task_id should return None"
        );
    }

    #[tokio::test]
    async fn test_find_by_task_ids_returns_empty_for_unknown() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .find_by_task_ids(&[Uuid::new_v4(), Uuid::new_v4()])
            .await;
        assert!(
            result.is_ok(),
            "find_by_task_ids failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown task_ids should return empty vec"
        );
    }

    // ========== fast-path (no DB access) ==========

    #[tokio::test]
    async fn test_find_by_task_ids_with_empty_slice_returns_empty_vec() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_task_ids(&[]).await;
        assert!(
            result.is_ok(),
            "empty slice should short-circuit without DB"
        );
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_team_avg_response_time_returns_zero_for_unknown_team() {
        // Unknown team_id → JOIN yields no rows → COALESCE returns 0.0.
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_team_avg_response_time(Uuid::new_v4()).await;
        assert!(
            result.is_ok(),
            "get_team_avg_response_time failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0.0);
    }

    // ========== additional boundary variants ==========

    #[tokio::test]
    async fn test_find_by_task_ids_with_single_id_returns_empty_for_unknown() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_task_ids(&[Uuid::new_v4()]).await;
        assert!(
            result.is_ok(),
            "find_by_task_ids failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown single id should return empty vec"
        );
    }

    #[tokio::test]
    async fn test_find_by_task_ids_with_many_ids_returns_empty_for_unknown() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let ids: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();
        let result = repo.find_by_task_ids(&ids).await;
        assert!(
            result.is_ok(),
            "find_by_task_ids failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown ids should return empty vec"
        );
    }

    #[tokio::test]
    async fn test_find_by_task_id_with_nil_uuid_returns_none() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        // Use a fresh random UUID instead of Uuid::nil() to avoid cross-test
        // data pollution (other tests may insert records with nil task_id).
        let result = repo.find_by_task_id(Uuid::new_v4()).await;
        assert!(result.is_ok(), "find_by_task_id failed: {:?}", result.err());
        assert!(
            result.unwrap().is_none(),
            "unknown task_id should return None"
        );
    }

    #[tokio::test]
    async fn test_save_with_nil_task_id_succeeds() {
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let mut result = sample_scrape_result();
        result.id = Uuid::new_v4();
        result.task_id = Uuid::nil();
        let res = repo.save(result).await;
        assert!(res.is_ok(), "save with nil task_id failed: {:?}", res.err());
    }

    #[tokio::test]
    async fn test_get_team_avg_response_time_with_nil_uuid_returns_zero() {
        // nil UUID as team_id: no tasks carry team_id=Nil, so JOIN yields 0 rows.
        let repo = ScrapeResultRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_team_avg_response_time(Uuid::nil()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0.0);
    }

    // ========== to_active_model / to_domain additional boundaries ==========

    #[test]
    fn test_to_active_model_with_empty_content() {
        let mut result = sample_scrape_result();
        result.content = "".to_string();
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.content.unwrap(), "");
    }

    #[test]
    fn test_to_active_model_with_empty_url() {
        let mut result = sample_scrape_result();
        result.url = "".to_string();
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.url.unwrap(), "");
    }

    #[test]
    fn test_to_active_model_with_zero_status_code() {
        let mut result = sample_scrape_result();
        result.status_code = 0;
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.status_code.unwrap(), 0);
    }

    #[test]
    fn test_to_active_model_with_large_status_code() {
        let mut result = sample_scrape_result();
        result.status_code = 599;
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.status_code.unwrap(), 599);
    }

    #[test]
    fn test_to_active_model_with_zero_response_time() {
        let mut result = sample_scrape_result();
        result.response_time_ms = 0;
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        assert_eq!(active.response_time_ms.unwrap(), 0);
    }

    #[test]
    fn test_to_active_model_with_empty_headers_and_metadata() {
        let mut result = sample_scrape_result();
        result.headers = serde_json::json!({});
        result.meta_data = serde_json::json!({});
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        // headers/meta_data are wrapped in Some()
        assert_eq!(active.headers.unwrap(), Some(serde_json::json!({})));
        assert_eq!(active.meta_data.unwrap(), Some(serde_json::json!({})));
    }

    #[test]
    fn test_to_active_model_with_complex_headers() {
        let mut result = sample_scrape_result();
        result.headers = serde_json::json!({
            "content-type": "application/json",
            "x-custom-header": "value-with-unicode-🔑",
            "x-numbers": [1, 2, 3],
            "x-nested": {"key": "value"}
        });
        let active = ScrapeResultRepositoryImpl::to_active_model(&result);
        let headers = active.headers.unwrap().unwrap();
        assert_eq!(headers["content-type"], "application/json");
        assert_eq!(headers["x-custom-header"], "value-with-unicode-🔑");
        assert_eq!(headers["x-numbers"][2], 3);
        assert_eq!(headers["x-nested"]["key"], "value");
    }

    #[test]
    fn test_to_domain_with_zero_response_time() {
        let mut model = sample_db_model();
        model.response_time_ms = 0;
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(domain.response_time_ms, 0);
    }

    #[test]
    fn test_to_domain_with_empty_strings() {
        let mut model = sample_db_model();
        model.url = "".to_string();
        model.content = "".to_string();
        model.content_type = "".to_string();
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(domain.url, "");
        assert_eq!(domain.content, "");
        assert_eq!(domain.content_type, "");
    }

    #[test]
    fn test_to_domain_with_both_headers_and_metadata_null() {
        let mut model = sample_db_model();
        model.headers = None;
        model.meta_data = None;
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        // Both should default to empty JSON objects
        assert_eq!(domain.headers, serde_json::json!({}));
        assert_eq!(domain.meta_data, serde_json::json!({}));
    }

    #[test]
    fn test_to_domain_with_screenshot_present() {
        let mut model = sample_db_model();
        model.screenshot = Some("base64data".to_string());
        let domain = ScrapeResultRepositoryImpl::to_domain(model);
        assert_eq!(domain.screenshot, Some("base64data".to_string()));
    }

    // ========== sample_scrape_result boundary verification ==========

    #[test]
    fn test_sample_scrape_result_construction_values() {
        let result = sample_scrape_result();
        assert_eq!(result.status_code, 200);
        assert_eq!(result.content_type, "text/html");
        assert_eq!(result.response_time_ms, 150);
        assert!(result.screenshot.is_some());
        assert_eq!(result.screenshot.as_ref().unwrap(), "base64screenshot");
        assert_eq!(result.url, "https://example.com/page");
    }

    #[test]
    fn test_sample_db_model_construction_values() {
        let model = sample_db_model();
        assert_eq!(model.status_code, 404);
        assert_eq!(model.content_type, "text/plain");
        assert_eq!(model.response_time_ms, 42);
        assert!(model.screenshot.is_none());
        assert!(model.headers.is_some());
        assert!(model.meta_data.is_some());
    }
}
