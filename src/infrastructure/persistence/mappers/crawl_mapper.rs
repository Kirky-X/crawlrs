// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl Mapper - converts between Crawl domain model and database entity

use crate::domain::models::{Crawl, CrawlStatus};
use crate::infrastructure::database::entities::crawl;

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
}
