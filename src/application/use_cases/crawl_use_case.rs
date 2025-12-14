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

use crate::{
    application::dto::crawl_request::CrawlRequestDto,
    domain::{
        models::{
            crawl::{Crawl, CrawlStatus},
            scrape_result::ScrapeResult,
            task::{Task, TaskStatus, TaskType},
        },
        repositories::{
            crawl_repository::CrawlRepository,
            scrape_result_repository::ScrapeResultRepository,
            task_repository::{RepositoryError, TaskRepository},
            webhook_repository::WebhookRepository,
        },
    },
};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

#[derive(Error, Debug)]
pub enum CrawlUseCaseError {
    #[error("Validation failed: {0}")]
    ValidationError(String),
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Crawl not found")]
    NotFound,
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub struct CrawlUseCase<CR, TR, WR, SRR> {
    crawl_repo: Arc<CR>,
    task_repo: Arc<TR>,
    webhook_repo: Arc<WR>,
    scrape_result_repo: Arc<SRR>,
}

impl<CR, TR, WR, SRR> CrawlUseCase<CR, TR, WR, SRR>
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
{
    pub fn new(
        crawl_repo: Arc<CR>,
        task_repo: Arc<TR>,
        webhook_repo: Arc<WR>,
        scrape_result_repo: Arc<SRR>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            webhook_repo,
            scrape_result_repo,
        }
    }

    // Allow unused webhook_repo until it's fully integrated
    #[allow(dead_code)]
    pub fn webhook_repo(&self) -> &Arc<WR> {
        &self.webhook_repo
    }

    pub async fn get_crawl_results(
        &self,
        crawl_id: Uuid,
        _team_id: Uuid,
    ) -> Result<Vec<ScrapeResult>, CrawlUseCaseError> {
        // 1. Check if crawl exists
        if self.crawl_repo.find_by_id(crawl_id).await?.is_none() {
            return Err(CrawlUseCaseError::NotFound);
        }

        // 2. Get all tasks for this crawl
        let tasks = self.task_repo.find_by_crawl_id(crawl_id).await?;

        // 3. Get results for each task
        let mut results = Vec::new();
        for task in tasks {
            if let Some(result) = self.scrape_result_repo.find_by_task_id(task.id).await? {
                results.push(result);
            }
        }

        Ok(results)
    }

    pub async fn create_crawl(
        &self,
        team_id: Uuid,
        dto: CrawlRequestDto,
    ) -> Result<Crawl, CrawlUseCaseError> {
        dto.validate()
            .map_err(|e| CrawlUseCaseError::ValidationError(e.to_string()))?;

        let crawl_id = Uuid::new_v4();
        let now = Utc::now();

        let crawl = Crawl {
            id: crawl_id,
            team_id,
            name: dto.name.unwrap_or_else(|| "Untitled Crawl".to_string()),
            root_url: dto.url.clone(),
            url: dto.url.clone(),
            status: CrawlStatus::Queued,
            config: json!(dto.config),
            total_tasks: 1,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        self.crawl_repo.create(&crawl).await?;

        let initial_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 100, // Default priority
            team_id,
            url: dto.url,
            payload: json!({ "crawl_id": crawl_id, "depth": 0, "config": dto.config }),
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: now.into(),
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl_id),
            updated_at: now.into(),
            lock_token: None,
            lock_expires_at: None,
        };

        self.task_repo.create(&initial_task).await?;

        Ok(crawl)
    }

    pub async fn get_crawl(&self, crawl_id: Uuid) -> Result<Option<Crawl>, CrawlUseCaseError> {
        self.crawl_repo
            .find_by_id(crawl_id)
            .await
            .map_err(Into::into)
    }

    pub async fn cancel_crawl(&self, id: Uuid, team_id: Uuid) -> Result<(), CrawlUseCaseError> {
        let crawl = self.crawl_repo.find_by_id(id).await?;

        match crawl {
            Some(mut c) => {
                // Ensure the crawl belongs to the team
                if c.team_id != team_id {
                    return Err(CrawlUseCaseError::NotFound); // Or Forbidden
                }

                if c.status == CrawlStatus::Completed
                    || c.status == CrawlStatus::Failed
                    || c.status == CrawlStatus::Cancelled
                {
                    return Ok(()); // Already finished
                }

                c.status = CrawlStatus::Cancelled;
                c.updated_at = Utc::now();
                self.crawl_repo.update(&c).await?;

                // Cancel all associated tasks
                self.task_repo.cancel_tasks_by_crawl_id(id).await?;

                Ok(())
            }
            None => Err(CrawlUseCaseError::NotFound),
        }
    }
}
