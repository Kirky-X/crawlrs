// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

static HEADER_NAME: &str = "x-team-id";

#[derive(Debug, Clone, Copy)]
pub struct TeamId(pub Uuid);

impl TeamId {
    /// 从 HeaderMap 中提取 TeamId
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, Box<Response>> {
        match headers.get(HEADER_NAME) {
            Some(header_value) => match header_value.to_str() {
                Ok(uuid_str) => match Uuid::parse_str(uuid_str) {
                    Ok(uuid) => Ok(TeamId(uuid)),
                    Err(_) => {
                        let status = axum::http::StatusCode::BAD_REQUEST;
                        let body =
                            Json(json!({ "error": "Invalid UUID format in X-Team-Id header" }));
                        Err(Box::new((status, body).into_response()))
                    }
                },
                Err(_) => {
                    let status = axum::http::StatusCode::BAD_REQUEST;
                    let body = Json(json!({ "error": "Invalid header value in X-Team-Id header" }));
                    Err(Box::new((status, body).into_response()))
                }
            },
            None => {
                let status = axum::http::StatusCode::BAD_REQUEST;
                let body = Json(json!({ "error": "Missing X-Team-Id header" }));
                Err(Box::new((status, body).into_response()))
            }
        }
    }

    /// 获取 UUID 字符串表示
    pub fn as_string(&self) -> String {
        self.0.to_string()
    }
}

///// 提供一个便利的函数，用于在处理器中使用
pub fn extract_team_id(headers: &HeaderMap) -> Result<TeamId, Box<Response>> {
    TeamId::from_headers(headers)
}
