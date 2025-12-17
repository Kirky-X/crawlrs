// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use axum::{
    extract::{Extension, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::presentation::middleware::team_semaphore::TeamSemaphore;

use crate::presentation::middleware::auth_middleware::AuthState;

pub async fn team_semaphore_middleware(
    State(semaphore): State<Arc<TeamSemaphore>>,
    Extension(user): Extension<AuthState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let _permit = semaphore.acquire(user.team_id).await;
    Ok(next.run(request).await)
}
