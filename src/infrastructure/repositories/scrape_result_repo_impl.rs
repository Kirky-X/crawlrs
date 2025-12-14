// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
                url: "".to_string(), // TODO: add url to scrape_result table
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
}
