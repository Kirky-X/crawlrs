// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl Mapper - converts between Crawl domain model and database entity

use crate::domain::models::{Crawl, CrawlStatus};
use crate::infrastructure::database::entities::crawl;
use sea_orm::ActiveValue::{Set, Unchanged};

/// Mapper for converting between Crawl domain model and database entity
pub struct CrawlMapper;

impl CrawlMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: crawl::Model) -> Crawl {
        Crawl::with_all_fields(
            entity.id,
            entity.team_id,
            entity.name,
            entity.root_url,
            entity.url,
            Self::parse_status(&entity.status),
            entity.config,
            entity.total_tasks,
            entity.completed_tasks,
            entity.failed_tasks,
            entity.created_at.and_utc(),
            entity.updated_at.and_utc(),
            entity.completed_at.map(|dt| dt.and_utc()),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &Crawl) -> crawl::Model {
        crawl::Model {
            id: domain.id,
            team_id: domain.team_id,
            name: domain.name.clone(),
            root_url: domain.root_url.clone(),
            url: domain.url.clone(),
            status: domain.status.to_string(),
            config: domain.config().clone(),
            total_tasks: domain.total_tasks(),
            completed_tasks: domain.completed_tasks(),
            failed_tasks: domain.failed_tasks(),
            created_at: domain.created_at.naive_utc(),
            updated_at: domain.updated_at.naive_utc(),
            completed_at: domain.completed_at.map(|dt| dt.naive_utc()),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<crawl::Model>) -> Vec<Crawl> {
        entities.into_iter().map(Self::to_domain).collect()
    }

    /// Convert domain model to ActiveModel for update operations
    ///
    /// 所有字段为 Set 状态，确保 `update()` 实际更新数据。
    /// id 用 Unchanged（用于 WHERE 条件），created_at 用 Unchanged（不应更新）。
    pub fn to_active_model(domain: &Crawl) -> crawl::ActiveModel {
        let entity = Self::to_entity(domain);
        crawl::ActiveModel {
            id: Unchanged(entity.id),
            team_id: Set(entity.team_id),
            name: Set(entity.name),
            root_url: Set(entity.root_url),
            url: Set(entity.url),
            status: Set(entity.status),
            config: Set(entity.config),
            total_tasks: Set(entity.total_tasks),
            completed_tasks: Set(entity.completed_tasks),
            failed_tasks: Set(entity.failed_tasks),
            created_at: Unchanged(entity.created_at),
            updated_at: Set(entity.updated_at),
            completed_at: Set(entity.completed_at),
        }
    }

    /// Parse status from string
    fn parse_status(s: &str) -> CrawlStatus {
        s.parse().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_crawl_mapper_roundtrip() {
        let now = Utc::now();
        let domain = Crawl::with_all_fields(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Test Crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com/page1".to_string(),
            CrawlStatus::Processing,
            serde_json::json!({"depth": 2}),
            10,
            5,
            1,
            now,
            now,
            None,
        );

        let entity = CrawlMapper::to_entity(&domain);
        let back_to_domain = CrawlMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.name, back_to_domain.name);
        assert_eq!(domain.status, back_to_domain.status);
        assert_eq!(domain.total_tasks(), back_to_domain.total_tasks());
    }

    #[test]
    fn test_crawl_mapper_to_domain_list() {
        let now_naive = Utc::now().naive_utc();
        let entities = vec![
            crawl::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                name: "Crawl A".to_string(),
                root_url: "https://a.com".to_string(),
                url: "https://a.com/page".to_string(),
                status: "processing".to_string(),
                config: serde_json::json!({"depth": 1}),
                total_tasks: 5,
                completed_tasks: 2,
                failed_tasks: 0,
                created_at: now_naive,
                updated_at: now_naive,
                completed_at: None,
            },
            crawl::Model {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                name: "Crawl B".to_string(),
                root_url: "https://b.com".to_string(),
                url: "https://b.com/page".to_string(),
                status: "completed".to_string(),
                config: serde_json::json!({"depth": 3}),
                total_tasks: 10,
                completed_tasks: 9,
                failed_tasks: 1,
                created_at: now_naive,
                updated_at: now_naive,
                completed_at: Some(now_naive),
            },
        ];

        let domains = CrawlMapper::to_domain_list(entities);
        assert_eq!(domains.len(), 2);
        assert_eq!(domains[0].name, "Crawl A");
        assert_eq!(domains[0].status, CrawlStatus::Processing);
        assert_eq!(domains[1].name, "Crawl B");
        assert_eq!(domains[1].status, CrawlStatus::Completed);
        assert!(domains[1].completed_at.is_some());
    }

    #[test]
    fn test_crawl_mapper_to_domain_list_empty() {
        let domains = CrawlMapper::to_domain_list(vec![]);
        assert!(domains.is_empty());
    }

    #[test]
    fn test_crawl_mapper_invalid_status_falls_back_to_default() {
        let now_naive = Utc::now().naive_utc();
        let entity = crawl::Model {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            name: "Bad Status".to_string(),
            root_url: "https://example.com".to_string(),
            url: "https://example.com/page".to_string(),
            status: "invalid_status".to_string(),
            config: serde_json::json!({}),
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: now_naive,
            updated_at: now_naive,
            completed_at: None,
        };

        let domain = CrawlMapper::to_domain(entity);
        // Default CrawlStatus is Queued
        assert_eq!(domain.status, CrawlStatus::Queued);
    }

    #[test]
    fn test_crawl_mapper_all_status_variants_roundtrip() {
        let now = Utc::now();
        let statuses = vec![
            CrawlStatus::Queued,
            CrawlStatus::Processing,
            CrawlStatus::Completed,
            CrawlStatus::Failed,
            CrawlStatus::Cancelled,
        ];

        for status in statuses {
            let domain = Crawl::with_all_fields(
                Uuid::new_v4(),
                Uuid::new_v4(),
                "Test".to_string(),
                "https://example.com".to_string(),
                "https://example.com/page".to_string(),
                status,
                serde_json::json!({}),
                10,
                5,
                1,
                now,
                now,
                None,
            );

            let entity = CrawlMapper::to_entity(&domain);
            let back_to_domain = CrawlMapper::to_domain(entity);
            assert_eq!(back_to_domain.status, status);
        }
    }

    #[test]
    fn test_crawl_mapper_with_completed_at() {
        let now = Utc::now();
        let domain = Crawl::with_all_fields(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Completed Crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com/page".to_string(),
            CrawlStatus::Completed,
            serde_json::json!({"depth": 2}),
            10,
            10,
            0,
            now,
            now,
            Some(now),
        );

        let entity = CrawlMapper::to_entity(&domain);
        assert!(entity.completed_at.is_some());

        let back_to_domain = CrawlMapper::to_domain(entity);
        assert!(back_to_domain.completed_at.is_some());
    }

    #[test]
    fn test_crawl_mapper_preserves_all_fields() {
        let now = Utc::now();
        let domain = Crawl::with_all_fields(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "Full Crawl".to_string(),
            "https://root.example.com".to_string(),
            "https://example.com/start".to_string(),
            CrawlStatus::Failed,
            serde_json::json!({"depth": 5, "max_pages": 100}),
            100,
            80,
            20,
            now,
            now,
            None,
        );

        let entity = CrawlMapper::to_entity(&domain);
        assert_eq!(entity.name, "Full Crawl");
        assert_eq!(entity.root_url, "https://root.example.com");
        assert_eq!(entity.url, "https://example.com/start");
        assert_eq!(entity.status, "failed");
        assert_eq!(entity.total_tasks, 100);
        assert_eq!(entity.completed_tasks, 80);
        assert_eq!(entity.failed_tasks, 20);

        let back_to_domain = CrawlMapper::to_domain(entity);
        assert_eq!(back_to_domain.root_url, "https://root.example.com");
        assert_eq!(back_to_domain.url, "https://example.com/start");
        assert_eq!(back_to_domain.total_tasks(), 100);
        assert_eq!(back_to_domain.completed_tasks(), 80);
        assert_eq!(back_to_domain.failed_tasks(), 20);
    }

    #[test]
    fn test_crawl_mapper_cancelled_status() {
        let now_naive = Utc::now().naive_utc();
        let entity = crawl::Model {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            name: "Cancelled".to_string(),
            root_url: "https://example.com".to_string(),
            url: "https://example.com/page".to_string(),
            status: "cancelled".to_string(),
            config: serde_json::json!({}),
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: now_naive,
            updated_at: now_naive,
            completed_at: None,
        };

        let domain = CrawlMapper::to_domain(entity);
        assert_eq!(domain.status, CrawlStatus::Cancelled);
    }
}
