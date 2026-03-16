// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{ConnectInfo, Extension},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;
use tracing::error;

use crate::application::dto::extract_request::ExtractRequestDto;
use crate::common::constants::crawl_task;
use crate::config::settings::Settings;
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::team_service::TeamService;
use crate::presentation::handlers::response_builder::{error_response, ApiResponse};
use crate::presentation::handlers::task_handler::wait_for_tasks_completion;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;
use std::sync::Arc;
use uuid::Uuid;

/// 提取任务响应数据传输对象
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractResponseDto {
    /// 任务ID
    pub id: Uuid,
    /// 任务状态
    pub status: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn extract<GR>(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_service): Extension<Arc<TeamService>>,
    Extension(auth_state): Extension<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<ExtractRequestDto>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    let team_id = auth_state.team_id;
    // Validate the request
    if payload.urls.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "At least one URL is required");
    }

    if payload.prompt.is_none() && payload.schema.is_none() && payload.rules.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Either prompt, schema, or rules is required",
        );
    }

    // 检查地理限制
    let client_ip = addr.ip().to_string();

    // 获取团队地理限制配置
    let restrictions = match geo_restriction_repo.get_team_restrictions(team_id).await {
        Ok(restrictions) => restrictions,
        Err(e) => {
            error!("Failed to get team restrictions: {:?}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to validate geographic access",
            );
        }
    };

    // Validate sync_wait_ms if present
    if let Some(ms) = payload.sync_wait_ms {
        if ms > crawl_task::MAX_SYNC_WAIT_MS {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("sync_wait_ms must be <= {}", crawl_task::MAX_SYNC_WAIT_MS),
            );
        }
    }

    // 使用团队服务验证地理限制
    match team_service
        .validate_geographic_restriction(team_id, &client_ip, &restrictions)
        .await
    {
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed) => {
            // 记录允许的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "ALLOWED",
                    "Extract request - Geographic restriction check passed",
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }
        }
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
            // 记录拒绝的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "DENIED",
                    &format!("Extract request - {}", reason),
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }

            return error_response(
                StatusCode::FORBIDDEN,
                format!("Access denied due to geographic restrictions: {}", reason),
            );
        }
        Err(e) => {
            error!("Failed to validate geographic restrictions: {:?}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to validate geographic access",
            );
        }
    }

    // Create a task for async extraction
    let primary_url = payload
        .urls
        .first()
        .expect("URLs already validated as non-empty");
    let now = chrono::Utc::now().naive_utc();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Extract.to_string(),
        status: TaskStatus::Queued.to_string(),
        priority: 0,
        team_id,
        api_key_id: auth_state.api_key_id,
        url: primary_url.clone(),
        payload: serde_json::to_value(&payload).unwrap_or_default(),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    };

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 处理同步等待逻辑
            let sync_wait_ms = payload
                .sync_wait_ms
                .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);
            let mut waited_time_ms = 0u64;

            if sync_wait_ms > 0 {
                let wait_start = std::time::Instant::now();

                // 调用智能轮询等待函数
                match wait_for_tasks_completion(
                    task_repository.as_ref(),
                    &[task.id],
                    team_id,
                    sync_wait_ms,
                    crawl_task::BASE_POLL_INTERVAL_MS,
                )
                .await
                {
                    Ok(_) => {
                        waited_time_ms = wait_start.elapsed().as_millis() as u64;
                    }
                    Err(e) => {
                        error!("Failed to wait for task completion: {:?}", e);
                        // 即使等待失败，也返回已创建的任务信息
                    }
                }
            }

            let response = ExtractResponseDto {
                id: task.id,
                status: "pending".to_string(),
            };

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 && waited_time_ms >= sync_wait_ms as u64 {
                StatusCode::ACCEPTED // 同步等待超时，任务已接受但可能未完成
            } else {
                StatusCode::CREATED // 任务已创建（可能已完成）
            };

            (status_code, Json(ApiResponse::success(response))).into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
