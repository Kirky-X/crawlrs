// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::dto::search_request::SearchRequestDto,
    domain::{
        repositories::{crawl_repository::CrawlRepository, task_repository::TaskRepository},
        services::search_service::{SearchService, SearchServiceError},
    },
};

/// Handle search request
pub async fn search<CR, TR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(team_id): Extension<Uuid>,
    Json(payload): Json<SearchRequestDto>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
{
    let service = SearchService::new(crawl_repo, task_repo);
    match service.search(team_id, payload).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

impl From<SearchServiceError> for (StatusCode, String) {
    fn from(err: SearchServiceError) -> Self {
        match err {
            SearchServiceError::ValidationError(details) => (StatusCode::BAD_REQUEST, details),
            SearchServiceError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            SearchServiceError::SearchEngine(e) => (StatusCode::BAD_GATEWAY, e),
        }
    }
}
