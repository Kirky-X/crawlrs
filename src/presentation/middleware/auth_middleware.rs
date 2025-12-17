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

use crate::infrastructure::database::entities::api_key;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

use std::sync::Arc;

/// 认证状态
#[derive(Clone)]
pub struct AuthState {
    /// 数据库连接
    pub db: Arc<DatabaseConnection>,
    pub team_id: Uuid,
}

/// 认证中间件
///
/// 验证请求中的API密钥
///
/// # 参数
///
/// * `state` - 认证状态
/// * `req` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(Response)` - 认证成功的响应
/// * `Err(StatusCode)` - 认证失败的状态码
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Allow public endpoints
    let path = req.uri().path();
    use tracing::debug;

    // ...

    // inside middleware function
    debug!("AuthMiddleware processing path: {}", path);
    if path == "/health" || path == "/v1/version" {
        return Ok(next.run(req).await);
    }

    let token_str = {
        let auth_header = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|header| header.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(StatusCode::UNAUTHORIZED);
        }

        auth_header[7..].to_string()
    };

    // Query DB to validate token and get Team ID
    match api_key::Entity::find()
        .filter(api_key::Column::Key.eq(token_str.clone()))
        .one(state.db.as_ref())
        .await
    {
        Ok(Some(key)) => {
            // Inject Team ID and API Key into extensions
            req.extensions_mut().insert(key.team_id);
            req.extensions_mut().insert(token_str);
            req.extensions_mut().insert(AuthState {
                db: state.db.clone(),
                team_id: key.team_id,
            });
            Ok(next.run(req).await)
        }
        Ok(None) => {
            tracing::warn!("API Key not found: {}", token_str);
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            tracing::error!("Database error checking API key: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
