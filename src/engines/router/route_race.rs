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

impl EngineRouter {
    /// 并发竞速模式：同时发起多个引擎请求，返回最快成功的那个
    pub(super) async fn route_race_mode(
        &self,
        request: &InternalScrapeRequest,
        candidates: Vec<(f64, Arc<dyn ScraperEngine>)>,
        start_time: Instant,
    ) -> Result<InternalScrapeResponse, EngineError> {
        use futures::future;
        use tokio::time;

        let remaining = request
            .timeout
            .checked_sub(start_time.elapsed())
            .unwrap_or(Duration::from_millis(0));

        if remaining.is_zero() {
            return Err(EngineError::Timeout(request.timeout));
        }

        // 限制竞速引擎数量
        let race_candidates: Vec<_> = candidates.into_iter().take(3).collect();

        debug!(
            "Race mode: launching {} engines concurrently for {}",
            race_candidates.len(),
            request.url
        );

        // 创建竞速任务 (使用 Box::pin 解决 Unpin 问题)
        let mut race_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>> =
            Vec::new();

        for (_score, engine) in race_candidates {
            let engine_name = engine.name().to_string();
            let engine_clone = engine.clone();

            let remaining_clone = remaining;
            let request_clone = InternalScrapeRequest {
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
            };

            let race_future: std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>> =
                Box::pin(async move {
                    let engine_start = Instant::now();
                    match engine_clone.scrape(&request_clone).await {
                        Ok(response) => Ok((engine_name, response, engine_start.elapsed())),
                        Err(e) => Err((engine_name, e, engine_start.elapsed())),
                    }
                });

            race_futures.push(race_future);
        }

        // 并发执行：循环 select_all，最快「成功」者胜出。
        // 失败的 future 从竞速集中移除并记录指标后继续等待其余候选，
        // 全部失败或整体超时才返回错误（与模块文档语义一致）。
        let timeout_duration = remaining.max(Duration::from_millis(100));
        let deadline = time::Instant::now() + timeout_duration;
        let total_candidates = race_futures.len();

        let mut pending = race_futures;
        let mut last_error: Option<EngineError> = None;

        loop {
            if pending.is_empty() {
                warn!(
                    "Race mode: all {} candidates failed for {}",
                    total_candidates, request.url
                );
                // 复查：全候选被挑战页作废且请求未要求 JS 时，按顺序模式
                // 语义以 needs_js=true 重派一轮浏览器引擎（升级链对齐）
                let antibot_detected = matches!(last_error, Some(EngineError::AntiBotDetected(_)));
                if antibot_detected && !request.needs_js {
                    let mut js_request = request.clone();
                    js_request.needs_js = true;
                    info!(
                        "Race mode: all candidates hit anti-bot, retrying once with needs_js=true"
                    );
                    // Box::pin 打断 route_internal→race→route_internal 的
                    // async 递归环
                    return Box::pin(self.route_internal(&js_request)).await;
                }
                return Err(last_error.unwrap_or(EngineError::AllEnginesFailed(
                    "race mode: all engines failed".to_string(),
                )));
            }

            if time::Instant::now() >= deadline {
                warn!(
                    "Race mode timed out after {:?} for request to {}",
                    timeout_duration, request.url
                );
                // 复查 L1：已有 AntiBotDetected 诊断时优先返回真实原因
                if let Some(e) = last_error {
                    return Err(e);
                }
                return Err(EngineError::Timeout(timeout_duration));
            }

            match time::timeout_at(deadline, future::select_all(pending)).await {
                Ok((result, _index, others)) => {
                    pending = others;
                    match result {
                        Ok((engine_name, response, response_time)) => {
                            // 胜出响应先过反爬分类：最快的引擎若返回的是挑战页
                            // （200 + Just a moment 等），作废该结果并继续竞速剩余候选，
                            // 与顺序模式的防护语义对齐。检查必须在成功统计之前。
                            #[cfg(feature = "content")]
                            {
                                if let Some(detection) =
                                    check_antibot_response(&response, &request.url)
                                {
                                    if detection.needs_browser {
                                        // 口径与顺序模式一致：挑战页丢弃只记
                                        // engine_stats/engine_failure，不计全局
                                        // total/failed 计数（挑战是站点行为非引擎故障）
                                        self.update_engine_stats(
                                            &engine_name,
                                            false,
                                            response_time,
                                        );
                                        self.metrics
                                            .record_engine_failure(&engine_name, &detection.reason);
                                        warn!(
                                            "Race mode: {} returned anti-bot challenge ({:?}): {}, \
                                             discarding and racing remaining candidates",
                                            engine_name, detection.tech, detection.reason
                                        );
                                        last_error =
                                            Some(EngineError::AntiBotDetected(detection.reason));
                                        continue;
                                    }
                                }
                            }

                            self.update_engine_stats(&engine_name, true, response_time);
                            self.circuit_breaker.record_success(&engine_name);
                            self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                            self.metrics
                                .successful_requests
                                .fetch_add(1, Ordering::Relaxed);
                            self.metrics
                                .record_engine_latency(&engine_name, response_time);
                            self.metrics.record_engine_success(&engine_name);

                            // Prometheus 指标埋点
                            counter!(
                                "crawlrs_engine_success_total",
                                "engine" => engine_name.clone(),
                                "result" => "success"
                            )
                            .increment(1);
                            histogram!(
                                "crawlrs_engine_duration_seconds",
                                "engine" => engine_name.clone()
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

                            // 取消其他正在进行的任务
                            return Ok(response);
                        }
                        Err((engine_name, e, response_time)) => {
                            // 指标口径与顺序模式对齐：total/failed 同步累计
                            self.update_engine_stats(&engine_name, false, response_time);
                            self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
                            self.metrics
                                .record_engine_failure(&engine_name, &e.to_string());

                            // Prometheus 指标埋点
                            counter!(
                                "crawlrs_engine_success_total",
                                "engine" => engine_name.clone(),
                                "result" => "failure"
                            )
                            .increment(1);

                            debug!(
                                "Race mode: {} failed ({}) in {:?}, continuing with remaining candidates",
                                engine_name, e, response_time
                            );

                            if e.is_retryable() {
                                self.circuit_breaker.record_failure(&engine_name);
                            }
                            last_error = Some(e);
                            // 继续竞速剩余候选
                        }
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
}
