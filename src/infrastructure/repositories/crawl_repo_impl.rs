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

use crate::domain::models::crawl::{Crawl, CrawlStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::crawl as crawl_entity;
use async_trait::async_trait;
use sea_orm::{sea_query::Expr, *};
use std::sync::Arc;
use uuid::Uuid;

/// 爬取仓库实现
pub struct CrawlRepositoryImpl {
    /// 数据库连接
    db: Arc<DatabaseConnection>,
}

impl CrawlRepositoryImpl {
    /// 创建新的爬取仓库实例
    ///
    /// # 参数
    ///
    /// * `db` - 数据库连接
    ///
    /// # 返回值
    ///
    /// 返回新的爬取仓库实例
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CrawlRepository for CrawlRepositoryImpl {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let model = crawl_entity::ActiveModel {
            id: Set(crawl.id),
            team_id: Set(crawl.team_id),
            name: Set(crawl.name.clone()),
            root_url: Set(crawl.root_url.clone()),
            url: Set(crawl.url.clone()),
            status: Set(crawl.status.to_string()),
            config: Set(crawl.config.clone()),
            total_tasks: Set(crawl.total_tasks),
            completed_tasks: Set(crawl.completed_tasks),
            failed_tasks: Set(crawl.failed_tasks),
            created_at: Set(crawl.created_at.into()),
            updated_at: Set(crawl.updated_at.into()),
            completed_at: Set(crawl.completed_at.map(Into::into)),
        };

        model.insert(self.db.as_ref()).await?;
        Ok(crawl.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
        let model = crawl_entity::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;

        match model {
            Some(m) => {
                let status = match m.status.as_str() {
                    "queued" => CrawlStatus::Queued,
                    "processing" => CrawlStatus::Processing,
                    "completed" => CrawlStatus::Completed,
                    "failed" => CrawlStatus::Failed,
                    "cancelled" => CrawlStatus::Cancelled,
                    _ => {
                        return Err(RepositoryError::Database(DbErr::Custom(
                            "Invalid crawl status".to_string(),
                        )))
                    }
                };

                Ok(Some(Crawl {
                    id: m.id,
                    team_id: m.team_id,
                    name: m.name,
                    root_url: m.root_url,
                    url: m.url,
                    status,
                    config: m.config,
                    total_tasks: m.total_tasks,
                    completed_tasks: m.completed_tasks,
                    failed_tasks: m.failed_tasks,
                    created_at: m.created_at.into(),
                    updated_at: m.updated_at.into(),
                    completed_at: m.completed_at.map(Into::into),
                }))
            }
            None => Ok(None),
        }
    }

    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let mut model: crawl_entity::ActiveModel = crawl_entity::Entity::find_by_id(crawl.id)
            .one(self.db.as_ref())
            .await?
            .ok_or(RepositoryError::NotFound)?
            .into();

        model.status = Set(crawl.status.to_string());
        model.total_tasks = Set(crawl.total_tasks);
        model.completed_tasks = Set(crawl.completed_tasks);
        model.failed_tasks = Set(crawl.failed_tasks);
        model.updated_at = Set(crawl.updated_at.into());
        model.completed_at = Set(crawl.completed_at.map(Into::into));

        model.update(self.db.as_ref()).await?;
        Ok(crawl.clone())
    }

    async fn update_status(&self, id: Uuid, status: CrawlStatus) -> Result<(), RepositoryError> {
        let model = crawl_entity::ActiveModel {
            id: Set(id),
            status: Set(status.to_string()),
            updated_at: Set(chrono::Utc::now().into()),
            ..Default::default()
        };

        model.update(self.db.as_ref()).await?;
        Ok(())
    }

    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        crawl_entity::Entity::update_many()
            .col_expr(
                crawl_entity::Column::TotalTasks,
                Expr::col(crawl_entity::Column::TotalTasks).add(1),
            )
            .filter(crawl_entity::Column::Id.eq(id))
            .exec(self.db.as_ref())
            .await?;
        Ok(())
    }

    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        crawl_entity::Entity::update_many()
            .col_expr(
                crawl_entity::Column::CompletedTasks,
                Expr::col(crawl_entity::Column::CompletedTasks).add(1),
            )
            .filter(crawl_entity::Column::Id.eq(id))
            .exec(self.db.as_ref())
            .await?;
        Ok(())
    }

    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        crawl_entity::Entity::update_many()
            .col_expr(
                crawl_entity::Column::FailedTasks,
                Expr::col(crawl_entity::Column::FailedTasks).add(1),
            )
            .filter(crawl_entity::Column::Id.eq(id))
            .exec(self.db.as_ref())
            .await?;
        Ok(())
    }
}
