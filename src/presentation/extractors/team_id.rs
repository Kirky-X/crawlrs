// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};

    fn make_headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static(HEADER_NAME),
            HeaderValue::from_str(value).unwrap(),
        );
        headers
    }

    #[test]
    fn test_from_headers_valid_uuid() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let headers = make_headers(uuid_str);
        let result = TeamId::from_headers(&headers);
        assert!(result.is_ok());
        let team_id = result.unwrap();
        assert_eq!(team_id.0.to_string(), uuid_str);
    }

    #[test]
    fn test_from_headers_missing_header() {
        let headers = HeaderMap::new();
        let result = TeamId::from_headers(&headers);
        assert!(result.is_err());
        let response = *result.unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_from_headers_missing_header_error_message() {
        let headers = HeaderMap::new();
        let result = TeamId::from_headers(&headers);
        let response = *result.unwrap_err();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "Missing X-Team-Id header");
    }

    #[test]
    fn test_from_headers_invalid_uuid_format() {
        let headers = make_headers("not-a-uuid");
        let result = TeamId::from_headers(&headers);
        assert!(result.is_err());
        let response = *result.unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_from_headers_invalid_uuid_format_error_message() {
        let headers = make_headers("not-a-uuid");
        let result = TeamId::from_headers(&headers);
        let response = *result.unwrap_err();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "Invalid UUID format in X-Team-Id header");
    }

    #[test]
    fn test_from_headers_partial_uuid() {
        let headers = make_headers("550e8400-e29b-41d4");
        let result = TeamId::from_headers(&headers);
        assert!(result.is_err());
        let response = *result.unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_from_headers_empty_string() {
        let headers = make_headers("");
        let result = TeamId::from_headers(&headers);
        assert!(result.is_err());
        let response = *result.unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_from_headers_uuid_with_uppercase() {
        let uuid_str = "550E8400-E29B-41D4-A716-446655440000";
        let headers = make_headers(uuid_str);
        let result = TeamId::from_headers(&headers);
        // Uuid::parse_str accepts uppercase
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_headers_nil_uuid() {
        let uuid_str = "00000000-0000-0000-0000-000000000000";
        let headers = make_headers(uuid_str);
        let result = TeamId::from_headers(&headers);
        assert!(result.is_ok());
        let team_id = result.unwrap();
        assert_eq!(team_id.0, Uuid::nil());
    }

    #[test]
    fn test_as_string_returns_uuid_string() {
        let uuid = Uuid::new_v4();
        let team_id = TeamId(uuid);
        assert_eq!(team_id.as_string(), uuid.to_string());
    }

    #[test]
    fn test_as_string_for_nil_uuid() {
        let team_id = TeamId(Uuid::nil());
        assert_eq!(team_id.as_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn test_team_id_copy_and_clone() {
        let uuid = Uuid::new_v4();
        let team_id = TeamId(uuid);
        let cloned = team_id;
        assert_eq!(team_id.0, cloned.0);
    }

    #[test]
    fn test_extract_team_id_valid() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let headers = make_headers(uuid_str);
        let result = extract_team_id(&headers);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.to_string(), uuid_str);
    }

    #[test]
    fn test_extract_team_id_missing() {
        let headers = HeaderMap::new();
        let result = extract_team_id(&headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_team_id_invalid() {
        let headers = make_headers("invalid");
        let result = extract_team_id(&headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_header_name_constant() {
        assert_eq!(HEADER_NAME, "x-team-id");
    }
}
