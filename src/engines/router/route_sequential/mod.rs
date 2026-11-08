// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 包含传统顺序模式的路由实现（带重试、身份升级、MRT 瀑布式 fallback）。
//!
//! 子模块拆分（单文件 ≤800 行门禁）：
//! - [`hints`]：host 反爬提示 / 纪律 gate / 空候选内存水位降级
//! - [`attempt`]：attempt 身份升级指令计算与请求构造（F2/F5）
//! - [`antibot`]：反爬挑战处理与 F4 失败现场快照（F2/F4/F7）
//! - [`finalize`]：成功/失败收尾簿记与 MRT 超时错误构造
//! - [`outcome`]：attempt 成功/失败/超时三臂的收尾控制流

#[cfg(feature = "content")]
mod antibot;
mod attempt;
mod finalize;
mod hints;
mod outcome;

use super::{EngineRouter, LoadBalancingStrategy};
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::utils::retry::{RetryReason, RetryTracker};
use log::{debug, warn};
use outcome::AttemptReport;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `route_internal` 准备阶段的结果。
enum PreparedRoute {
    /// 候选引擎已就绪，进入顺序/竞速执行。
    Ready(RoutePlan),
    /// 准备阶段已产生最终结果（无候选，或竞速模式已返回）。
    Immediate(Result<InternalScrapeResponse, EngineError>),
}

/// 顺序执行所需的请求上下文与候选引擎。
struct RoutePlan {
    host: Option<String>,
    request: InternalScrapeRequest,
    candidates: Vec<(f64, Arc<dyn ScraperEngine>)>,
    start_time: Instant,
}

/// 单次引擎尝试的静态上下文。
struct AttemptContext<'a> {
    request: &'a InternalScrapeRequest,
    host: Option<&'a str>,
    start_time: Instant,
    max_retries: usize,
}

/// 跨 attempt 保留的可变重试状态。
struct AttemptState<'a> {
    tracker: &'a mut RetryTracker,
    last_error: &'a mut Option<EngineError>,
    last_reason: &'a mut Option<RetryReason>,
    force_needs_js: &'a mut bool,
}

/// 单次引擎尝试的控制流结果。
enum AttemptOutcome {
    /// 尝试结束并携带最终路由结果（成功响应或终止错误）。
    Done(Result<InternalScrapeResponse, EngineError>),
    /// 本引擎失败，继续下一候选。
    Retry,
}

impl EngineRouter {
    /// Internal route implementation without timeout
    #[allow(unused_variables)]
    pub(super) async fn route_internal(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        match self.prepare_route(request).await {
            PreparedRoute::Immediate(outcome) => outcome,
            PreparedRoute::Ready(plan) => self.run_sequential(plan).await,
        }
    }

    /// 候选选择与前置处理（host hints / 空候选降级 / 轮询 / 竞速分派）。
    async fn prepare_route(&self, request: &InternalScrapeRequest) -> PreparedRoute {
        let start_time = Instant::now();

        let (host, request) = self.apply_host_hints(request).await;

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(&request);

        self.metrics.record_candidates(candidates.len());

        if candidates.is_empty() {
            if let Some(outcome) = self.handle_no_candidates(&request).await {
                return PreparedRoute::Immediate(outcome);
            }
            warn!("No suitable engines available for request");
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
            return PreparedRoute::Immediate(Err(EngineError::AllEnginesFailed(
                "No suitable engines available".to_string(),
            )));
        }

        // 轮询策略特殊处理
        if self.strategy == LoadBalancingStrategy::RoundRobin {
            let start_index = self.get_next_round_robin_index(candidates.len());
            candidates.rotate_left(start_index);
        }

        debug!(
            "Selected {} candidate engines using {:?} strategy",
            candidates.len(),
            self.strategy
        );

        // 并发竞速模式
        if self.race_mode_enabled && candidates.len() > 1 {
            return PreparedRoute::Immediate(
                self.route_race_mode(&request, candidates, start_time).await,
            );
        }

        PreparedRoute::Ready(RoutePlan {
            host,
            request,
            candidates,
            start_time,
        })
    }

    /// 传统顺序模式执行体（带 max_retries 限制）。
    async fn run_sequential(&self, plan: RoutePlan) -> Result<InternalScrapeResponse, EngineError> {
        let RoutePlan {
            host,
            request,
            candidates,
            start_time,
        } = plan;

        // 传统顺序模式 (带 max_retries 限制)
        let max_attempts = self.max_engine_attempts.max(1).min(candidates.len());
        let max_retries = self.max_retries.max(1);
        let mut total_attempts = 0;
        let mut last_error = None;
        // anti-bot 检测命中后，后续 attempt 强制 needs_js=true，
        // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬挑战。
        // 仅 `antibot` 特性启用时 `force_needs_js` 会被改写；关闭时需抑制 unused_mut。
        #[cfg_attr(not(feature = "content"), allow(unused_mut))]
        let mut force_needs_js = false;

        // 智能重试基础设施
        // - RetryTracker：各 reason 独立计数与上限（AntiBot cap=2、FeatureToggle cap=3、total=5）
        // - UaPool：按 attempt 稳定轮换 UA（pick_seeded）
        // - last_reason：上次失败原因，用于计算下次 attempt 的 RetryDirective
        let mut tracker = RetryTracker::new_default();
        let mut last_reason: Option<RetryReason> = None;

        let context = AttemptContext {
            request: &request,
            host: host.as_deref(),
            start_time,
            max_retries,
        };
        let mut state = AttemptState {
            tracker: &mut tracker,
            last_error: &mut last_error,
            last_reason: &mut last_reason,
            force_needs_js: &mut force_needs_js,
        };

        for (score, engine) in candidates.into_iter().take(max_attempts) {
            total_attempts += 1;
            match self
                .attempt_engine(engine, score, total_attempts, &context, &mut state)
                .await
            {
                AttemptOutcome::Done(result) => return result,
                AttemptOutcome::Retry => {}
            }
        }

        warn!("All engines failed for request to {}", request.url);
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        Err(last_error
            .unwrap_or_else(|| EngineError::AllEnginesFailed("All engines failed".to_string())))
    }

    /// 单引擎尝试：执行一次带 MRT 包裹的 scrape，并按结果更新重试状态。
    async fn attempt_engine(
        &self,
        engine: Arc<dyn ScraperEngine>,
        score: f64,
        total_attempts: usize,
        context: &AttemptContext<'_>,
        state: &mut AttemptState<'_>,
    ) -> AttemptOutcome {
        let request = context.request;
        let host = context.host;
        let start_time = context.start_time;

        let engine_name = engine.name();

        self.metrics.record_engine_selection(engine_name);
        self.metrics.record_attempt();

        debug!(
            "Trying engine {} with score {:.2} for request to {}",
            engine_name, score, request.url
        );

        let remaining = request
            .timeout
            .checked_sub(start_time.elapsed())
            .unwrap_or(Duration::from_millis(0));
        if remaining.is_zero() {
            return AttemptOutcome::Done(Err(EngineError::Timeout(request.timeout)));
        }

        let directive = self.compute_attempt_directive(total_attempts, *state.last_reason, host);

        let (attempt_request, session_identity) = self.build_attempt_request(
            request,
            host,
            directive,
            total_attempts,
            *state.force_needs_js,
            remaining,
        );

        let engine_start = Instant::now();

        // 瀑布式 MRT 超时包裹
        //
        // 单引擎调用以 `min(remaining, engine.max_response_time())` 包裹：
        // - `remaining` = 请求整体剩余时间（request.timeout - 已耗时）
        // - `mrt` = 引擎级最大响应时间（engine.max_response_time()）
        //
        // 取 min 确保：
        // 1. remaining < mrt：请求整体超时优先（不浪费 MRT 配额）
        // 2. mrt < remaining：超 MRT 即切下一引擎（瀑布式 fallback）
        //
        // race_mode 路径不受影响（保留作为可选模式，在 route_race_mode 中独立处理）。
        let engine_mrt = engine.max_response_time();
        let effective_timeout = std::cmp::min(remaining, engine_mrt);

        match tokio::time::timeout(effective_timeout, engine.scrape(&attempt_request)).await {
            Ok(Ok(response)) => {
                let report = AttemptReport {
                    engine_name,
                    response_time: engine_start.elapsed(),
                    total_attempts,
                };
                self.finish_attempt_response(
                    &report,
                    response,
                    attempt_request.needs_js,
                    session_identity.as_ref(),
                    context,
                    state,
                )
                .await
            }
            Ok(Err(e)) => {
                let report = AttemptReport {
                    engine_name,
                    response_time: engine_start.elapsed(),
                    total_attempts,
                };
                self.handle_attempt_failure(report, e, context, state).await
            }
            Err(_elapsed) => {
                let report = AttemptReport {
                    engine_name,
                    response_time: engine_start.elapsed(),
                    total_attempts,
                };
                self.handle_attempt_timeout(
                    report,
                    effective_timeout,
                    engine_mrt,
                    remaining,
                    context,
                    state,
                )
                .await
            }
        }
    }
}
