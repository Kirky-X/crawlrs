// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 包含传统顺序模式的路由实现（带重试、身份升级、MRT 瀑布式 fallback）。

#[cfg(feature = "content")]
use super::check_antibot_response;
use super::{check_js_upgrade_probe, EngineRouter, LoadBalancingStrategy};
use crate::common::metrics_shim::{counter, histogram};
use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use crate::engines::session::SessionIdentity;
use crate::utils::host_discipline::ObserveOutcome;
use crate::utils::retry::{RetryDirective, RetryReason, RetryTracker};
use log::{debug, info, warn};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 已知 WAF 变体：403 命中任一即为带 challenge 的拦截页，
/// 只有"无 WAF 指纹的裸 403"才计入 host 纪律连击
#[cfg(feature = "content")]
fn is_known_waf(tech: crate::engines::antibot::AntiBotTech) -> bool {
    use crate::engines::antibot::AntiBotTech;
    matches!(
        tech,
        AntiBotTech::Cloudflare
            | AntiBotTech::Akamai
            | AntiBotTech::PerimeterX
            | AntiBotTech::DataDome
            | AntiBotTech::Imperva
            | AntiBotTech::Sucuri
            | AntiBotTech::Kasada
            | AntiBotTech::AwsWaf
    )
}

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
    /// F4：失败现场快照（旁路观测，永不失败）
    ///
    /// 快照器未配置（`failure_snapshot_dir` 空串）时 `Option` 短路零成本；
    /// 内部 IO 失败已在 [`FailureSnapshotter::snapshot`](crate::infrastructure::observability::failure_snapshot::FailureSnapshotter::snapshot)
    /// 内 `log::debug` 静默降级，绝不影响抓取主流程。
    /// 仅透传引擎已产出的 screenshot（不额外触发截图）。
    #[cfg(feature = "content")]
    pub(super) async fn snapshot_failure(
        &self,
        url: &str,
        status_code: u16,
        reason: &str,
        response: &InternalScrapeResponse,
    ) {
        if let Some(snapshotter) = &self.failure_snapshot {
            snapshotter
                .snapshot(
                    crate::infrastructure::observability::failure_snapshot::SnapshotInput {
                        url,
                        status_code,
                        reason,
                        content: &response.content,
                        headers: &response.headers,
                        screenshot: response.screenshot.as_deref(),
                    },
                )
                .await;
        }
    }

    /// Internal route implementation without timeout
    #[allow(unused_variables)]
    pub(super) async fn route_internal(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let start_time = Instant::now();

        // F7：host 反爬记忆——TTL 内检出过挑战页的 host 直接按 needs_js
        // 路由（HTTP 引擎不再每次撞墙），过期后恢复候选重探。
        // host 纪律叠加：tls_suspect 的 host 附加 needs_tls_fingerprint。
        let host = url::Url::parse(&request.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string));

        let request = if let Some(host) = &host {
            let needs_js_hint = self.host_needs_browser(host);
            let tls_suspect = self.host_discipline.tls_suspect(host);
            if needs_js_hint {
                debug!(
                    "host {} has anti-bot hint, routing with needs_js=true",
                    host
                );
            }
            if needs_js_hint || tls_suspect {
                let mut hinted = request.clone();
                hinted.needs_js |= needs_js_hint;
                hinted.needs_tls_fingerprint |= tls_suspect;
                hinted
            } else {
                request.clone()
            }
        } else {
            request.clone()
        };

        // per-host 纪律 gate：礼貌限速 + 429 冷却（单次等待钳制 ≤5s）
        if let Some(host) = &host {
            self.host_discipline.gate(host).await;
        }

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(&request);

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
        // 复用结构体字段 ua_pool，避免每请求分配 44 个 profile
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

            // 计算本次 attempt 的身份升级指令
            // - 首次尝试（total_attempts=1）用 default（无升级）
            // - 第 N 次重试（total_attempts=N+1, N>=1）基于上次失败 reason 升级
            //   directive_attempt = total_attempts - 2（0-indexed：0=首次重试、1=第二次重试...）
            let mut directive = if total_attempts == 1 {
                RetryDirective::default()
            } else {
                let directive_attempt = (total_attempts - 2) as u32;
                RetryDirective::for_attempt(
                    last_reason.unwrap_or(RetryReason::Transient),
                    directive_attempt,
                )
            };

            // F5 停滞检测：连续 RateLimited 达阈值的 host，本次 attempt 强制
            // 轮换代理（沿用 RetryDirective 身份升级模式）。仅置位指令并上报，
            // 代理的实际切换由引擎/代理层消费该字段；Session 退休由 F2 接入。
            if let Some(host) = &host {
                if self.host_discipline.is_stalled(host) && !directive.rotate_proxy {
                    warn!(
                        "host {} stalled by consecutive rate limits, forcing proxy rotation \
                         (attempt {})",
                        host, total_attempts
                    );
                    directive.rotate_proxy = true;
                }
            }

            // F2 Session 身份三件套 / 现状盲轮换二选一：
            //
            // - session.enabled=true：按 (host, session_id) 亲和 acquire 身份，
            //   三件指纹 header 从身份 UA profile 取（profile 内部 UA 与
            //   Accept-Language/sec-ch-ua/tls_emulation 绑定一致），代理 =
            //   身份代理（请求显式代理兜底），Cookie 手动通道注入。
            //   directive.rotate_ua 与 A 簇 is_stalled 的「身份升级」语义
            //   改为退休当前身份 → 本次 acquire 直接换新（取代按 attempt 的
            //   pick_seeded 盲轮换）。
            // - session.enabled=false（默认）：走 pick_seeded 盲轮换分支，
            //   行为与引入 F2 之前完全一致。
            //
            // 三件 header 一次性同步写入（两路径同规则）：
            //   - User-Agent / Accept-Language：所有 profile 必设
            //   - sec-ch-ua：仅 Chromium-based 非空时设置；为空时移除原值，
            //     避免残留 Firefox/Safari UA 但保留 Chromium sec-ch-ua 的矛盾。
            let mut headers = request.headers.clone();
            let mut attempt_proxy = request.proxy.clone();
            let session_identity: Option<Arc<SessionIdentity>> = if self.session.is_enabled() {
                let host_key = host.as_deref().unwrap_or("");
                if directive.rotate_ua || self.host_discipline.is_stalled(host_key) {
                    // 身份升级：强制退休当前亲和身份，本次 acquire 换新
                    self.session.retire(host_key, request.session_id.as_deref());
                }
                let identity =
                    self.session
                        .acquire(host_key, request.session_id.as_deref(), request.mobile);
                let profile = identity.ua_profile(self.session.ua_pool());
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
                if let Some(cookie) = self.session.cookie_header(&identity, host_key) {
                    headers.insert("Cookie".to_string(), cookie);
                }
                attempt_proxy = identity.proxy_url.clone().or(request.proxy.clone());
                debug!(
                    "Attempt {}: session identity #{} (proxy={}, ua={}, AL={}, sec-ch-ua={})",
                    total_attempts,
                    identity.id,
                    identity
                        .proxy_url
                        .as_deref()
                        .map(crate::utils::proxy::redact_proxy_url)
                        .unwrap_or_else(|| "<none>".to_string()),
                    profile.ua,
                    profile.accept_language,
                    if profile.sec_ch_ua.is_empty() {
                        "<none>"
                    } else {
                        profile.sec_ch_ua
                    }
                );
                Some(identity)
            } else if directive.rotate_ua {
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
                None
            } else {
                None
            };

            let attempt_request = InternalScrapeRequest {
                url: request.url.clone(),
                method: request.method,
                headers,
                timeout: remaining,
                // force_needs_js（anti-bot）或 directive.force_browser 都强制 needs_js
                needs_js: request.needs_js || force_needs_js || directive.force_browser,
                needs_screenshot: request.needs_screenshot,
                screenshot_config: request.screenshot_config.clone(),
                mobile: request.mobile,
                // F2：session 启用时为身份代理（请求显式代理兜底）；禁用时原样透传
                proxy: attempt_proxy,
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
                needs_mllm: request.needs_mllm,
            };

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

                    // 引擎返回"成功"响应后，检查是否为反爬挑战页。
                    // 命中 needs_browser 时将当前结果视为失败，强制后续 attempt needs_js=true，
                    // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬。
                    #[cfg(feature = "content")]
                    {
                        if let Some(detection) = check_antibot_response(&response, &request.url) {
                            if detection.needs_browser {
                                // F7：记录 host 反爬记忆，TTL 内后续同 host
                                // 请求直接按 needs_js 路由
                                if let Ok(u) = url::Url::parse(&request.url) {
                                    if let Some(host) = u.host_str() {
                                        self.record_host_browser_hint(host);
                                        // host 纪律观测：429 → 冷却；
                                        // 裸 403（无 WAF 指纹）→ 连击计数
                                        if detection.tech
                                            == crate::engines::antibot::AntiBotTech::RateLimited
                                        {
                                            self.host_discipline.observe(
                                                host,
                                                ObserveOutcome::RateLimited {
                                                    retry_after: detection
                                                        .retry_after_secs
                                                        .map(Duration::from_secs),
                                                },
                                            );
                                        } else if response.status_code == 403
                                            && !is_known_waf(detection.tech)
                                        {
                                            self.host_discipline.observe(
                                                host,
                                                ObserveOutcome::Forbidden { challenge: false },
                                            );
                                        }
                                    }
                                }
                                self.update_engine_stats(engine_name, false, response_time);
                                // F2：挑战页 = 身份可疑证据，同址 mark_bad
                                if let Some(identity) = &session_identity {
                                    self.session.mark_bad(identity);
                                }
                                self.metrics
                                    .record_engine_failure(engine_name, &detection.reason);
                                // F4：失败现场快照（签名键去重存档，旁路观测；
                                // 内部 spawn_blocking 落盘，不阻塞 worker 线程）
                                self.snapshot_failure(
                                    &request.url,
                                    response.status_code,
                                    &detection.reason,
                                    &response,
                                )
                                .await;
                                warn!(
                                    "Engine {} returned anti-bot challenge ({:?}): {}, \
                                     forcing needs_js for subsequent attempts",
                                    engine_name, detection.tech, detection.reason
                                );
                                last_error = Some(EngineError::AntiBotDetected(detection.reason));
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
                                        EngineError::AllEnginesFailed(
                                            "Max retries reached".to_string(),
                                        )
                                    }));
                                }
                                continue;
                            }
                        }
                    }

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
                    if let Some(host) = &host {
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
                    if let Some(identity) = &session_identity {
                        if (200..300).contains(&response.status_code) {
                            self.session.mark_good(identity);
                        } else if matches!(response.status_code, 401 | 403 | 429) {
                            self.session.mark_bad(identity);
                        }
                        self.session.capture_cookies(
                            identity,
                            host.as_deref().unwrap_or(""),
                            &response.headers,
                        );
                    }

                    return Ok(response);
                }
                Ok(Err(e)) => {
                    let response_time = engine_start.elapsed();
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
                        .record_engine_failure(engine_name, &e.to_string());

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name);

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

#[cfg(all(test, feature = "content"))]
mod snapshot_failure_tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;

    fn sample_response() -> InternalScrapeResponse {
        InternalScrapeResponse {
            captured_responses: Vec::new(),
            status_code: 403,
            content: "<html>challenge</html>".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 42,
            final_url: None,
        }
    }

    fn dir_file_count(dir: &std::path::Path) -> usize {
        fs::read_dir(dir).expect("read_dir").count()
    }

    /// 快照器为 None（默认未配置）时调用点零开销短路（不 panic、无落盘）
    #[tokio::test]
    async fn snapshot_failure_none_short_circuits_without_panic() {
        let router = EngineRouter::new(Vec::new());
        assert!(router.failure_snapshot.is_none(), "default must be None");
        router
            .snapshot_failure("https://x.com/a", 403, "waf", &sample_response())
            .await;
    }

    /// 注入快照器后 snapshot_failure 落盘 .html + .meta.json
    #[tokio::test]
    async fn snapshot_failure_with_snapshotter_writes_archive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("snapshots");
        let mut router = EngineRouter::new(Vec::new());
        router.set_failure_snapshotter(Some(Arc::new(
            crate::infrastructure::observability::failure_snapshot::FailureSnapshotter::new(
                dir.clone(),
                200,
            ),
        )));
        router
            .snapshot_failure("https://x.com/a", 403, "waf", &sample_response())
            .await;
        assert_eq!(dir_file_count(&dir), 2, "html + meta.json");
    }
}
