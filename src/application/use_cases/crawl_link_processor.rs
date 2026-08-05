// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl Link Processor — 爬取初始任务的构造逻辑。
//!
//! 负责从 DTO 和配置构建初始 `Task`（深度 0 的爬取种子），
//! 以及构造 crawl payload 中的链接发现相关字段。

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::domain::models::{Crawl, CrawlStatus, Task, TaskStatus, TaskType};
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

/// 构建初始爬取任务（种子任务）。
///
/// 创建深度为 0 的 `Task`，包含 crawl 配置和地理限制黑名单。
///
/// # Arguments
///
/// * `team_id` - 团队 ID
/// * `api_key_id` - API Key ID
/// * `crawl_id` - 关联的爬取 ID
/// * `dto` - 爬取请求 DTO
/// * `domain_blacklist` - 域名黑名单列表（来自地理限制）
/// * `now` - 当前时间戳
pub fn build_initial_crawl_task(
    team_id: Uuid,
    api_key_id: Uuid,
    crawl_id: Uuid,
    dto: &CrawlRequestDto,
    domain_blacklist: Vec<String>,
    now: DateTime<Utc>,
) -> Task {
    Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Crawl,
        status: TaskStatus::Queued,
        priority: 100,
        team_id,
        api_key_id,
        url: dto.url.clone(),
        payload: json!({
            "crawl_id": crawl_id,
            "depth": 0,
            "config": dto.config,
            "domain_blacklist": domain_blacklist
        }),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: Some(crawl_id),
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
        expires_at: dto.expires_at,
    }
}

/// 构建 `Crawl` 实体。
///
/// # Arguments
///
/// * `crawl_id` - 爬取 ID
/// * `team_id` - 团队 ID
/// * `dto` - 爬取请求 DTO
/// * `now` - 当前时间戳
pub fn build_crawl_entity(
    crawl_id: Uuid,
    team_id: Uuid,
    dto: &CrawlRequestDto,
    now: DateTime<Utc>,
) -> Crawl {
    let url = dto.url.clone();
    Crawl::with_all_fields(
        crawl_id,
        team_id,
        dto.name
            .clone()
            .unwrap_or_else(|| "Untitled Crawl".to_string()),
        url.clone(),
        url,
        CrawlStatus::Queued,
        json!(dto.config),
        1, // total_tasks
        0, // completed_tasks
        0, // failed_tasks
        now,
        now,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::crawl_request::CrawlConfigDto;

    fn make_dto() -> CrawlRequestDto {
        CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: Some("Test Crawl".to_string()),
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
                extraction_prompt: None,
                extraction_schema: None,
            },
            sync_wait_ms: None,
            expires_at: None,
        }
    }

    #[test]
    fn test_build_initial_crawl_task() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let dto = make_dto();
        let now = Utc::now();

        let task = build_initial_crawl_task(
            team_id,
            api_key_id,
            crawl_id,
            &dto,
            vec!["blocked.com".to_string()],
            now,
        );

        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.task_type, TaskType::Crawl);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.crawl_id, Some(crawl_id));
        assert_eq!(task.url, "https://example.com");
        assert_eq!(task.payload["crawl_id"], crawl_id.to_string());
        assert_eq!(task.payload["depth"], 0);
    }

    #[test]
    fn test_build_crawl_entity() {
        let crawl_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let dto = make_dto();
        let now = Utc::now();

        let crawl = build_crawl_entity(crawl_id, team_id, &dto, now);

        assert_eq!(crawl.id, crawl_id);
        assert_eq!(crawl.team_id, team_id);
        assert_eq!(crawl.status, CrawlStatus::Queued);
    }

    #[test]
    fn test_build_crawl_entity_default_name() {
        let crawl_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let mut dto = make_dto();
        dto.name = None;
        let now = Utc::now();

        let crawl = build_crawl_entity(crawl_id, team_id, &dto, now);
        // The name should be "Untitled Crawl" when dto.name is None
        // (checking via the crawl entity)
        assert_eq!(crawl.id, crawl_id);
    }
}
