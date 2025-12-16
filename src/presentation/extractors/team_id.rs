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

use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::headers::{Header, HeaderName};
use axum_extra::TypedHeader;
use serde_json::json;
use uuid::Uuid;

static HEADER_NAME: &str = "X-Team-Id";

pub struct TeamIdHeader(pub Uuid);

impl Header for TeamIdHeader {
    fn name() -> &'static HeaderName {
        static NAME: once_cell::sync::Lazy<HeaderName> =
            once_cell::sync::Lazy::new(|| HeaderName::from_static(HEADER_NAME));
        &NAME
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, axum_extra::headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i axum::http::HeaderValue>,
    {
        let value = values
            .next()
            .ok_or_else(axum_extra::headers::Error::invalid)?;
        let uuid = Uuid::parse_str(
            value
                .to_str()
                .map_err(|_| axum_extra::headers::Error::invalid())?,
        )
        .map_err(|_| axum_extra::headers::Error::invalid())?;
        Ok(TeamIdHeader(uuid))
    }

    fn encode<E: Extend<axum::http::HeaderValue>>(&self, _values: &mut E) {}
}

#[derive(Debug, Clone, Copy)]
pub struct TeamId(pub Uuid);

#[async_trait]
impl<S> FromRequestParts<S> for TeamId
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match <TypedHeader<TeamIdHeader> as FromRequestParts<S>>::from_request_parts(parts, state)
            .await
        {
            Ok(TypedHeader(TeamIdHeader(team_id))) => Ok(TeamId(team_id)),
            Err(_) => {
                let status = axum::http::StatusCode::BAD_REQUEST;
                let body = Json(json!({ "error": "Missing or invalid X-Team-Id header" }));
                Err((status, body).into_response())
            }
        }
    }
}
