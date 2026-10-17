// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例生命周期管理 API（auth 保护路由组）
//!
//! 三端点：
//! - `GET /v1/searxng/instances` — 登记簿分页列表
//! - `GET /v1/searxng/pool` — 当前热池 + 熔断镜像态 + 健康口径
//! - `POST /v1/searxng/instances/collect` — 同步触发一轮采集并返回计数报告
//!
//! `search.searxng.enabled = false`（实例服务为 None）时三端点统一返回 503，
//! 错误信息含 `searxng disabled`。

use std::sync::Arc;

use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use log::info;
use serde::{Deserialize, Serialize};

use crate::domain::services::searxng::{
    BreakerMirror, CollectorReport, PoolSnapshot, SearxngInstanceService,
};
use crate::presentation::handlers::response_builder::{error_response, ApiResponse};

/// 管理 API 分页上限（对齐 `search.max_results` 语义）
const MAX_PAGE_LIMIT: u64 = 100;
/// 默认分页大小
const DEFAULT_PAGE_LIMIT: u64 = 20;

/// disabled 错误信息（spec：信息必须含 "searxng disabled"）
const DISABLED_MESSAGE: &str = "searxng disabled (search.searxng.enabled=false)";

/// 分页查询参数
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    /// 每页条数（上限 100）
    pub limit: Option<u64>,
    /// 偏移量
    pub offset: Option<u64>,
}

/// 登记簿条目 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngInstanceDto {
    /// 规范化 base url
    pub url: String,
    /// 生命周期状态
    pub state: String,
    /// 来源（searx_space / seed）
    pub source: String,
    /// 日在线率
    pub uptime_day: Option<f64>,
    /// 最近成功探测时延（毫秒）
    pub latency_ms: Option<i64>,
    /// 最近失败原因
    pub last_error: Option<String>,
    /// 最近更新时间（RFC3339）
    pub updated_at: String,
}

/// 登记簿列表 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngInstancesDto {
    /// 条目
    pub instances: Vec<SearxngInstanceDto>,
    /// 生效的每页条数
    pub limit: u64,
    /// 生效的偏移量
    pub offset: u64,
}

/// 热池成员 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngPoolMemberDto {
    /// 实例 url
    pub url: String,
    /// 登记簿状态
    pub state: String,
    /// 最近成功时延
    pub latency_ms: Option<i64>,
    /// 熔断镜像态（closed / open / half_open）
    pub breaker: BreakerMirror,
}

/// 热池视图 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngPoolDto {
    /// 控制回路目标成员数
    pub target: usize,
    /// 健康口径（healthy / degraded / unhealthy）
    pub health: String,
    /// 当前成员
    pub members: Vec<SearxngPoolMemberDto>,
}

/// 采集报告 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngCollectorReportDto {
    /// 本轮是否因并发锁跳过
    pub skipped: bool,
    /// 数据集条目总数
    pub fetched: usize,
    /// 通过准入的条目数
    pub seen: usize,
    /// 新增行数
    pub admitted_new: usize,
    /// 刷新行数
    pub updated: usize,
    /// 退役数
    pub retired: usize,
    /// 准入拒绝 + 写入失败总数
    pub errors: usize,
}

impl From<CollectorReport> for SearxngCollectorReportDto {
    fn from(report: CollectorReport) -> Self {
        Self {
            skipped: report.skipped,
            fetched: report.fetched,
            seen: report.seen,
            admitted_new: report.admitted_new,
            updated: report.updated,
            retired: report.retired,
            errors: report.errors,
        }
    }
}

/// 健康口径 → 字符串
fn health_label(health: crate::search::types::EngineHealth) -> &'static str {
    use crate::search::types::EngineHealth;
    match health {
        EngineHealth::Healthy => "healthy",
        EngineHealth::Degraded => "degraded",
        EngineHealth::Unhealthy => "unhealthy",
        EngineHealth::Unknown => "unknown",
        EngineHealth::Isolated => "isolated",
    }
}

/// 池快照 → DTO
fn pool_dto(snapshot: PoolSnapshot) -> SearxngPoolDto {
    SearxngPoolDto {
        target: snapshot.target,
        health: health_label(snapshot.health).to_string(),
        members: snapshot
            .members
            .into_iter()
            .map(|m| SearxngPoolMemberDto {
                url: m.url,
                state: m.state.as_str().to_string(),
                latency_ms: m.latency_ms,
                breaker: m.breaker,
            })
            .collect(),
    }
}

/// `GET /v1/searxng/instances`
pub async fn list_instances(
    Extension(searxng): Extension<Option<Arc<dyn SearxngInstanceService>>>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let Some(service) = searxng else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, DISABLED_MESSAGE);
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .min(MAX_PAGE_LIMIT);
    let offset = query.offset.unwrap_or(0);

    match service.list_instances(limit, offset).await {
        Ok(instances) => {
            let dto = SearxngInstancesDto {
                instances: instances
                    .into_iter()
                    .map(|i| SearxngInstanceDto {
                        url: i.url,
                        state: i.state.as_str().to_string(),
                        source: i.source.as_str().to_string(),
                        uptime_day: i.uptime_day,
                        latency_ms: i.latency_ms,
                        last_error: i.last_error,
                        updated_at: i.updated_at.to_rfc3339(),
                    })
                    .collect(),
                limit,
                offset,
            };
            (StatusCode::OK, Json(ApiResponse::success(dto))).into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `GET /v1/searxng/pool`
pub async fn get_pool(
    Extension(searxng): Extension<Option<Arc<dyn SearxngInstanceService>>>,
) -> impl IntoResponse {
    let Some(service) = searxng else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, DISABLED_MESSAGE);
    };
    let snapshot = service.pool_snapshot().await;
    (
        StatusCode::OK,
        Json(ApiResponse::success(pool_dto(snapshot))),
    )
        .into_response()
}

/// `POST /v1/searxng/instances/collect`
pub async fn trigger_collect(
    Extension(searxng): Extension<Option<Arc<dyn SearxngInstanceService>>>,
) -> impl IntoResponse {
    let Some(service) = searxng else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, DISABLED_MESSAGE);
    };
    info!("searxng collection triggered by management API");

    match service.collect().await {
        Ok(report) => {
            let dto: SearxngCollectorReportDto = report.into();
            (StatusCode::OK, Json(ApiResponse::success(dto))).into_response()
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("searxng collect failed: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    use crate::domain::repositories::searxng_instance_repository::SearxngInstanceState;
    use crate::domain::services::searxng::test_support::{
        shared_repo, test_settings, InMemorySearxngRepo, Scripted, ScriptedTransport,
    };
    use crate::domain::services::searxng::SearxngInstanceServiceImpl;

    fn router(service: Option<Arc<dyn SearxngInstanceService>>) -> Router {
        Router::new()
            .route("/v1/searxng/instances", get(list_instances))
            .route("/v1/searxng/pool", get(get_pool))
            .route("/v1/searxng/instances/collect", post(trigger_collect))
            .layer(Extension(service))
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    async fn service_with_instance() -> (Arc<InMemorySearxngRepo>, Arc<dyn SearxngInstanceService>)
    {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://searx.a.example",
            SearxngInstanceState::Active,
        ));
        let service: Arc<dyn SearxngInstanceService> = Arc::new(SearxngInstanceServiceImpl::new(
            repo.clone(),
            Arc::new(ScriptedTransport::new(vec![])),
            test_settings(),
        ));
        (repo, service)
    }

    #[tokio::test]
    async fn test_list_instances_returns_dto_fields() {
        let (_repo, service) = service_with_instance().await;
        let response = router(Some(service))
            .oneshot(
                axum::http::Request::builder()
                    .uri("/v1/searxng/instances?limit=1000&offset=0")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["success"], true);
        assert_eq!(body["data"]["limit"], 100, "limit must be capped at 100");
        let item = &body["data"]["instances"][0];
        assert_eq!(item["url"], "https://searx.a.example");
        assert_eq!(item["state"], "active");
        assert_eq!(item["source"], "searx_space");
        assert!(item.get("uptime_day").is_some());
        assert!(item.get("latency_ms").is_some());
        assert!(item.get("last_error").is_some());
        assert!(item.get("updated_at").is_some());
    }

    #[tokio::test]
    async fn test_get_pool_returns_members_and_health() {
        let (_repo, service) = service_with_instance().await;
        let response = router(Some(service))
            .oneshot(
                axum::http::Request::builder()
                    .uri("/v1/searxng/pool")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["data"]["target"], 3);
        // 实例已登记但未入池（bootstrap 未跑）→ 空池 + 重建未完成 = Degraded
        assert_eq!(body["data"]["members"].as_array().expect("array").len(), 0);
        assert_eq!(body["data"]["health"], "degraded");
    }

    #[tokio::test]
    async fn test_trigger_collect_returns_report_counts() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({
                "instances": {
                    "https://searx.new.example": {
                        "network_type": "normal", "api": true, "uptime": { "day": 99.0 }
                    },
                    "http://searx.plain.example": { "network_type": "normal", "api": true }
                }
            }),
        }]));
        let service: Arc<dyn SearxngInstanceService> = Arc::new(SearxngInstanceServiceImpl::new(
            repo.clone(),
            transport,
            test_settings(),
        ));

        let response = router(Some(service))
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/v1/searxng/instances/collect")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["data"]["skipped"], false);
        assert_eq!(body["data"]["fetched"], 2);
        assert_eq!(body["data"]["seen"], 1);
        assert_eq!(body["data"]["admitted_new"], 1);
        assert_eq!(body["data"]["updated"], 0);
        assert_eq!(body["data"]["retired"], 0);
        assert_eq!(body["data"]["errors"], 1);
    }

    #[tokio::test]
    async fn test_all_endpoints_return_503_when_disabled() {
        let app = router(None);
        for (method, uri) in [
            (axum::http::Method::GET, "/v1/searxng/instances"),
            (axum::http::Method::GET, "/v1/searxng/pool"),
            (axum::http::Method::POST, "/v1/searxng/instances/collect"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = body_json(response).await;
            assert_eq!(body["success"], false);
            let message = body["error"]["message"].as_str().expect("message");
            assert!(
                message.contains("searxng disabled"),
                "disabled message must mention searxng disabled, got: {message}"
            );
        }
    }

    #[tokio::test]
    async fn test_collect_error_returns_500_with_reason() {
        let repo = shared_repo();
        let service: Arc<dyn SearxngInstanceService> = Arc::new(SearxngInstanceServiceImpl::new(
            repo,
            Arc::new(ScriptedTransport::new(vec![Scripted::Network(
                "connection refused".to_string(),
            )])),
            test_settings(),
        ));

        let response = router(Some(service))
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/v1/searxng/instances/collect")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(response).await;
        let message = body["error"]["message"].as_str().expect("message");
        assert!(message.contains("connection refused"), "got: {message}");
    }
}
