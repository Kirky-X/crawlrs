// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::presentation::middleware::team_semaphore::TeamSemaphore;

pub async fn team_semaphore_middleware(
    State(semaphore): State<Arc<TeamSemaphore>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Temporarily hardcode team_id to nil for testing
    let team_id = uuid::Uuid::nil();
    let _permit = semaphore.acquire(team_id).await;
    Ok(next.run(request).await)
}
