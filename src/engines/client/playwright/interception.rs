// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Playwright 请求拦截 — 从 `playwright.rs` 拆分
//!
//! CDP Fetch domain 激活、暂停请求事件订阅与 continue/fail 分发。

use super::InterceptGuard;
use crate::engines::engine_client::{EngineError, InternalScrapeRequest};
use crate::engines::intercept::{InterceptController, ResourceKind, BLOCK_REASON};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams,
};
use futures::StreamExt;
use std::sync::Arc;

/// 请求拦截（广告/追踪域名 + 媒体资源）。
/// 仅当 block_ads 或 block_media 任一启用时激活 CDP Fetch domain 拦截。
/// 启用后所有请求被暂停，必须由事件处理 task 及时 continue/fail，
/// 否则请求会挂起直至超时。task 在 page 关闭后事件流结束自动退出。
pub(super) async fn setup_request_interception(
    page: &chromiumoxide::page::Page,
    request: &InternalScrapeRequest,
    intercept_guard: &mut InterceptGuard,
) -> Result<(), EngineError> {
    if request.block_ads || request.block_media {
        let controller = Arc::new(InterceptController::new(
            request.block_ads,
            request.block_media,
        ));

        // 启用 Fetch domain（默认拦截所有请求阶段）
        page.execute(EnableParams::default()).await.map_err(|e| {
            EngineError::BrowserError(format!("Failed to enable Fetch interception: {}", e))
        })?;

        // 订阅 EventRequestPaused 事件流
        let mut events = page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(|e| {
                EngineError::BrowserError(format!("Failed to subscribe EventRequestPaused: {}", e))
            })?;

        // 克隆 page + controller 给事件处理 task
        let page_clone = page.clone();
        let controller_clone = Arc::clone(&controller);

        // 事件处理 task：对每个被暂停的请求判断是否拦截
        // 命中黑名单/媒体 → FailRequest(BlockedByClient) + 计数
        // 否则 → ContinueRequest（放行）
        //
        // CDP `ResourceType` 在边界处通过 `ResourceKind::from` 转换为领域
        // `ResourceKind`，避免 InterceptController 依赖具体 CDP 实现。
        let intercept_handle = tokio::spawn(async move {
            while let Some(event) = events.next().await {
                dispatch_paused_request(&page_clone, &controller_clone, event).await;
            }
        });
        // 保存 JoinHandle 到守卫：scrape 退出（成功/错误/超时）时 abort，
        // 防止拦截 task 残留在归还池的复用页面上。
        intercept_guard.set(intercept_handle);
    }
    Ok(())
}

/// 处理单个被暂停的请求（自 [`setup_request_interception`] 提取）：
/// 命中黑名单/媒体 → FailRequest(BlockedByClient) + 计数；否则 ContinueRequest。
async fn dispatch_paused_request(
    page: &chromiumoxide::page::Page,
    controller: &InterceptController,
    event: Arc<EventRequestPaused>,
) {
    let url = event.request.url.clone();
    let kind = ResourceKind::from(event.resource_type.clone());
    let request_id = event.request_id.clone();
    if controller.should_block(&url, Some(kind)) {
        controller.record_block();
        if let Err(e) = page
            .execute(FailRequestParams::new(request_id, BLOCK_REASON))
            .await
        {
            log::debug!("FailRequest failed for {}: {}", url, e);
        }
    } else if let Err(e) = page.execute(ContinueRequestParams::new(request_id)).await {
        log::debug!("ContinueRequest failed for {}: {}", url, e);
    }
}
