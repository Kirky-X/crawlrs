// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 并发竞速路由 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 同时发起多个引擎请求，返回最快成功的结果。

#[cfg(feature = "content")]
use super::check_antibot_response;
use super::EngineRouter;
use crate::common::metrics_shim::{counter, histogram};
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use log::{debug, info, warn};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 竞速单个引擎 future 的统一输出：
/// `Ok((引擎名, 响应, 引擎耗时))` / `Err((引擎名, 错误, 引擎耗时))`
type RaceCompletion =
    Result<(String, InternalScrapeResponse, Duration), (String, EngineError, Duration)>;

type RaceFuture = std::pin::Pin<Box<dyn std::future::Future<Output = RaceCompletion> + Send>>;

impl EngineRouter {
    /// 并发竞速模式：同时发起多个引擎请求，返回最快成功的那个
    pub(super) async fn route_race_mode(
        &self,
        request: &InternalScrapeRequest,
        candidates: Vec<(f64, Arc<dyn ScraperEngine>)>,
        start_time: Instant,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let remaining = request
            .timeout
            .checked_sub(start_time.elapsed())
            .unwrap_or(Duration::from_millis(0));

        if remaining.is_zero() {
            return Err(EngineError::Timeout(request.timeout));
        }

        let pending = build_race_pool(request, candidates, remaining);
        self.race_until_win(request, pending, remaining, start_time)
            .await
    }

    /// 竞速选择循环（自 [`EngineRouter::route_race_mode`] 提取）
    ///
    /// 并发执行已构建的竞速 future：循环 select_all，最快「成功」者胜出。
    /// 失败的 future 从竞速集中移除并记录指标后继续等待其余候选，
    /// 全部失败或整体超时才返回错误（与模块文档语义一致）。
    async fn race_until_win(
        &self,
        request: &InternalScrapeRequest,
        mut pending: Vec<RaceFuture>,
        remaining: Duration,
        start_time: Instant,
    ) -> Result<InternalScrapeResponse, EngineError> {
        use futures::future;
        use tokio::time;

        let timeout_duration = remaining.max(Duration::from_millis(100));
        let deadline = time::Instant::now() + timeout_duration;
        let total_candidates = pending.len();

        let mut last_error: Option<EngineError> = None;

        loop {
            if pending.is_empty() {
                return self
                    .finish_race_all_failed(request, total_candidates, last_error)
                    .await;
            }

            if time::Instant::now() >= deadline {
                warn!(
                    "Race mode timed out after {:?} for request to {}",
                    timeout_duration, request.url
                );
                // 已有 AntiBotDetected 诊断时优先返回真实原因
                if let Some(e) = last_error {
                    return Err(e);
                }
                return Err(EngineError::Timeout(timeout_duration));
            }

            match time::timeout_at(deadline, future::select_all(pending)).await {
                Ok((result, _index, others)) => {
                    pending = others;
                    if let Some(outcome) = self
                        .handle_race_completion(request, result, start_time, &mut last_error)
                        .await
                    {
                        return outcome;
                    }
                }
                Err(_) => {
                    // 超时
                    warn!(
                        "Race mode timed out after {:?} for request to {}",
                        timeout_duration, request.url
                    );
                    return Err(EngineError::Timeout(timeout_duration));
                }
            }
        }
    }

    /// 处理单个竞速 future 的完成结果（自 [`EngineRouter::race_until_win`] 提取）。
    ///
    /// 成功响应先过反爬分类，挑战页作废并返回 `None`（继续竞速）；
    /// 失败记录指标、更新 `last_error` 后返回 `None`。返回 `Some` 表示
    /// 竞速已有最终结果，调用方直接返回。
    #[cfg_attr(not(feature = "content"), allow(unused_variables))]
    async fn handle_race_completion(
        &self,
        request: &InternalScrapeRequest,
        result: RaceCompletion,
        start_time: Instant,
        last_error: &mut Option<EngineError>,
    ) -> Option<Result<InternalScrapeResponse, EngineError>> {
        match result {
            Ok((engine_name, response, response_time)) => {
                // 胜出响应先过反爬分类：最快的引擎若返回的是挑战页
                // （200 + Just a moment 等），作废该结果并继续竞速剩余候选，
                // 与顺序模式的防护语义对齐。检查必须在成功统计之前。
                #[cfg(feature = "content")]
                if let Some(reason) = self.discard_race_antibot_challenge(
                    &engine_name,
                    &request.url,
                    &response,
                    response_time,
                ) {
                    *last_error = Some(EngineError::AntiBotDetected(reason));
                    return None;
                }

                self.record_race_win(&engine_name, response_time, start_time)
                    .await;

                // 取消其他正在进行的任务
                Some(Ok(response))
            }
            Err((engine_name, e, response_time)) => {
                self.record_race_failure(&engine_name, &e, response_time)
                    .await;
                *last_error = Some(e);
                // 继续竞速剩余候选
                None
            }
        }
    }

    /// 全候选竞速失败后的终局处理（自 [`EngineRouter::route_race_mode`] 提取）
    ///
    /// 全候选被挑战页作废且请求未要求 JS 时，按顺序模式
    /// 语义以 needs_js=true 重派一轮浏览器引擎（升级链对齐）；
    /// 否则返回最后一个错误。
    async fn finish_race_all_failed(
        &self,
        request: &InternalScrapeRequest,
        total_candidates: usize,
        last_error: Option<EngineError>,
    ) -> Result<InternalScrapeResponse, EngineError> {
        warn!(
            "Race mode: all {} candidates failed for {}",
            total_candidates, request.url
        );
        let antibot_detected = matches!(last_error, Some(EngineError::AntiBotDetected(_)));
        if antibot_detected && !request.needs_js {
            let mut js_request = request.clone();
            js_request.needs_js = true;
            info!("Race mode: all candidates hit anti-bot, retrying once with needs_js=true");
            // Box::pin 打断 route_internal→race→route_internal 的
            // async 递归环
            return Box::pin(self.route_internal(&js_request)).await;
        }
        Err(last_error.unwrap_or(EngineError::AllEnginesFailed(
            "race mode: all engines failed".to_string(),
        )))
    }

    /// 胜出响应反爬分类（自 [`EngineRouter::route_race_mode`] 提取）
    ///
    /// 最快的引擎若返回挑战页（200 + Just a moment 等），作废该结果并继续
    /// 竞速剩余候选，与顺序模式的防护语义对齐。检查必须在成功统计之前。
    /// 口径与顺序模式一致：挑战页丢弃只记 engine_stats/engine_failure，不计
    /// 全局 total/failed 计数（挑战是站点行为非引擎故障）。
    ///
    /// 返回 `Some(reason)` 表示结果已作废（调用方以
    /// `EngineError::AntiBotDetected(reason)` 更新 last_error 后继续竞速）。
    #[cfg(feature = "content")]
    fn discard_race_antibot_challenge(
        &self,
        engine_name: &str,
        request_url: &str,
        response: &InternalScrapeResponse,
        response_time: Duration,
    ) -> Option<String> {
        if let Some(detection) = check_antibot_response(response, request_url) {
            if detection.needs_browser {
                self.update_engine_stats(engine_name, false, response_time);
                self.metrics
                    .record_engine_failure(engine_name, &detection.reason);
                warn!(
                    "Race mode: {} returned anti-bot challenge ({:?}): {}, \
                     discarding and racing remaining candidates",
                    engine_name, detection.tech, detection.reason
                );
                return Some(detection.reason);
            }
        }
        None
    }

    /// 胜出路径记账（自 [`EngineRouter::route_race_mode`] 提取）：引擎统计、
    /// 熔断、指标、Prometheus 埋点、Hedge 延迟记录与胜利日志，顺序逐字保留。
    async fn record_race_win(
        &self,
        engine_name: &str,
        response_time: Duration,
        start_time: Instant,
    ) {
        self.update_engine_stats(engine_name, true, response_time);
        self.circuit_breaker.record_success(engine_name).await;
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .successful_requests
            .fetch_add(1, Ordering::Relaxed);
        self.metrics
            .record_engine_latency(engine_name, response_time);
        self.metrics.record_engine_success(engine_name);

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

        // §17：记录胜出引擎延迟到 Hedge 控制器，
        // 为未来顺序路径提供 P84 阈值估算（接入 race 路径为可选增强）
        self.hedge_controller.record_latency(response_time);

        info!(
            "Race mode: {} won in {:?}, total time: {:?}",
            engine_name,
            response_time,
            start_time.elapsed()
        );
    }

    /// 失败路径记账（自 [`EngineRouter::route_race_mode`] 提取）：指标口径
    /// 与顺序模式对齐（total/failed 同步累计）；仅可重试错误计入熔断失败。
    async fn record_race_failure(
        &self,
        engine_name: &str,
        e: &EngineError,
        response_time: Duration,
    ) {
        // 指标口径与顺序模式对齐：total/failed 同步累计
        self.update_engine_stats(engine_name, false, response_time);
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .record_engine_failure(engine_name, &e.to_string());

        // Prometheus 指标埋点
        counter!(
            "crawlrs_engine_success_total",
            "engine" => engine_name.to_string(),
            "result" => "failure"
        )
        .increment(1);

        debug!(
            "Race mode: {} failed ({}) in {:?}, continuing with remaining candidates",
            engine_name, e, response_time
        );

        if e.is_retryable() {
            self.circuit_breaker.record_failure(engine_name).await;
        }
    }
}

/// 复制请求并把 timeout 替换为竞速剩余时间
/// （自 [`EngineRouter::route_race_mode`] 提取）
fn clone_race_request(
    request: &InternalScrapeRequest,
    remaining: Duration,
) -> InternalScrapeRequest {
    let remaining_clone = remaining;
    InternalScrapeRequest {
        url: request.url.clone(),
        method: request.method,
        headers: request.headers.clone(),
        timeout: remaining_clone,
        needs_js: request.needs_js,
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
        needs_mllm: request.needs_mllm,
    }
}

/// 构建竞速 future 集合（自 [`EngineRouter::route_race_mode`] 提取）：
/// 限制竞速引擎数量并复制请求（timeout 替换为竞速剩余时间）。
/// 使用 Box::pin 解决 Unpin 问题。
fn build_race_pool(
    request: &InternalScrapeRequest,
    candidates: Vec<(f64, Arc<dyn ScraperEngine>)>,
    remaining: Duration,
) -> Vec<RaceFuture> {
    // 限制竞速引擎数量
    let race_candidates: Vec<_> = candidates.into_iter().take(3).collect();

    debug!(
        "Race mode: launching {} engines concurrently for {}",
        race_candidates.len(),
        request.url
    );

    // 创建竞速任务 (使用 Box::pin 解决 Unpin 问题)
    let mut race_futures: Vec<RaceFuture> = Vec::new();

    for (_score, engine) in race_candidates {
        race_futures.push(build_race_future(
            &engine,
            clone_race_request(request, remaining),
        ));
    }

    race_futures
}

/// 构建单个引擎的竞速 future（自 [`EngineRouter::route_race_mode`] 提取）。
/// Box::pin 解决 Unpin 问题。
fn build_race_future(
    engine: &Arc<dyn ScraperEngine>,
    request_clone: InternalScrapeRequest,
) -> RaceFuture {
    let engine_name = engine.name().to_string();
    let engine_clone = engine.clone();

    Box::pin(async move {
        let engine_start = Instant::now();
        match engine_clone.scrape(&request_clone).await {
            Ok(response) => Ok((engine_name, response, engine_start.elapsed())),
            Err(e) => Err((engine_name, e, engine_start.elapsed())),
        }
    })
}
