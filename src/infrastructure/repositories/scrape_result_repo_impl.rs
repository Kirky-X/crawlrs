// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::scrape_result::ScrapeResult;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::infrastructure::database::entities::scrape_result as scrape_result_entity;
use async_trait::async_trait;
use sea_orm::*;
use std::sync::Arc;
use uuid::Uuid;

/// 抓取结果仓库实现
pub struct ScrapeResultRepositoryImpl {
    /// 数据库连接
    db: Arc<DatabaseConnection>,
}

impl ScrapeResultRepositoryImpl {
    /// 创建新的抓取结果仓库实例
    ///
    /// # 参数
    ///
    /// * `db` - 数据库连接
    ///
    /// # 返回值
    ///
    /// 返回新的抓取结果仓库实例
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryImpl {
    async fn save(&self, result: ScrapeResult) -> anyhow::Result<()> {
        let active_model = scrape_result_entity::ActiveModel {
            id: Set(result.id),
            task_id: Set(result.task_id),
            url: Set(result.url),
            status_code: Set(result.status_code as i32),
            content: Set(result.content),
            content_type: Set(result.content_type),
            response_time_ms: Set(result.response_time_ms as i64),
            created_at: Set(result.created_at.into()),
            headers: Set(Some(result.headers)),
            meta_data: Set(Some(result.meta_data)),
            screenshot: Set(result.screenshot),
        };

        scrape_result_entity::Entity::insert(active_model)
            .exec(self.db.as_ref())
            .await?;

        Ok(())
    }

    async fn find_by_task_id(&self, task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
        let model = scrape_result_entity::Entity::find()
            .filter(scrape_result_entity::Column::TaskId.eq(task_id))
            .one(self.db.as_ref())
            .await?;

        match model {
            Some(m) => Ok(Some(ScrapeResult {
                id: m.id,
                task_id: m.task_id,
                url: m.url,
                status_code: m.status_code as u16,
                content: m.content,
                content_type: m.content_type,
                response_time_ms: m.response_time_ms as u64,
                created_at: m.created_at.into(),
                headers: m.headers.unwrap_or_default(),
                meta_data: m.meta_data.unwrap_or_default(),
                screenshot: m.screenshot,
            })),
            None => Ok(None),
        }
    }

    async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }

        let models = scrape_result_entity::Entity::find()
            .filter(scrape_result_entity::Column::TaskId.is_in(task_ids.to_vec()))
            .all(self.db.as_ref())
            .await?;

        let results = models
            .into_iter()
            .map(|m| ScrapeResult {
                id: m.id,
                task_id: m.task_id,
                url: m.url,
                status_code: m.status_code as u16,
                content: m.content,
                content_type: m.content_type,
                response_time_ms: m.response_time_ms as u64,
                created_at: m.created_at.into(),
                headers: m.headers.unwrap_or_default(),
                meta_data: m.meta_data.unwrap_or_default(),
                screenshot: m.screenshot,
            })
            .collect();

        Ok(results)
    }
}
