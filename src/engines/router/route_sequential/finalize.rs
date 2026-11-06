// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由·结果收尾 — 从 `route_sequential.rs` 拆分
//!
//! - `finish_success`：成功臂尾部簿记（统计/熔断/指标/host 纪律/Session）
//! - `record_engine_error`：`Ok(Err)` 臂头部失败簿记
//! - `mrt_timeout_error`：MRT 瀑布式超时错误构造

use super::super::EngineRouter;
use crate::common::metrics_shim::{counter, histogram};
use crate::engines::engine_client::{EngineError, InternalScrapeResponse};
use crate::engines::session::SessionIdentity;
use crate::utils::host_discipline::ObserveOutcome;
use log::{info, warn};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 解析 Retry-After 头（delta-seconds 或 HTTP 日期），供纯 429 直通观测使用。
///
/// 与 `antibot::parse_retry_after` 共用 [`crate::utils::retry_after`] 单实现
/// （含 600s 钳制），仅头部容器不同；不依赖 `content` feature——antibot
/// 分类关闭时 429 仍能携带 Retry-After 进入 host 纪律。
fn parse_retry_after_header(
    headers: &std::collections::HashMap<String, String>,
) -> Option<Duration> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| crate::utils::retry_after::parse_retry_after_value(v))
        .map(Duration::from_secs)
}

impl EngineRouter {
    /// 成功收尾（route_internal 成功臂尾部，原样提取）：引擎统计、熔断、
    /// Prometheus 指标、host 纪律观测（F1/F5）、Session 记分与 Cookie 捕获（F2）。
    pub(super) async fn finish_success(
        &self,
        response: InternalScrapeResponse,
        engine_name: &str,
        response_time: Duration,
        start_time: Instant,
        host: Option<&str>,
        session_identity: Option<&Arc<SessionIdentity>>,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self.update_engine_stats(engine_name, true, response_time);
        self.circuit_breaker.record_success(engine_name).await;

        // Prometheus 指标埋点
        counter!(
            "crawlrs_engine_success_total",
            "engine" => engine_name.to_string(),
            "result" => "success"
        )
        .increment(1);
        histogram!(
            "crawlrs_engine_duration_seconds",
            "engine" => engine_name.to_string()
        )
        .record(response_time.as_secs_f64());

        // 记录成功指标
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .successful_requests
            .fetch_add(1, Ordering::Relaxed);
        self.metrics
            .record_engine_latency(engine_name, response_time);
        self.metrics.record_engine_success(engine_name);

        info!(
            "Engine {} succeeded in {:?}, total time: {:?}",
            engine_name,
            response_time,
            start_time.elapsed()
        );

        // host 纪律观测：最终 2xx 成功 → 清除冷却/连击记忆并按
        // 引擎耗时收敛 AutoThrottle 间隔（F1）；纯 429 若未被上面的
        // antibot 分类拦截（content feature 关闭或指纹不匹配），
        // 直通进入 host 纪律（F5）——限速响应必须触发冷却。
        if let Some(host) = host {
            if (200..300).contains(&response.status_code) {
                self.host_discipline
                    .observe_success_with_latency(host, response_time);
            } else if response.status_code == 429 {
                self.host_discipline.observe(
                    host,
                    ObserveOutcome::RateLimited {
                        retry_after: parse_retry_after_header(&response.headers),
                    },
                );
            }
        }

        // F2 Session 记分与 Cookie 捕获（与 host 纪律观测同址）：
        // 2xx → mark_good；401/403/429 → mark_bad（记分退休点位，
        // 429 若未被上面 antibot 分类拦截也在此记分）；
        // Set-Cookie 累积进身份 Cookie 通道（按目标 host 分桶，
        // 跨站凭据隔离）供后续同 host 请求注入。
        if let Some(identity) = session_identity {
            if (200..300).contains(&response.status_code) {
                self.session.mark_good(identity);
            } else if matches!(response.status_code, 401 | 403 | 429) {
                self.session.mark_bad(identity);
            }
            self.session
                .capture_cookies(identity, host.unwrap_or(""), &response.headers);
        }

        Ok(response)
    }

    /// 引擎失败簿记（route_internal `Ok(Err)` 臂头部，原样提取）：
    /// 引擎统计 + Prometheus 指标 + 失败指标。
    pub(super) fn record_engine_error(
        &self,
        engine_name: &str,
        response_time: Duration,
        error: &EngineError,
    ) {
        self.update_engine_stats(engine_name, false, response_time);

        // Prometheus 指标埋点
        counter!(
            "crawlrs_engine_success_total",
            "engine" => engine_name.to_string(),
            "result" => "failure"
        )
        .increment(1);
        histogram!(
            "crawlrs_engine_duration_seconds",
            "engine" => engine_name.to_string()
        )
        .record(response_time.as_secs_f64());

        // 记录失败指标
        self.metrics
            .record_engine_failure(engine_name, &error.to_string());
    }

    /// MRT 瀑布式超时错误构造（route_internal 超时臂，原样提取）：
    /// `effective_timeout <= remaining` 时为请求整体超时（保守语义，边界
    /// 相等也按 Timeout 处理），否则为引擎级 MRT 超时（瀑布式 fallback）。
    pub(super) fn mrt_timeout_error(
        engine_name: &str,
        effective_timeout: Duration,
        engine_mrt: Duration,
        remaining: Duration,
    ) -> EngineError {
        // 判断本次超时来源（边界 == 走 Timeout 分支）
        let is_overall_timeout = effective_timeout <= remaining;
        let timeout_err = if is_overall_timeout {
            // remaining 耗尽（remaining <= engine_mrt）→ 请求整体超时
            warn!(
                "Request overall timeout (remaining={:?} <= mrt={:?}); \
                             engine={} cancelled at effective_timeout={:?}",
                remaining, engine_mrt, engine_name, effective_timeout
            );
            EngineError::Timeout(effective_timeout)
        } else {
            // engine_mrt < remaining → 引擎级 MRT 超时
            let mrt_err = EngineError::EngineMrtExceeded {
                engine: engine_name.to_string(),
                mrt: effective_timeout,
            };
            warn!(
                "Engine {} exceeded MRT (effective_timeout={:?}, mrt={:?}, remaining={:?}); \
                             waterfall fallback to next engine",
                engine_name, effective_timeout, engine_mrt, remaining
            );
            mrt_err
        };
        timeout_err
    }
}
