// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Playwright 浏览器会话执行体 — 从 `playwright.rs` 拆分
//!
//! 前置校验（GET / SSRF / JS 或截图请求）与超时包裹内的会话编排。

use super::interception::setup_request_interception;
use super::{
    acquire_fingerprinted_page, collect_and_build_response, navigate_and_wait, InterceptGuard,
    PlaywrightEngine, XhrCaptureSession,
};
use crate::engines::client::playwright_pool::BrowserPool;
use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use crate::engines::validators;
use std::time::Instant;

/// 抓取前置校验（自 [`PlaywrightEngine::scrape`] 提取）：仅支持 GET，
/// 目标 URL 必须通过 SSRF 校验，且请求必须要求 JS 渲染或截图。
pub(super) async fn validate_scrape_request(
    request: &InternalScrapeRequest,
) -> Result<(), EngineError> {
    if request.method != crate::engines::engine_client::HttpMethod::Get {
        return Err(EngineError::Other("Unsupported HTTP method".to_string()));
    }
    // SSRF protection
    validators::validate_url(&request.url)
        .await
        .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;

    // Only run if specifically requested for JS or screenshot
    if !request.needs_js && !request.needs_screenshot {
        return Err(EngineError::AllEnginesFailed(
            "PlaywrightEngine only supports JS and screenshot requests".to_string(),
        ));
    }

    Ok(())
}

impl PlaywrightEngine {
    /// 浏览器会话执行体（自 [`PlaywrightEngine::scrape`] 提取）：获取页面 →
    /// 拦截/XHR 捕获 → 导航 → 收集响应。
    pub(super) async fn scrape_session(
        &self,
        request: &InternalScrapeRequest,
        pool: &BrowserPool,
        start: Instant,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let (pooled_page, page) = acquire_fingerprinted_page(pool, &self.ua_pool, request).await?;

        if !request.headers.is_empty() {
            // 如果 chromiumoxide 的 API 限制太多，我们暂时记录日志并跳过，
            // 或在未来版本中寻找更底层的 CDP 调用方式
            log::warn!("Custom headers are currently partially supported in PlaywrightEngine due to API constraints");
        }

        // 拦截事件处理 task 的守卫：声明在 if 块外的 async 作用域，覆盖整个
        // 导航→返回生命周期。无论 scrape 以成功返回、`?` 早退、还是 timeout
        // 取消（整个 future 被 drop）退出，守卫 drop 时都会 abort 拦截 task。
        let mut intercept_guard = InterceptGuard::new();

        setup_request_interception(&page, request, &mut intercept_guard).await?;

        // F6：XHR/Fetch 后台响应捕获会话（导航前启动，详见 XhrCaptureSession）。
        // 捕获与上方 Fetch 拦截并存互不干扰：被 FailRequest 的请求不会产生
        // ResponseReceived 事件，自然不出现在捕获中。capture_xhr=false 时不创建
        // 会话，全程零 Network 监听开销。
        let xhr_session = match &self.xhr_capture {
            Some(config) => XhrCaptureSession::start(&page, config).await,
            None => None,
        };

        navigate_and_wait(&page, request).await?;

        collect_and_build_response(&page, request, xhr_session.as_ref(), pooled_page, start).await
    }
}
