// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T035: 并发竞速路由 — 从 `EngineRouter` 拆分的 partial impl block
//!
//! 同时发起多个引擎请求，返回最快成功的结果。

use super::EngineRouter;
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use log::{debug, info, warn};
use metrics::{counter, histogram};
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
                        Err(e) => Err((engine_name, e)),
                    }
                });

            race_futures.push(race_future);
        }

        // 并发执行，返回最快成功的
        let timeout_duration = remaining.max(Duration::from_millis(100));

        // 使用 SelectAll 进行竞速
        let select_all_future = future::select_all(race_futures);

        match time::timeout(timeout_duration, select_all_future).await {
            Ok((result, _index, _others)) => {
                match result {
                    Ok((engine_name, response, response_time)) => {
                        self.update_engine_stats(&engine_name, true, response_time);
                        self.circuit_breaker.record_success(&engine_name);
                        self.metrics
                            .successful_requests
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics
                            .record_engine_latency(&engine_name, response_time);
                        self.metrics.record_engine_success(&engine_name);

                        // Phase 4a: Prometheus 指标埋点 (T062/T063)
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

                        // T070/§17：记录胜出引擎延迟到 Hedge 控制器，
                        // 为未来顺序路径提供 P84 阈值估算（接入 race 路径为可选增强）
                        self.hedge_controller.record_latency(response_time);

                        info!(
                            "Race mode: {} won in {:?}, total time: {:?}",
                            engine_name,
                            response_time,
                            start_time.elapsed()
                        );

                        // 取消其他正在进行的任务
                        Ok(response)
                    }
                    Err((engine_name, e)) => {
                        self.metrics
                            .record_engine_failure(&engine_name, &e.to_string());

                        // Phase 4a: Prometheus 指标埋点 (T062/T063)
                        counter!(
                            "crawlrs_engine_success_total",
                            "engine" => engine_name.clone(),
                            "result" => "failure"
                        )
                        .increment(1);

                        if e.is_retryable() {
                            self.circuit_breaker.record_failure(&engine_name);
                            Err(e)
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            Err(_) => {
                // 超时
                warn!(
                    "Race mode timed out after {:?} for request to {}",
                    timeout_duration, request.url
                );
                Err(EngineError::Timeout(timeout_duration))
            }
        }
    }
}
