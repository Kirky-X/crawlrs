// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T035: 顺序路由 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 包含传统顺序模式的路由实现（带重试、身份升级、MRT 瀑布式 fallback）。

use super::{check_js_upgrade_probe, EngineRouter, LoadBalancingStrategy};
#[cfg(feature = "antibot")]
use super::check_antibot_response;
use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use crate::utils::retry::{RetryDirective, RetryReason, RetryTracker};
use log::{debug, info, warn};
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

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(request);

        // 记录候选引擎数量
        self.metrics.record_candidates(candidates.len());

        if candidates.is_empty() {
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
            return self.route_race_mode(request, candidates, start_time).await;
        }

        // 传统顺序模式 (带 max_retries 限制)
        let max_attempts = self.max_engine_attempts.max(1).min(candidates.len());
        let max_retries = self.max_retries.max(1);
        let mut total_attempts = 0;
        let mut last_error = None;
        // T013（R-antibot-003）：anti-bot 检测命中后，后续 attempt 强制 needs_js=true，
        // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬挑战。
        // 仅 `antibot` 特性启用时 `force_needs_js` 会被改写；关闭时需抑制 unused_mut。
        #[cfg_attr(not(feature = "antibot"), allow(unused_mut))]
        let mut force_needs_js = false;

        // T028（R-identity-002）：智能重试基础设施
        // - RetryTracker：各 reason 独立计数与上限（AntiBot cap=2、FeatureToggle cap=3、total=5）
        // - UaPool：按 attempt 稳定轮换 UA（pick_seeded）
        // - last_reason：上次失败原因，用于计算下次 attempt 的 RetryDirective
        let mut tracker = RetryTracker::new_default();
        // 性能审查 H-1：复用结构体字段 ua_pool，避免每请求分配 44 个 profile
        let ua_pool = &self.ua_pool;
        let mut last_reason: Option<RetryReason> = None;

        for (score, engine) in candidates.into_iter().take(max_attempts) {
            total_attempts += 1;
            let engine_name = engine.name();

            // 记录引擎选择
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

            // T028（R-identity-002）：计算本次 attempt 的身份升级指令
            // - 首次尝试（total_attempts=1）用 default（无升级）
            // - 第 N 次重试（total_attempts=N+1, N>=1）基于上次失败 reason 升级
            //   directive_attempt = total_attempts - 2（0-indexed：0=首次重试、1=第二次重试...）
            let directive = if total_attempts == 1 {
                RetryDirective::default()
            } else {
                let directive_attempt = (total_attempts - 2) as u32;
                RetryDirective::for_attempt(
                    last_reason.unwrap_or(RetryReason::Transient),
                    directive_attempt,
                )
            };

            // T028：按 directive.rotate_ua 轮换 UA（C-1 修复：同步轮换所有指纹相关 header）
            //
            // C-1 问题：原实现仅覆盖 User-Agent，未同步轮换 Accept-Language 与 sec-ch-ua，
            //          导致重试时 UA 与 Accept-Language/sec-ch-ua 不一致（指纹矛盾），
            //          反爬服务可识别为「指纹不一致的爬虫」。
            //
            // C-1 修复：将 profile 的所有指纹相关 header 一次性写入：
            //   - User-Agent：所有 profile 必设
            //   - Accept-Language：所有 profile 必设
            //   - sec-ch-ua：仅 Chromium-based 浏览器非空时设置；为空时移除原值，
            //     避免残留 Firefox/Safari UA 但保留 Chromium sec-ch-ua 的矛盾。
            let mut headers = request.headers.clone();
            if directive.rotate_ua {
                let profile = ua_pool.pick_seeded((total_attempts - 1) as u64, request.mobile);
                headers.insert("User-Agent".to_string(), profile.ua.to_string());
                headers.insert(
                    "Accept-Language".to_string(),
                    profile.accept_language.to_string(),
                );
                if profile.sec_ch_ua.is_empty() {
                    headers.remove("sec-ch-ua");
                } else {
                    headers.insert("sec-ch-ua".to_string(), profile.sec_ch_ua.to_string());
                }
                debug!(
                    "Attempt {}: rotated UA via pick_seeded(seed={}) -> {} ({}, AL={}, sec-ch-ua={})",
                    total_attempts,
                    total_attempts - 1,
                    profile.ua,
                    if profile.mobile { "mobile" } else { "desktop" },
                    profile.accept_language,
                    if profile.sec_ch_ua.is_empty() {
                        "<none>"
                    } else {
                        profile.sec_ch_ua
                    }
                );
            }

            let attempt_request = InternalScrapeRequest {
                url: request.url.clone(),
                method: request.method,
                headers,
                timeout: remaining,
                // T013 + T028：force_needs_js（anti-bot）或 directive.force_browser 都强制 needs_js
                needs_js: request.needs_js || force_needs_js || directive.force_browser,
                needs_screenshot: request.needs_screenshot,
                screenshot_config: request.screenshot_config.clone(),
                mobile: request.mobile,
                proxy: request.proxy.clone(),
                skip_tls_verification: request.skip_tls_verification,
                needs_tls_fingerprint: request.needs_tls_fingerprint,
                use_fire_engine: request.use_fire_engine,
                actions: request.actions.clone(),
                body: request.body.clone(),
                sync_wait_ms: request.sync_wait_ms,
                block_ads: request.block_ads,
                block_media: request.block_media,
                session_id: request.session_id.clone(),
                wait_for: request.wait_for.clone(),
            };

            let engine_start = Instant::now();

            // T062（design.md §14）：瀑布式 MRT 超时包裹
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

                    // T013（R-antibot-003）：引擎返回"成功"响应后，检查是否为反爬挑战页。
                    // 命中 needs_browser 时将当前结果视为失败，强制后续 attempt needs_js=true，
                    // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬。
                    #[cfg(feature = "antibot")]
                    {
                        if let Some(detection) = check_antibot_response(&response, &request.url) {
                            if detection.needs_browser {
                                self.update_engine_stats(engine_name, false, response_time);
                                self.metrics
                                    .record_engine_failure(engine_name, &detection.reason);
                                warn!(
                                    "Engine {} returned anti-bot challenge ({:?}): {}, \
                                     forcing needs_js for subsequent attempts",
                                    engine_name, detection.tech, detection.reason
                                );
                                last_error = Some(EngineError::AntiBotDetected(detection.reason));
                                force_needs_js = true;

                                // T028（R-identity-002）：记录 AntiBot 失败到 tracker
                                let reason = RetryReason::AntiBot;
                                tracker.record(reason);
                                last_reason = Some(reason);

                                // T028：检查 tracker 上限（anti_bot cap=2）+ max_retries
                                // H-1 修复：使用 should_stop_after_retry_check 消除重复（DRY）
                                if self.should_stop_after_retry_check(
                                    &tracker,
                                    reason,
                                    total_attempts,
                                    max_retries,
                                ) {
                                    return Err(last_error.unwrap_or_else(|| {
                                        EngineError::AllEnginesFailed(
                                            "Max retries reached".to_string(),
                                        )
                                    }));
                                }
                                continue;
                            }
                        }
                    }

                    // T015（R-jsrender-001）：流式 HTTP→Chrome 升级探测
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

                            let mut js_request = request.clone();
                            js_request.needs_js = true;
                            return Box::pin(self.route_internal(&js_request)).await;
                        }
                    }

                    self.update_engine_stats(engine_name, true, response_time);
                    self.circuit_breaker.record_success(engine_name);

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

                    return Ok(response);
                }
                Ok(Err(e)) => {
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

                    // 记录失败指标
                    self.metrics
                        .record_engine_failure(engine_name, &e.to_string());

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name);

                        // T028（R-identity-002）：记录失败原因到 tracker（在 e move 之前取 reason）
                        let reason = e.retry_reason();
                        tracker.record(reason);
                        last_reason = Some(reason);

                        // T028：检查 tracker 上限（AntiBot cap=2、FeatureToggle cap=3、total=5）+ max_retries
                        // H-1 修复：使用 should_stop_after_retry_check 消除重复（DRY）
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
                    return Err(e);
                }
                Err(_elapsed) => {
                    // T062：MRT 瀑布式超时——`tokio::time::timeout` 在 effective_timeout 后
                    // 取消 engine.scrape() future，进入此分支。
                    //
                    // 架构审查 MEDIUM-2 修复：根据 effective_timeout 的来源区分两种语义：
                    // - `effective_timeout == remaining`（remaining <= engine_mrt）：
                    //   请求整体超时（剩余时间耗尽），返回 `EngineError::Timeout`。
                    //   此时即使切下一引擎也无力回天，由上层 route_sequential 循环下轮
                    //   的 `remaining.is_zero()` 检查兜底（L776）。
                    // - `effective_timeout == engine_mrt`（engine_mrt < remaining）：
                    //   真正的引擎 MRT 超时，返回 `EngineError::EngineMrtExceeded`，
                    //   router 触发瀑布式 fallback 切下一引擎继续。
                    //
                    // 边界情况 `remaining == engine_mrt`：按 Timeout 处理（保守语义，
                    // 避免误认为仍有剩余时间可 fallback）。
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

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

                    self.metrics
                        .record_engine_failure(engine_name, &timeout_err.to_string());

                    self.circuit_breaker.record_failure(engine_name);

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
