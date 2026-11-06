// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 顺序路由·attempt 构造 — 从 `route_sequential.rs` 拆分
//!
//! - `compute_attempt_directive`：身份升级指令计算 + F5 停滞检测强制代理轮换
//! - `build_attempt_request`：F2 Session 亲和 / 盲轮换身份解析与 attempt 请求组装

use super::super::EngineRouter;
use crate::engines::engine_client::InternalScrapeRequest;
use crate::engines::session::SessionIdentity;
use crate::utils::retry::{RetryDirective, RetryReason};
use crate::utils::ua_pool::UaProfile;
use log::{debug, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

impl EngineRouter {
    /// 计算本次 attempt 的身份升级指令（route_internal 候选循环内提取）
    pub(super) fn compute_attempt_directive(
        &self,
        total_attempts: usize,
        last_reason: Option<RetryReason>,
        host: Option<&str>,
    ) -> RetryDirective {
        // 计算本次 attempt 的身份升级指令
        // - 首次尝试（total_attempts=1）用 default（无升级）
        // - 第 N 次重试（total_attempts=N+1, N>=1）基于上次失败 reason 升级
        // directive_attempt = total_attempts - 2（0-indexed：0=首次重试、1=第二次重试...）
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
        if let Some(host) = host {
            if self.host_discipline.is_stalled(host) && !directive.rotate_proxy {
                warn!(
                    "host {} stalled by consecutive rate limits, forcing proxy rotation \
                         (attempt {})",
                    host, total_attempts
                );
                directive.rotate_proxy = true;
            }
        }
        directive
    }

    /// 构造本次 attempt 的请求与身份（route_internal 候选循环内提取）：
    /// F2 Session 亲和 / 现状盲轮换二选一解析身份，返回
    /// `(attempt_request, session_identity)`。UA 轮换复用结构体字段
    /// ua_pool，避免每请求分配 44 个 profile。
    pub(super) fn build_attempt_request(
        &self,
        request: &InternalScrapeRequest,
        host: Option<&str>,
        directive: RetryDirective,
        total_attempts: usize,
        force_needs_js: bool,
        remaining: Duration,
    ) -> (InternalScrapeRequest, Option<Arc<SessionIdentity>>) {
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
        // 三件 header 一次性同步写入（两路径同规则），写入规则见
        // [`apply_ua_profile_headers`] 文档。
        let mut headers = request.headers.clone();
        let mut attempt_proxy = request.proxy.clone();
        let session_identity: Option<Arc<SessionIdentity>> = if self.session.is_enabled() {
            let host_key = host.unwrap_or("");
            if directive.rotate_ua || self.host_discipline.is_stalled(host_key) {
                // 身份升级：强制退休当前亲和身份，本次 acquire 换新
                self.session.retire(host_key, request.session_id.as_deref());
            }
            let identity =
                self.session
                    .acquire(host_key, request.session_id.as_deref(), request.mobile);
            let profile = identity.ua_profile(self.session.ua_pool());
            apply_ua_profile_headers(&mut headers, profile);
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
            let profile = self
                .ua_pool
                .pick_seeded((total_attempts - 1) as u64, request.mobile);
            apply_ua_profile_headers(&mut headers, profile);
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
        (attempt_request, session_identity)
    }
}

/// UA profile 三件 header 一次性同步写入（session 亲和与 pick_seeded 盲轮换
/// 两路径同规则）：
/// - `User-Agent` / `Accept-Language`：所有 profile 必设
/// - `sec-ch-ua`：仅 Chromium-based 非空时设置；为空时移除原值，
///   避免残留 Firefox/Safari UA 但保留 Chromium sec-ch-ua 的矛盾。
fn apply_ua_profile_headers(headers: &mut HashMap<String, String>, profile: &UaProfile) {
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
}
