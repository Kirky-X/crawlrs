// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由·host 提示与空候选降级 — 从 `route_sequential.rs` 拆分
//!
//! - `apply_host_hints`：host 解析 + F7 反爬记忆提示 + host 纪律 gate
//! - `handle_no_candidates`：内存水位 Critical 降级语义（engine-moli）

use super::super::EngineRouter;
use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use log::debug;
#[cfg(feature = "engine-moli")]
use log::warn;
#[cfg(feature = "engine-moli")]
use std::sync::atomic::Ordering;

impl EngineRouter {
    /// F7：host 反爬记忆——TTL 内检出过挑战页的 host 直接按 needs_js
    /// 路由（HTTP 引擎不再每次撞墙），过期后恢复候选重探。
    /// host 纪律叠加：tls_suspect 的 host 附加 needs_tls_fingerprint。
    pub(super) async fn apply_host_hints(
        &self,
        request: &InternalScrapeRequest,
    ) -> (Option<String>, InternalScrapeRequest) {
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

        (host, request)
    }

    /// 内存水位 Critical 降级语义（engine-moli）：needs_js 请求的浏览器系引擎
    /// 已被水位归 0 挤出候选且 moli 不可用。fallback_to_static=true 时改走
    /// ReqwestEngine 并标记 js_degraded；否则快速失败 MemoryConstrained。
    ///
    /// 返回值语义：`Some(Ok(resp))` = 静态降级成功；`Some(Err)` = Critical 且
    /// 无静态 fallback（或降级失败）；`None` = 无特殊语义，调用方继续走
    /// "No suitable engines available" 常规失败路径。原 `map_err(...)?` 的
    /// `?` 传播在此改写为显式 match 分支（`Err` → `Some(Err(...))`），
    /// 最终仍由调用方 `return`，控制流与消息逐字等价。
    #[cfg_attr(
        not(feature = "engine-moli"),
        allow(clippy::unused_async, unused_variables)
    )]
    pub(super) async fn handle_no_candidates(
        &self,
        request: &InternalScrapeRequest,
    ) -> Option<Result<InternalScrapeResponse, EngineError>> {
        #[cfg(feature = "engine-moli")]
        if let Some(outcome) = self.try_static_memory_fallback(request).await {
            return Some(outcome);
        }
        None
    }

    /// 内存水位 Critical 且无 JS 引擎时的静态引擎降级
    /// （自 [`handle_no_candidates`] 提取；控制流与日志逐字保留）。
    #[cfg(feature = "engine-moli")]
    async fn try_static_memory_fallback(
        &self,
        request: &InternalScrapeRequest,
    ) -> Option<Result<InternalScrapeResponse, EngineError>> {
        if !request.needs_js {
            return None;
        }
        let watermark = self.memory.as_ref()?;
        if watermark.level() != crate::engines::memory_watermark::MemoryLevel::Critical {
            return None;
        }
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        if self.memory_fallback_to_static {
            if let Some(static_engine) = self.engines.iter().find(|e| e.name() == "reqwest") {
                warn!(
                    "memory Critical without JS engines, degrading request to static engine (js_degraded): {}",
                    request.url
                );
                let mut static_request = request.clone();
                static_request.needs_js = false;
                let fallback_started = std::time::Instant::now();
                let mut response = match static_engine.scrape(&static_request).await {
                    Ok(resp) => resp,
                    Err(e) => {
                        return Some(Err(EngineError::AllEnginesFailed(format!(
                            "static fallback after memory degradation failed: {e}"
                        ))))
                    }
                };
                response.js_degraded = true;
                // 降级旁路与候选循环同款簿记：引擎统计/熔断/指标不因
                // 旁路执行而失真（否则 reqwest 健康面板漏记这些请求）
                let engine_name = static_engine.name();
                let response_time = fallback_started.elapsed();
                self.update_engine_stats(engine_name, true, response_time);
                self.circuit_breaker.record_success(engine_name).await;
                self.metrics
                    .record_engine_latency(engine_name, response_time);
                self.metrics.record_engine_success(engine_name);
                self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .successful_requests
                    .fetch_add(1, Ordering::Relaxed);
                return Some(Ok(response));
            }
        }
        Some(Err(EngineError::MemoryConstrained(
            "no JS engine available under Critical memory watermark".to_string(),
        )))
    }
}
