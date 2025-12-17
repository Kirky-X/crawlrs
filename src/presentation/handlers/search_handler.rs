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
    config::settings::Settings,
    domain::{
        repositories::{crawl_repository::CrawlRepository, task_repository::TaskRepository},
        services::search_service::{SearchService, SearchServiceError},
    },
};

/// 处理搜索请求
///
/// # 参数
///
/// * `crawl_repo` - 爬取任务仓库实例
/// * `task_repo` - 任务仓库实例  
/// * `team_id` - 团队ID
/// * `payload` - 搜索请求数据
///
/// # 返回值
///
/// 返回实现了 `IntoResponse` 的响应，包含搜索结果或错误信息
///
/// # 错误
///
/// 可能在以下情况下返回错误响应：
/// - 搜索参数验证失败
/// - 仓库操作失败
/// - 搜索引擎错误
pub async fn search<CR, TR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(settings): Extension<Arc<Settings>>,
    Json(payload): Json<SearchRequestDto>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
{
    let team_id = Uuid::nil();
    let service = SearchService::new(crawl_repo, task_repo, settings);
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
