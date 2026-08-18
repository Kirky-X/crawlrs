// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T052: MLLM 自主导航爬取引擎
//!
//! 实现 `ScraperEngine` trait，在 `scrape()` 内封装 agentic loop：
//! 导航 → 截图 → vision_analyze → execute_decision → 循环，
//! 直到 Extract/Done 决策或达到 max_iterations。

use crate::domain::services::llm::vision_adapter::VisionAdapterTrait;
use crate::engines::client::playwright_pool::{get_global_pool, BrowserPool};
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::mllm::action_executor::{execute_decision, ActionResult};
use crate::engines::mllm::config::MllmEngineConfig;
use crate::engines::mllm::decision::{parse_decision, MllmDecision};
use crate::engines::validators;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// MLLM 引擎默认 MRT（60 秒）
///
/// MLLM 引擎涉及多轮截图+视觉模型推理，比普通浏览器引擎需要更多时间。
const DEFAULT_MLLM_MRT_SECONDS: u64 = 60;

/// MLLM 自主导航爬取引擎
///
/// 使用视觉大模型分析页面截图，自主决策导航操作，实现 agentic loop 式智能爬取。
pub struct MllmEngine {
    /// 浏览器池引用
    pool: Option<BrowserPool>,
    /// 视觉模型适配器
    vision_adapter: Arc<dyn VisionAdapterTrait>,
    /// 引擎配置
    config: MllmEngineConfig,
    /// 引擎级最大响应时间
    mrt: Duration,
}

impl MllmEngine {
    /// 创建 MLLM 引擎
    pub fn new(vision_adapter: Arc<dyn VisionAdapterTrait>, config: MllmEngineConfig) -> Self {
        let mrt = Duration::from_secs(config.mrt_seconds.max(DEFAULT_MLLM_MRT_SECONDS));
        Self {
            pool: None,
            vision_adapter,
            config,
            mrt,
        }
    }

    /// 设置浏览器池（用于测试注入）
    #[allow(dead_code)]
    pub fn with_pool(mut self, pool: BrowserPool) -> Self {
        self.pool = Some(pool);
        self
    }

    /// 获取或初始化浏览器池
    fn get_pool(&self) -> Result<&BrowserPool, EngineError> {
        if let Some(pool) = &self.pool {
            return Ok(pool);
        }
        get_global_pool().ok_or_else(|| {
            EngineError::BrowserError("Global browser pool not initialized".to_string())
        })
    }

    /// 截取当前页面的截图并返回 base64
    async fn take_screenshot_base64(
        &self,
        page: &chromiumoxide::Page,
    ) -> Result<String, EngineError> {
        let format = CaptureScreenshotFormat::Jpeg;
        let params = chromiumoxide::page::ScreenshotParams::builder()
            .format(format)
            .quality(self.config.screenshot_quality as i64)
            .full_page(false)
            .build();

        let screenshot_bytes = page
            .screenshot(params)
            .await
            .map_err(|e| EngineError::BrowserError(format!("Screenshot failed: {}", e)))?;

        Ok(BASE64.encode(screenshot_bytes))
    }

    /// 构造视觉分析的 prompt
    fn build_vision_prompt(
        &self,
        goal: &str,
        iteration: u8,
        last_result: Option<&ActionResult>,
    ) -> String {
        let mut prompt = format!(
            "Analyze the current page screenshot and decide the next action to accomplish this goal: {}\n",
            goal
        );

        if iteration > 0 {
            prompt.push_str(&format!(
                "\nThis is iteration {}/{}.\n",
                iteration, self.config.max_iterations
            ));
        }

        if let Some(result) = last_result {
            if !result.success {
                prompt.push_str(&format!(
                    "\nLast action failed: {}. Please try a different approach.\n",
                    result.message
                ));
            } else {
                prompt.push_str(&format!("\nLast action result: {}.\n", result.message));
            }
        }

        prompt.push_str("\nRespond with a JSON object representing your decision.");
        prompt
    }
}

#[async_trait]
impl ScraperEngine for MllmEngine {
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        // SSRF protection
        validators::validate_url(&request.url)
            .await
            .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;

        let start = Instant::now();
        let timeout_duration = request.timeout;

        let pool = self.get_pool()?;

        // Wrap in timeout
        tokio::time::timeout(timeout_duration, async {
            // Acquire browser page
            let pooled_page = pool.acquire_page().await?;
            let page: chromiumoxide::Page = pooled_page.page().clone();

            // Navigate to target URL
            info!("MLLM engine navigating to: {}", request.url);
            page.goto(&request.url)
                .await
                .map_err(|e| EngineError::BrowserError(format!("Navigation failed: {}", e)))?;

            // Wait for initial page load
            tokio::time::sleep(Duration::from_millis(1500)).await;

            // Extract goal from URL or request context
            let goal = extract_goal_from_request(request);

            // Agentic loop
            let mut last_result: Option<ActionResult> = None;
            let mut iteration: u8 = 0;
            let final_content;

            loop {
                if iteration >= self.config.max_iterations {
                    warn!(
                        "MLLM engine reached max_iterations ({}) for {}",
                        self.config.max_iterations, request.url
                    );
                    // Fallback: return current page content
                    final_content = get_page_content(&page).await;
                    break;
                }

                // Check timeout budget
                let elapsed = start.elapsed();
                if elapsed + Duration::from_secs(10) > timeout_duration {
                    warn!("MLLM engine approaching timeout for {}", request.url);
                    final_content = get_page_content(&page).await;
                    break;
                }

                iteration += 1;
                debug!(
                    "MLLM agentic loop iteration {}/{}",
                    iteration, self.config.max_iterations
                );

                // Step 1: Take screenshot
                let screenshot_b64 = self.take_screenshot_base64(&page).await?;

                // Step 2: Send to vision model
                let prompt = self.build_vision_prompt(&goal, iteration, last_result.as_ref());
                let vision_response = self
                    .vision_adapter
                    .send_vision_request(
                        &screenshot_b64,
                        &prompt,
                        &self.config.system_prompt,
                        &self.config.vision_model,
                    )
                    .await
                    .map_err(|e| {
                        error!("Vision model call failed: {}", e);
                        EngineError::Other(format!("Vision model call failed: {}", e))
                    })?;

                // Step 3: Parse decision
                let decision = match parse_decision(&vision_response) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Failed to parse vision response: {}", e);
                        last_result = Some(ActionResult::err(format!("parse error: {}", e)));
                        continue;
                    }
                };

                debug!("MLLM decision at iteration {}: {:?}", iteration, decision);

                // Step 4: Check for terminal decisions
                match &decision {
                    MllmDecision::Extract { .. } => {
                        final_content = get_page_content(&page).await;
                        info!("MLLM engine extracted content at iteration {}", iteration);
                        break;
                    }
                    MllmDecision::Done { .. } => {
                        final_content = get_page_content(&page).await;
                        info!(
                            "MLLM engine completed navigation at iteration {}",
                            iteration
                        );
                        break;
                    }
                    _ => {}
                }

                // Step 5: Execute the decision
                let result = execute_decision(&decision, &page).await;
                last_result = Some(result);

                // Brief pause for page to settle after action
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            // Take final screenshot if requested
            let mut screenshot: Option<String> = None;
            if request.needs_screenshot {
                screenshot = Some(self.take_screenshot_base64(&page).await?);
            }

            drop(pooled_page);

            Ok(InternalScrapeResponse {
                status_code: 200,
                content: final_content,
                screenshot,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: start.elapsed().as_millis() as u64,
            })
        })
        .await
        .map_err(|_| EngineError::Timeout(timeout_duration))?
    }

    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.needs_mllm {
            100 // Highest priority for MLLM requests
        } else {
            5 // Low priority for non-MLLM requests
        }
    }

    fn name(&self) -> &'static str {
        "mllm"
    }

    fn max_response_time(&self) -> Duration {
        self.mrt
    }
}

/// 从请求中提取导航目标描述
fn extract_goal_from_request(request: &InternalScrapeRequest) -> String {
    // 如果有 extraction prompt，用作目标
    // 否则使用 URL 作为默认目标
    if let Some(body) = &request.body {
        if !body.is_empty() {
            return body.clone();
        }
    }
    format!("Navigate to and extract content from: {}", request.url)
}

/// 获取页面 HTML 内容
async fn get_page_content(page: &chromiumoxide::Page) -> String {
    match page
        .evaluate_expression("document.documentElement.outerHTML")
        .await
    {
        Ok(result) => result.into_value::<String>().unwrap_or_default(),
        Err(e) => {
            error!("Failed to get page content: {}", e);
            String::new()
        }
    }
}
