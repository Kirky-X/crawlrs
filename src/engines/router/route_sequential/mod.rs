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

#[cfg(feature = "content")]
mod antibot;
mod attempt;
mod finalize;
mod hints;

use super::{check_js_upgrade_probe, EngineRouter, LoadBalancingStrategy};
use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use crate::utils::retry::{RetryReason, RetryTracker};
use log::{debug, warn};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl EngineRouter {
    /// Internal route implementation without timeout
    #[allow(unused_variables)]
    pub(super) async fn route_internal(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let start_time = Instant::now();

        let (host, request) = self.apply_host_hints(request).await;

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(&request);

        self.metrics.record_candidates(candidates.len());

        if candidates.is_empty() {
            if let Some(outcome) = self.handle_no_candidates(&request).await {
                return outcome;
            }
            warn!("No suitable engines available for request");
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
            return Err(EngineError::AllEnginesFailed(
                "No suitable engines available".to_string(),
            ));
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
            return self.route_race_mode(&request, candidates, start_time).await;
        }

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

        for (score, engine) in candidates.into_iter().take(max_attempts) {
            total_attempts += 1;
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
                return Err(EngineError::Timeout(request.timeout));
            }

            let directive =
                self.compute_attempt_directive(total_attempts, last_reason, host.as_deref());

            let (attempt_request, session_identity) = self.build_attempt_request(
                &request,
                host.as_deref(),
                directive,
                total_attempts,
                force_needs_js,
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
                    let response_time = engine_start.elapsed();

                    #[cfg(feature = "content")]
                    let response = match self
                        .record_antibot_failure(
                            response,
                            &request.url,
                            engine_name,
                            response_time,
                            session_identity.as_ref(),
                        )
                        .await
                    {
                        Err(challenge_reason) => {
                            last_error = Some(EngineError::AntiBotDetected(challenge_reason));
                            force_needs_js = true;

                            // 记录 AntiBot 失败到 tracker
                            let reason = RetryReason::AntiBot;
                            tracker.record(reason);
                            last_reason = Some(reason);

                            // 检查 tracker 上限（anti_bot cap=2）+ max_retries
                            // 使用 should_stop_after_retry_check 消除重复（DRY）
                            if self.should_stop_after_retry_check(
                                &tracker,
                                reason,
                                total_attempts,
                                max_retries,
                            ) {
                                return Err(last_error.unwrap_or_else(|| {
                                    EngineError::AllEnginesFailed("Max retries reached".to_string())
                                }));
                            }
                            continue;
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
                    if !attempt_request.needs_js {
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
                                return Box::pin(self.route_internal(&js_request)).await;
                            }
                        }
                    }

                    return self
                        .finish_success(
                            response,
                            engine_name,
                            response_time,
                            start_time,
                            host.as_deref(),
                            session_identity.as_ref(),
                        )
                        .await;
                }
                Ok(Err(e)) => {
                    let response_time = engine_start.elapsed();
                    self.record_engine_error(engine_name, response_time, &e);

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name).await;

                        // 记录失败原因到 tracker（在 e move 之前取 reason）
                        let reason = e.retry_reason();
                        tracker.record(reason);
                        last_reason = Some(reason);

                        // 检查 tracker 上限（AntiBot cap=2、FeatureToggle cap=3、total=5）+ max_retries
                        // 使用 should_stop_after_retry_check 消除重复（DRY）
                        if self.should_stop_after_retry_check(
                            &tracker,
                            reason,
                            total_attempts,
                            max_retries,
                        ) {
                            return Err(e);
                        }

                        warn!(
                            "Engine {} failed with retryable error: {}, trying next engine \
                             (reason={:?}, tracker total={})",
                            engine_name,
                            e,
                            reason,
                            tracker.total()
                        );
                        last_error = Some(e);
                        continue;
                    }

                    warn!(
                        "Engine {} failed with non-retryable error: {}",
                        engine_name, e
                    );
                    // 指标口径对齐：提前终止路径同样累计 total/failed，
                    // 否则 success_rate 分母缺失导致统计失真
                    self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                    self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
                    return Err(e);
                }
                Err(_elapsed) => {
                    // MRT 瀑布式超时——`tokio::time::timeout` 在 effective_timeout 后
                    // 取消 engine.scrape() future，进入此分支。
                    //
                    // 根据 effective_timeout 的来源区分两种语义：
                    // - `effective_timeout == remaining`（remaining <= engine_mrt）：
                    //   请求整体超时（剩余时间耗尽），返回 `EngineError::Timeout`。
                    //   此时即使切下一引擎也无力回天，由上层 route_sequential 循环下轮
                    //   的 `remaining.is_zero()` 检查兜底。
                    // - `effective_timeout == engine_mrt`（engine_mrt < remaining）：
                    //   真正的引擎 MRT 超时，返回 `EngineError::EngineMrtExceeded`，
                    //   router 触发瀑布式 fallback 切下一引擎继续。
                    //
                    // 边界情况 `remaining == engine_mrt`：按 Timeout 处理（保守语义，
                    // 避免误认为仍有剩余时间可 fallback）。
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

                    let timeout_err = Self::mrt_timeout_error(
                        engine_name,
                        effective_timeout,
                        engine_mrt,
                        remaining,
                    );

                    self.metrics
                        .record_engine_failure(engine_name, &timeout_err.to_string());

                    self.circuit_breaker.record_failure(engine_name).await;

                    let reason = timeout_err.retry_reason();
                    tracker.record(reason);
                    last_reason = Some(reason);

                    if self.should_stop_after_retry_check(
                        &tracker,
                        reason,
                        total_attempts,
                        max_retries,
                    ) {
                        return Err(timeout_err);
                    }

                    last_error = Some(timeout_err);
                    continue;
                }
            }
        }

        warn!("All engines failed for request to {}", request.url);
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        Err(last_error
            .unwrap_or_else(|| EngineError::AllEnginesFailed("All engines failed".to_string())))
    }
}
