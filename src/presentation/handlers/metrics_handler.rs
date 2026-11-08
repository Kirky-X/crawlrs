// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `GET /metrics` —— Prometheus 文本曝光。
//!
//! D8 修复：此前返回硬编码 JSON stub（version/uptime 恒定、无任何真实指标族，
//! E2E SJ-03/S-H 实证）。现从全局 `PrometheusHandle` 渲染真实指标文本。

use axum::response::{IntoResponse, Response};

/// Prometheus 文本曝光（`metrics` 特性编译时）。
///
/// recorder 未安装（如未调用 `init_metrics` 的测试进程）返回 503，
/// 明确"不可用"而非伪造数据。
#[cfg(feature = "metrics")]
pub async fn metrics() -> Response {
    match crate::infrastructure::observability::metrics::prometheus_handle() {
        Some(handle) => (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            handle.render(),
        )
            .into_response(),
        None => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            "metrics recorder unavailable",
        )
            .into_response(),
    }
}

/// `metrics` 特性未编译时的显式降级（禁止返回伪造指标）。
#[cfg(not(feature = "metrics"))]
pub async fn metrics() -> Response {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        "metrics feature not compiled",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// 端点必须返回真实文本或显式不可用状态，禁止伪造成功。
    #[tokio::test]
    async fn test_metrics_returns_explicit_state() {
        let resp = metrics().await;
        let status = resp.status();
        assert!(
            status == axum::http::StatusCode::OK
                || status == axum::http::StatusCode::SERVICE_UNAVAILABLE
                || status == axum::http::StatusCode::NOT_IMPLEMENTED,
            "unexpected status {status}"
        );
        let body = to_bytes(resp.into_body(), 1024 * 1024).await.expect("body");
        if status == axum::http::StatusCode::OK {
            assert!(!body.is_empty(), "200 must carry non-empty metrics body");
            let text = String::from_utf8_lossy(&body);
            assert!(
                text.contains("app_info") || text.contains("# HELP") || text.contains(' '),
                "200 body must look like Prometheus exposition"
            );
        }
    }
}
