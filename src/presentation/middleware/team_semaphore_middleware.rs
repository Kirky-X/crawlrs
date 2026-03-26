// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Extension,
};

use crate::presentation::middleware::team_semaphore::TeamSemaphore;

/// 从请求扩展中提取team_id
/// 认证中间件会将team_id注入到请求扩展中
fn extract_team_id(request: &Request) -> Option<uuid::Uuid> {
    request.extensions().get::<uuid::Uuid>().copied()
}

pub async fn team_semaphore_middleware(
    Extension(semaphore): Extension<Arc<TeamSemaphore>>,
    request: Request,
    next: Next,
) -> Response {
    // 从请求扩展中获取team_id（由认证中间件注入）
    let team_id = match extract_team_id(&request) {
        Some(id) => id,
        None => {
            tracing::warn!("No team_id found in request extensions - authentication may have failed");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // 使用真实的team_id获取并发许可
    match semaphore.acquire(team_id).await {
        Ok(_permit) => next.run(request).await,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
