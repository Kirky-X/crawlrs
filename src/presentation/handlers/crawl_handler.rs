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

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::{
        dto::crawl_request::CrawlRequestDto,
        use_cases::crawl_use_case::{CrawlUseCase, CrawlUseCaseError},
    },
    domain::repositories::{
        crawl_repository::CrawlRepository, scrape_result_repository::ScrapeResultRepository,
        task_repository::TaskRepository, webhook_repository::WebhookRepository,
    },
    presentation::middleware::auth_middleware::AuthState,
};

/// 创建新的爬取任务
pub async fn create_crawl<CR, TR, WR, SRR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(team_id): Extension<Uuid>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
{
    let use_case = CrawlUseCase::new(crawl_repo, task_repo, webhook_repo, scrape_result_repo);
    match use_case.create_crawl(team_id, payload).await {
        Ok(crawl) => (StatusCode::CREATED, Json(crawl)).into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 获取爬取任务详情
pub async fn get_crawl<CR, TR, WR, SRR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
{
    let use_case = CrawlUseCase::new(crawl_repo, task_repo, webhook_repo, scrape_result_repo);
    match use_case.get_crawl(crawl_id).await {
        Ok(Some(crawl)) => (StatusCode::OK, Json(crawl)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 获取爬取任务结果
pub async fn get_crawl_results<CR, TR, WR, SRR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(user): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
{
    let use_case = CrawlUseCase::new(crawl_repo, task_repo, webhook_repo, scrape_result_repo);
    let team_id = user.team_id;

    match use_case.get_crawl_results(crawl_id, team_id).await {
        Ok(results) => (StatusCode::OK, Json(results)).into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 取消进行中的爬取任务
pub async fn cancel_crawl<CR, TR, WR, SRR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(user): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
{
    let use_case = CrawlUseCase::new(crawl_repo, task_repo, webhook_repo, scrape_result_repo);
    // TODO: 获取当前用户的 Team ID
    // 假设 user.team_id 存在
    let team_id = user.team_id;

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

impl From<CrawlUseCaseError> for (StatusCode, String) {
    fn from(err: CrawlUseCaseError) -> Self {
        match err {
            CrawlUseCaseError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg),
            CrawlUseCaseError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            CrawlUseCaseError::NotFound => (StatusCode::NOT_FOUND, "Crawl not found".to_string()),
            CrawlUseCaseError::Anyhow(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}
