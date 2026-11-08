// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由·attempt 收尾 — 从 `route_sequential/mod.rs` 拆分
//!
//! - `finish_attempt_response`：成功臂（antibot 分类 → SPA 空壳升级探测 → 成功簿记）
//! - `handle_attempt_failure`：`Ok(Err)` 臂（可重试/不可重试分支）
//! - `handle_attempt_timeout`：MRT 超时臂（瀑布式 fallback 记账）

use super::super::check_js_upgrade_probe;
use super::super::EngineRouter;
use super::{AttemptContext, AttemptOutcome, AttemptState};
use crate::engines::engine_client::{EngineError, InternalScrapeResponse};
use crate::engines::session::SessionIdentity;
use crate::utils::retry::RetryReason;
use log::{debug, warn};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// 单次尝试收尾参数（引擎名 / 引擎耗时 / attempt 序号）。
pub(super) struct AttemptReport<'a> {
    pub(super) engine_name: &'a str,
    pub(super) response_time: Duration,
    pub(super) total_attempts: usize,
}

impl EngineRouter {
    /// 成功臂收尾（自 [`EngineRouter::attempt_engine`] 提取）：antibot 分类 →
    /// SPA 空壳升级探测 → 成功簿记。
    pub(super) async fn finish_attempt_response(
        &self,
        report: &AttemptReport<'_>,
        response: InternalScrapeResponse,
        attempt_needs_js: bool,
        session_identity: Option<&Arc<SessionIdentity>>,
        context: &AttemptContext<'_>,
        state: &mut AttemptState<'_>,
    ) -> AttemptOutcome {
        let request = context.request;
        let host = context.host;
        let start_time = context.start_time;
        let max_retries = context.max_retries;
        let engine_name = report.engine_name;
        let response_time = report.response_time;
        let total_attempts = report.total_attempts;

        #[cfg(feature = "content")]
        let response = match self
            .record_antibot_failure(
                response,
                &request.url,
                engine_name,
                response_time,
                session_identity,
            )
            .await
        {
            Err(challenge_reason) => {
                *state.last_error = Some(EngineError::AntiBotDetected(challenge_reason));
                *state.force_needs_js = true;

                // 记录 AntiBot 失败到 tracker
                let reason = RetryReason::AntiBot;
                state.tracker.record(reason);
                *state.last_reason = Some(reason);

                // 检查 tracker 上限（anti_bot cap=2）+ max_retries
                // 使用 should_stop_after_retry_check 消除重复（DRY）
                if self.should_stop_after_retry_check(
                    state.tracker,
                    reason,
                    total_attempts,
                    max_retries,
                ) {
                    return AttemptOutcome::Done(Err(state.last_error.take().unwrap_or_else(
                        || EngineError::AllEnginesFailed("Max retries reached".to_string()),
                    )));
                }
                return AttemptOutcome::Retry;
            }
            Ok(resp) => resp,
        };

        // 流式 HTTP→Chrome 升级探测
        //
        // HTTP 引擎（needs_js==false）返回"成功"响应后，检查是否疑似 SPA 空壳。
        // 若 probe 判定 upgrade，将当前结果视为失败（不返回空壳给用户），
        // 以 needs_js=true 重新 route_internal 改派浏览器引擎（Playwright）渲染。
        //
        // 防递归：递归调用时 request.needs_js=true，attempt_request.needs_js=true，
        // 故 `!attempt_request.needs_js` 为 false，probe 检查自然跳过。
        if !attempt_needs_js {
            let verdict = check_js_upgrade_probe(&response);
            if verdict.upgrade {
                let mut js_request = request.clone();
                js_request.needs_js = true;

                // 死端预检：以 needs_js=true 重选引擎为空（部署未注册
                // 任何 JS-capable 引擎）时，递归只会返回
                // `No suitable engines available` 并丢弃这份已成功的响应。
                // probe 仅是启发式，此时降级返回原始响应：跳过本次 probe
                // 的失败记账，控制流落入下方成功收尾段（stats 成功记账 /
                // 熔断 record_success / host 纪律 / session Cookie 捕获
                // 均按成功语义原样执行）。
                if self.select_optimal_engines(&js_request).is_empty() {
                    warn!(
                        "JS upgrade probe hit (score={}, reason={}) but no \
                         JS-capable engine is registered for the needs_js \
                         re-route; degrading to the original HTTP response \
                         for {}",
                        verdict.score, verdict.reason, request.url
                    );
                } else {
                    self.update_engine_stats(engine_name, false, response_time);
                    self.metrics.record_engine_failure(
                        engine_name,
                        &format!("js-upgrade-probe: {}", verdict.reason),
                    );
                    debug!(
                        "Engine {} returned SPA shell (probe score={}, reason={}); \
                         re-routing with needs_js=true to dispatch browser engine",
                        engine_name, verdict.score, verdict.reason
                    );
                    return AttemptOutcome::Done(Box::pin(self.route_internal(&js_request)).await);
                }
            }
        }

        AttemptOutcome::Done(
            self.finish_success(
                response,
                engine_name,
                response_time,
                start_time,
                host,
                session_identity,
            )
            .await,
        )
    }

    /// 失败臂收尾（自 [`EngineRouter::attempt_engine`] 提取）：记录失败簿记，
    /// 按可重试性更新 tracker 并决定继续下一候选或终止。
    pub(super) async fn handle_attempt_failure(
        &self,
        report: AttemptReport<'_>,
        e: EngineError,
        context: &AttemptContext<'_>,
        state: &mut AttemptState<'_>,
    ) -> AttemptOutcome {
        let engine_name = report.engine_name;
        let response_time = report.response_time;
        let total_attempts = report.total_attempts;
        let max_retries = context.max_retries;

        self.record_engine_error(engine_name, response_time, &e);

        if e.is_retryable() {
            self.circuit_breaker.record_failure(engine_name).await;

            // 记录失败原因到 tracker（在 e move 之前取 reason）
            let reason = e.retry_reason();
            state.tracker.record(reason);
            *state.last_reason = Some(reason);

            // 检查 tracker 上限（AntiBot cap=2、FeatureToggle cap=3、total=5）+ max_retries
            // 使用 should_stop_after_retry_check 消除重复（DRY）
            if self.should_stop_after_retry_check(
                state.tracker,
                reason,
                total_attempts,
                max_retries,
            ) {
                return AttemptOutcome::Done(Err(e));
            }

            warn!(
                "Engine {} failed with retryable error: {}, trying next engine \
                 (reason={:?}, tracker total={})",
                engine_name,
                e,
                reason,
                state.tracker.total()
            );
            *state.last_error = Some(e);
            return AttemptOutcome::Retry;
        }

        warn!(
            "Engine {} failed with non-retryable error: {}",
            engine_name, e
        );
        // 指标口径对齐：提前终止路径同样累计 total/failed，
        // 否则 success_rate 分母缺失导致统计失真
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        AttemptOutcome::Done(Err(e))
    }

    /// 超时臂收尾（自 [`EngineRouter::attempt_engine`] 提取）：
    ///
    /// MRT 瀑布式超时——`tokio::time::timeout` 在 effective_timeout 后
    /// 取消 engine.scrape() future，进入此分支。
    ///
    /// 根据 effective_timeout 的来源区分两种语义：
    /// - `effective_timeout == remaining`（remaining <= engine_mrt）：
    ///   请求整体超时（剩余时间耗尽），返回 `EngineError::Timeout`。
    ///   此时即使切下一引擎也无力回天，由上层 route_sequential 循环下轮
    ///   的 `remaining.is_zero()` 检查兜底。
    /// - `effective_timeout == engine_mrt`（engine_mrt < remaining）：
    ///   真正的引擎 MRT 超时，返回 `EngineError::EngineMrtExceeded`，
    ///   router 触发瀑布式 fallback 切下一引擎继续。
    ///
    /// 边界情况 `remaining == engine_mrt`：按 Timeout 处理（保守语义，
    /// 避免误认为仍有剩余时间可 fallback）。
    pub(super) async fn handle_attempt_timeout(
        &self,
        report: AttemptReport<'_>,
        effective_timeout: Duration,
        engine_mrt: Duration,
        remaining: Duration,
        context: &AttemptContext<'_>,
        state: &mut AttemptState<'_>,
    ) -> AttemptOutcome {
        let engine_name = report.engine_name;
        let response_time = report.response_time;
        let total_attempts = report.total_attempts;
        let max_retries = context.max_retries;

        self.update_engine_stats(engine_name, false, response_time);

        let timeout_err =
            Self::mrt_timeout_error(engine_name, effective_timeout, engine_mrt, remaining);

        self.metrics
            .record_engine_failure(engine_name, &timeout_err.to_string());

        self.circuit_breaker.record_failure(engine_name).await;

        let reason = timeout_err.retry_reason();
        state.tracker.record(reason);
        *state.last_reason = Some(reason);

        if self.should_stop_after_retry_check(state.tracker, reason, total_attempts, max_retries) {
            return AttemptOutcome::Done(Err(timeout_err));
        }

        *state.last_error = Some(timeout_err);
        AttemptOutcome::Retry
    }
}
