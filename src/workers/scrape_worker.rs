// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::{Context, Result};
use chrono::Utc;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};
use url::Url;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::use_cases::create_scrape::CreateScrapeUseCase;
use crate::config::settings::Settings;
use crate::domain::models::crawl::CrawlStatus;
use crate::domain::models::scrape_result::ScrapeResult;
use crate::domain::models::task::{Task, TaskStatus, TaskType};
use crate::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::utils::regex_cache::RegexCache;

use crate::engines::engine_client::{
    EngineClient, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse, ScreenshotConfig,
    ScrollDirection,
};
#[cfg(feature = "redis-cache")]
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::queue::task_queue::TaskQueue;
use crate::utils::crawl_text_integration::{CrawlTextIntegration, ScrapeResponseInput};
use crate::utils::retry_policy::RetryPolicy;
use crate::utils::robots::{RobotsChecker, RobotsCheckerTrait};
use crate::workers::constants::CONCURRENCY_CONTROL_LUA;

// Regex cache for crawl pattern matching
use once_cell::sync::Lazy;
use thiserror::Error;

/// Worker 错误类型
#[derive(Error, Debug)]
pub enum ScrapeWorkerError {
    #[error("正则表达式编译错误: {0}")]
    RegexError(String),

    #[error("正则表达式缓存锁获取失败")]
    CacheLockError,

    #[error("选择器解析错误: {0}")]
    SelectorError(String),

    #[error("任务处理错误: {0}")]
    TaskError(String),
}

// From implementations for ScrapeWorkerError
impl From<String> for ScrapeWorkerError {
    fn from(msg: String) -> Self {
        ScrapeWorkerError::TaskError(msg)
    }
}

impl From<&str> for ScrapeWorkerError {
    fn from(msg: &str) -> Self {
        ScrapeWorkerError::TaskError(msg.to_string())
    }
}

impl From<anyhow::Error> for ScrapeWorkerError {
    fn from(err: anyhow::Error) -> Self {
        ScrapeWorkerError::TaskError(err.to_string())
    }
}

fn get_cached_regex(pattern: &str) -> Result<regex::Regex, ScrapeWorkerError> {
    RegexCache::global()
        .get_or_insert(pattern)
        .map_err(|e| ScrapeWorkerError::RegexError(e))
}

/// 抓取工作者
pub struct ScrapeWorker<R, S, C, CRR>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    CRR: CreditsRepository + Send + Sync,
{
    repository: Arc<R>,
    result_repository: Arc<S>,
    crawl_repository: Arc<C>,
    storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    credits_repository: Arc<CRR>,
    engine_client: Arc<EngineClient>,
    _create_scrape_use_case: Arc<CreateScrapeUseCase>,
    #[cfg(feature = "redis-cache")]
    redis: RedisClient,
    robots_checker: Arc<RobotsChecker>,
    settings: Arc<Settings>,
    worker_id: Uuid,
    default_concurrency_limit: usize,
    retry_policy: RetryPolicy,
}

impl<R, S, C, CRR> ScrapeWorker<R, S, C, CRR>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    CRR: CreditsRepository + Send + Sync,
{
    /// 创建新的抓取工作器实例
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "redis-cache")]
    pub fn new(
        repository: Arc<R>,
        result_repository: Arc<S>,
        crawl_repository: Arc<C>,
        storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
        webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
        credits_repository: Arc<CRR>,
        engine_client: Arc<EngineClient>,
        _create_scrape_use_case: Arc<CreateScrapeUseCase>,
        redis: RedisClient,
        robots_checker: Arc<RobotsChecker>,
        settings: Arc<Settings>,
        default_concurrency_limit: usize,
    ) -> Self {
        // 根据任务类型选择合适的重试策略
        let retry_policy = RetryPolicy::slow(); // 网络请求适合慢速重试策略

        Self {
            repository,
            result_repository,
            crawl_repository,
            storage_repository,
            webhook_event_repository,
            credits_repository,
            engine_client,
            _create_scrape_use_case,
            redis,
            robots_checker,
            settings,
            worker_id: Uuid::new_v4(),
            default_concurrency_limit,
            retry_policy,
        }
    }

    /// 运行抓取工作器
    pub async fn run<Q>(&self, queue: Q)
    where
        Q: TaskQueue + Send + Sync,
    {
        info!("Scrape worker {} started", self.worker_id);

        loop {
            match self.process_next_task(&queue).await {
                Ok(processed) => {
                    if !processed {
                        sleep(Duration::from_secs(1)).await;
                    }
                }
                Err(e) => {
                    error!("Error processing task: {}", e);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn process_next_task<Q>(&self, queue: &Q) -> Result<bool>
    where
        Q: TaskQueue,
    {
        let task_opt = queue.dequeue(self.worker_id).await?;

        if let Some(task) = task_opt {
            self.process_task(task).await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn acquire_concurrency_permit(&self, task: &Task) -> Result<bool> {
        let team_id = task.team_id;
        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        let team_concurrency_limit_key = format!("team:{}:concurrency_limit", team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        // Get limit - Priority: 1. Task Payload 2. Redis Key 3. Default
        let payload_limit = if task.task_type == TaskType::Crawl {
            task.payload
                .get("config")
                .and_then(|c| c.get("max_concurrency"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
        } else {
            None
        };

        let default_limit = payload_limit.unwrap_or(self.default_concurrency_limit);

        // Execute atomic Lua script - reduces 4 Redis calls to 1
        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&team_active_tasks_key, &team_concurrency_limit_key],
                &[
                    &task.id.to_string(),
                    &now.to_string(),
                    &stale_threshold.to_string(),
                    &default_limit.to_string(),
                ],
            )
            .await?;

        let granted = result == "1";
        Ok(granted)
    }

    async fn release_concurrency_permit(&self, team_id: Uuid, task_id: Uuid) -> Result<()> {
        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        self.redis
            .zrem(&team_active_tasks_key, &task_id.to_string())
            .await?;
        Ok(())
    }

    #[instrument(skip(self, task), fields(task_id = %task.id, url = %task.url, task_type = %task.task_type))]
    async fn process_task(&self, mut task: Task) -> Result<()> {
        debug!(task_id = %task.id, task_type = ?task.task_type);
        info!("Processing task");

        // Check Task Expiration
        if let Some(expires_at) = task.expires_at {
            if Utc::now() > expires_at {
                warn!("Task {} expired at {}", task.id, expires_at);
                self.repository.mark_failed(task.id).await?;
                // Trigger failure webhook if needed
                let event_type = match task.task_type {
                    TaskType::Scrape => WebhookEventType::ScrapeFailed,
                    TaskType::Crawl => WebhookEventType::CrawlFailed,
                    _ => WebhookEventType::Custom("task.failed".to_string()),
                };
                self.trigger_webhook(&task, event_type, Some("Task expired".to_string()))
                    .await;
                return Ok(());
            }
        }

        // Concurrency Check (Layer 2: Team Semaphore)
        if !self.acquire_concurrency_permit(&task).await? {
            warn!(
                "Team {} concurrency limit exceeded, rescheduling task {}",
                task.team_id, task.id
            );
            // Reschedule logic (Backlog)
            // Delay by 30 seconds
            task.scheduled_at = Some((Utc::now() + chrono::Duration::seconds(30)).into());
            task.status = TaskStatus::Queued;
            // Reset attempt count to avoid failing task due to concurrency limits?
            // Or keep it? If we keep it, it might eventually fail.
            // PRD says "Enter backlog". It doesn't imply failure count increment.
            // So we probably shouldn't increment attempt_count here, but update resets status.
            // Since we acquired it, attempt_count might have been incremented by queue?
            // PostgresTaskQueue usually doesn't increment attempt_count on acquire, only on error handling?
            // Let's assume we just update it.
            self.repository.update(&task).await?;
            return Ok(());
        }

        // Extract values needed after task is moved
        let task_id = task.id;
        let team_id = task.team_id;

        let task_type = task.task_type;

        // Take task by value only for the specific branch that needs it
        // This avoids 3 unnecessary clones in the match
        let result = match task_type {
            TaskType::Scrape => self.process_scrape_task(task).await,
            TaskType::Crawl => self.process_crawl_task(task).await,
            TaskType::Extract => self.process_extract_task(task).await,
        };

        // Always release permit
        if let Err(e) = self.release_concurrency_permit(team_id, task_id).await {
            error!(
                "Failed to release concurrency permit for team {}: {}",
                team_id, e
            );
        }

        if let Err(ref e) = result {
            debug!(error = %e);
        } else {
            debug!("Task processing completed successfully");
        }

        result
    }

    async fn process_scrape_task(&self, mut task: Task) -> Result<()> {
        debug!(task_id = %task.id);

        // Resolve engine router directly to handle actions if they exist
        let scrape_request = Self::build_scrape_request(&task).unwrap_or_else(|e| {
            error!("Failed to parse task payload, using default: {}", e);
            ScrapeRequest::new(task.url.clone()).timeout(Duration::from_secs(30))
        });

        let response = self.engine_client.scrape(&scrape_request).await;

        match response {
            Ok(response) => {
                debug!(status_code = response.status_code);
                info!("Scrape successful, status: {}", response.status_code);

                // Map ScrapeResponse to ScrapeResult
                // _result variable is currently unused but might be used later or for debugging
                let _result = ScrapeResult {
                    id: Uuid::new_v4(),
                    task_id: task.id,
                    url: task.url.clone(),
                    status_code: response.status_code,
                    content: response.content.clone(),
                    content_type: response.content_type.clone(),
                    headers: serde_json::to_value(&response.headers).unwrap_or(Value::Null),
                    meta_data: Value::Null,
                    screenshot: response.screenshot.clone(),
                    response_time_ms: response.response_time_ms,
                    created_at: Utc::now(),
                };

                if let Err(e) = self.handle_scrape_success(&task, &response).await {
                    error!("Scrape success handler failed: {}", e);
                    debug!(error = %e);
                    self.handle_failure(&mut task).await?;
                } else {
                    debug!("Scrape success handler completed successfully");
                    // 扣除基础费用及高级功能费用 (PRD-253)
                    self.deduct_feature_credits(
                        task.team_id,
                        task.id,
                        response.screenshot.is_some(),
                        scrape_request.options.proxy.is_some(),
                    )
                    .await;
                }
                Ok(())
            }
            Err(e) => {
                error!("Scrape failed: {}", e);
                debug!(error = %e);

                // If it's a timeout error, mark as failed immediately instead of rescheduling
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("timeout")
                    || err_str.contains("expired")
                    || err_str.contains("all engines failed")
                {
                    debug!("Timeout or AllEnginesFailed detected, marking task as failed");
                    // Fetch task to ensure we have latest state
                    if let Ok(Some(mut t)) = self.repository.find_by_id(task.id).await {
                        t.status = TaskStatus::Failed;
                        t.completed_at = Some(Utc::now().into());
                        // Add error to payload for tracking
                        let mut payload = t.payload.clone();
                        if let Some(obj) = payload.as_object_mut() {
                            obj.insert("error".to_string(), json!(e.to_string()));
                        }
                        t.payload = payload;
                        self.repository.update(&t).await?;
                    }
                } else {
                    self.handle_failure(&mut task).await?;
                }

                // 触发失败 Webhook
                self.trigger_webhook(&task, WebhookEventType::ScrapeFailed, Some(e.to_string()))
                    .await;
                Ok(())
            }
        }
    }

    async fn process_crawl_task(&self, mut task: Task) -> Result<()> {
        // 1. 解析 Crawl 任务特定的 Payload
        // Payload 格式: { "crawl_id": "...", "depth": 0, "config": { ... } }
        let payload = &task.payload;
        let crawl_id = match payload.get("crawl_id").and_then(|v| v.as_str()) {
            Some(id) => Uuid::parse_str(id).unwrap_or_default(),
            None => {
                error!("Missing crawl_id in task payload");
                self.repository.mark_failed(task.id).await?;
                return Ok(());
            }
        };

        let depth = payload.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let config: CrawlConfigDto =
            match serde_json::from_value(payload.get("config").cloned().unwrap_or(json!({}))) {
                Ok(c) => c,
                Err(e) => {
                    error!("Invalid crawl config: {}", e);
                    self.handle_failure(&mut task).await?;
                    return Ok(());
                }
            };

        // Robots.txt Check
        let user_agent = "crawlrs-bot";
        if !self
            .robots_checker
            .is_allowed(&task.url, user_agent)
            .await
            .unwrap_or(true)
        {
            info!("Access denied by robots.txt for {}", task.url);
            // Mark as failed or maybe a specific status like "Skipped" or "Blocked"
            // For now, mark as failed but maybe we should add a reason
            self.repository.mark_failed(task.id).await?;
            return Ok(());
        }

        if let Some(delay) = self
            .robots_checker
            .get_crawl_delay(&task.url, user_agent)
            .await
            .unwrap_or(None)
        {
            info!("Respecting crawl delay of {:?} for {}", delay, task.url);
            sleep(delay).await;
        }

        // 2. 构建抓取请求
        let mut headers = HashMap::with_capacity(16);
        if let Some(h) = &config.headers {
            if let Some(obj) = h.as_object() {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        headers.insert(k.clone(), s.to_string());
                    }
                }
            }
        }

        let request = ScrapeRequest::new(task.url.clone()).with_options(ScrapeOptions {
            headers,
            timeout: Duration::from_secs(30),
            needs_js: false, // 爬虫默认不需要 JS，除非配置指定
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: config.proxy.clone(),
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            sync_wait_ms: 0,
        });

        // 检查配置中是否有自定义请求头 (假设 CrawlConfigDto 中可能包含 headers 字段，如果没有则忽略)
        // 目前 CrawlConfigDto 定义如下：
        // pub struct CrawlConfigDto {
        //     pub max_depth: u32,
        //     pub include_patterns: Option<Vec<String>>,
        //     pub exclude_patterns: Option<Vec<String>>,
        //     pub strategy: Option<String>,
        // }
        // 我们可以扩展 CrawlConfigDto 或在 payload 中单独传递 headers。
        // 暂时假设不传递自定义 headers，或者从 config.strategy 中解析特殊需求。

        let response = self.engine_client.scrape(&request).await;

        match response {
            Ok(response) => {
                info!(
                    "Crawl step successful, url: {}, status: {}",
                    task.url, response.status_code
                );

                // 文本编码处理 - 集成文本处理功能
                let processed_content = match self.process_text_encoding(&task, &response).await {
                    Ok(content) => content,
                    Err(e) => {
                        warn!("文本编码处理失败，使用原始内容: {}", e);
                        response.content.clone()
                    }
                };

                // 创建处理后的响应用于后续处理
                let processed_response = ScrapeResponse {
                    content: processed_content,
                    ..response
                };

                // Map ScrapeResponse to ScrapeResult (No need to map here, we use ScrapeResponse directly)

                // 3. 执行数据提取（如果配置了提取规则）
                let mut extracted_data = None;
                if let Some(rules) = &config.extraction_rules {
                    match crate::domain::services::extraction_service::ExtractionService::extract(
                        &processed_response.content,
                        rules,
                        &self.settings,
                    )
                    .await
                    {
                        Ok((data, usage)) => {
                            extracted_data = Some(data);
                            // Record usage (PRD-334: Tokens Billing)
                            self.deduct_token_credits(
                                task.team_id,
                                task.id,
                                &usage,
                                "Tokens used for extraction",
                            )
                            .await;
                        }
                        Err(e) => {
                            error!("Extraction failed for url {}: {}", task.url, e);
                        }
                    }
                }

                // 4. 保存结果
                self.save_result(&task, &processed_response, extracted_data)
                    .await?;

                // 5. 如果深度未达上限，解析链接并生成子任务
                if depth < config.max_depth {
                    self.extract_and_queue_links(
                        &task,
                        &processed_response,
                        crawl_id,
                        depth,
                        &config,
                    )
                    .await?;
                }

                // 5. 更新任务状态和 Crawl 统计
                self.repository.mark_completed(task.id).await?;
                if let Err(e) = self
                    .crawl_repository
                    .increment_completed_tasks(crawl_id)
                    .await
                {
                    error!(
                        "Failed to increment completed tasks for crawl {}: {}",
                        crawl_id, e
                    );
                }

                // Check if all tasks are completed
                match self.crawl_repository.find_by_id(crawl_id).await {
                    Ok(Some(c)) => {
                        if c.completed_tasks + c.failed_tasks == c.total_tasks {
                            info!(
                                "All tasks completed for crawl {}, marking as completed",
                                crawl_id
                            );
                            if let Err(e) = self
                                .crawl_repository
                                .update_status(crawl_id, CrawlStatus::Completed)
                                .await
                            {
                                error!(
                                    "Failed to update crawl status to completed for crawl {}: {}",
                                    crawl_id, e
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        error!("Crawl not found for id {}", crawl_id);
                    }
                    Err(e) => {
                        error!("Failed to fetch crawl {}: {}", crawl_id, e);
                    }
                }

                // 扣除高级功能费用 (Proxy/Screenshot)
                self.deduct_feature_credits(
                    task.team_id,
                    task.id,
                    processed_response.screenshot.is_some(),
                    request.options.proxy.is_some(),
                )
                .await;

                Ok(())
            }
            Err(e) => {
                // Deduct proxy credits if proxy was used, even if failed
                self.deduct_feature_credits(
                    task.team_id,
                    task.id,
                    false, // No screenshot on failure
                    request.options.proxy.is_some(),
                )
                .await;

                error!("Crawl step failed: {}", e);
                self.handle_failure(&mut task).await?;
                if let Err(e) = self.crawl_repository.increment_failed_tasks(crawl_id).await {
                    error!(
                        "Failed to increment failed tasks for crawl {}: {}",
                        crawl_id, e
                    );
                }

                // Check if all tasks are completed (even with failure)
                match self.crawl_repository.find_by_id(crawl_id).await {
                    Ok(Some(c)) => {
                        if c.completed_tasks + c.failed_tasks == c.total_tasks {
                            info!("All tasks completed (some failed) for crawl {}, marking as completed", crawl_id);
                            if let Err(e) = self
                                .crawl_repository
                                .update_status(crawl_id, CrawlStatus::Completed)
                                .await
                            {
                                error!(
                                    "Failed to update crawl status to completed for crawl {}: {}",
                                    crawl_id, e
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        error!("Crawl not found for id {}", crawl_id);
                    }
                    Err(e) => {
                        error!("Failed to fetch crawl {}: {}", crawl_id, e);
                    }
                }

                // 触发失败 Webhook
                self.trigger_webhook(&task, WebhookEventType::CrawlFailed, Some(e.to_string()))
                    .await;
                Ok(())
            }
        }
    }

    async fn process_extract_task(&self, mut task: Task) -> Result<()> {
        info!("Processing extract task {}", task.id);

        let payload: crate::application::dto::extract_request::ExtractRequestDto =
            serde_json::from_value(task.payload.clone())
                .context("Failed to parse extract task input")?;

        debug!(has_rules = payload.rules.is_some());
        if let Some(ref rules) = payload.rules {
            debug!(rules_count = rules.len());
        }

        let url = payload.urls.first().context("No URL provided")?.clone();

        // 1. Scrape Content
        let scrape_req = ScrapeRequest::new(url.clone()).with_options(ScrapeOptions {
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: true,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: vec![],
            sync_wait_ms: 0,
        });

        // Execute scrape
        let scrape_resp = self.engine_client.scrape(&scrape_req).await?;

        // 文本编码处理 - 集成文本处理功能
        let processed_content = match self.process_text_encoding(&task, &scrape_resp).await {
            Ok(content) => content,
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                scrape_resp.content.clone()
            }
        };

        // 创建处理后的响应用于后续处理
        let processed_scrape_resp = ScrapeResponse {
            content: processed_content,
            ..scrape_resp
        };

        // 2. Extract Data using ExtractionService (which uses LLM internally if configured)

        // Handle extraction rules if provided
        if let Some(rules) = payload.rules {
            debug!(?rules);
            // Use provided extraction rules with potential LLM usage
            let (extracted_data, usage) =
                crate::domain::services::extraction_service::ExtractionService::extract(
                    &processed_scrape_resp.content,
                    &rules,
                    &self.settings,
                )
                .await?;

            debug!(?usage);
            debug!(?extracted_data);

            // Record usage and deduct credits for LLM usage
            self.deduct_token_credits(
                task.team_id,
                task.id,
                &usage,
                "Tokens used for extraction rules",
            )
            .await;

            // Save results
            let mut scrape_result = ScrapeResult::new(
                task.id,
                url.clone(),
                processed_scrape_resp.status_code,
                processed_scrape_resp.content.clone(),
                "text/html".to_string(),
                0,
            );
            scrape_result.meta_data = json!({
                "extracted_data": extracted_data
            });

            self.result_repository.save(scrape_result).await?;

            task.status = TaskStatus::Completed;
            self.repository.update(&task).await?;

            self.trigger_webhook(
                &task,
                WebhookEventType::Custom("extract.completed".to_string()),
                None,
            )
            .await;

            return Ok(());
        }

        // Handle prompt-based extraction (legacy)
        let mut rules = HashMap::with_capacity(8);
        if let Some(prompt) = payload.prompt {
            rules.insert(
                "extracted_data".to_string(),
                crate::domain::services::extraction_service::ExtractionRule {
                    selector: None,
                    attr: None,
                    is_array: false,
                    use_llm: Some(true),
                    llm_prompt: Some(prompt),
                },
            );

            // Use extraction rules for prompt-based extraction
            let (extracted_data, usage) =
                crate::domain::services::extraction_service::ExtractionService::extract(
                    &processed_scrape_resp.content,
                    &rules,
                    &self.settings,
                )
                .await?;

            // Record usage and deduct credits
            self.deduct_token_credits(task.team_id, task.id, &usage, "Tokens used for extraction")
                .await;

            // Save results
            let mut scrape_result = ScrapeResult::new(
                task.id,
                url.clone(),
                processed_scrape_resp.status_code,
                processed_scrape_resp.content.clone(),
                "text/html".to_string(),
                0,
            );
            scrape_result.meta_data = json!({
                "extracted_data": extracted_data
            });

            self.result_repository.save(scrape_result).await?;

            task.status = TaskStatus::Completed;
            self.repository.update(&task).await?;

            self.trigger_webhook(
                &task,
                WebhookEventType::Custom("extract.completed".to_string()),
                None,
            )
            .await;

            return Ok(());
        } else if let Some(_schema) = payload.schema {
            // 使用新实现的 extract_with_schema 优化提取流程
            let (extracted_data, usage) =
                crate::domain::services::extraction_service::ExtractionService::extract_with_schema(
                    &processed_scrape_resp.content,
                    &_schema,
                    &self.settings,
                )
                    .await?;

            // Record usage and deduct credits
            self.deduct_token_credits(
                task.team_id,
                task.id,
                &usage,
                "Tokens used for schema extraction",
            )
            .await;

            // Save results
            let mut scrape_result = ScrapeResult::new(
                task.id,
                url,
                processed_scrape_resp.status_code,
                processed_scrape_resp.content,
                "text/html".to_string(),
                0,
            );
            scrape_result.meta_data = json!({
                "extracted_data": extracted_data
            });

            self.result_repository.save(scrape_result).await?;

            task.status = TaskStatus::Completed;
            self.repository.update(&task).await?;

            self.trigger_webhook(
                &task,
                WebhookEventType::Custom("extract.completed".to_string()),
                None,
            )
            .await;

            return Ok(());
        }

        // Fallback if no schema/prompt (should usually have one)
        // If we reach here, it means we didn't do the direct LLM call above.
        // We could default to empty extraction or error.

        let scrape_result = ScrapeResult::new(
            task.id,
            url,
            processed_scrape_resp.status_code,
            processed_scrape_resp.content,
            "text/html".to_string(),
            0,
        );
        self.result_repository.save(scrape_result).await?;

        task.status = TaskStatus::Completed;
        self.repository.update(&task).await?;

        self.trigger_webhook(
            &task,
            WebhookEventType::Custom("extract.completed".to_string()),
            None,
        )
        .await;

        Ok(())
    }

    async fn extract_and_queue_links(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        crawl_id: Uuid,
        current_depth: u32,
        config: &CrawlConfigDto,
    ) -> Result<()> {
        // 只解析 HTML 内容
        if !response.content_type.contains("text/html") {
            return Ok(());
        }

        let unique_links = {
            let document = Html::parse_document(&response.content);
            let selector = Selector::parse("a")
                .map_err(|e| ScrapeWorkerError::SelectorError(e.to_string()))?;
            let base_url = Url::parse(&task.url)?;

            let mut links = HashSet::new();

            for element in document.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    // 转换相对路径为绝对路径
                    if let Ok(absolute_url) = base_url.join(href) {
                        let url_str = absolute_url.to_string();

                        // 过滤非 http/https 协议
                        if !url_str.starts_with("http") {
                            continue;
                        }

                        // 过滤自身
                        if url_str == task.url {
                            continue;
                        }

                        // 检查包含/排除模式
                        if !self.should_crawl(&url_str, config) {
                            continue;
                        }

                        links.insert(url_str);
                    }
                }
            }
            links
        };

        info!("Found {} unique links on {}", unique_links.len(), task.url);

        for link in unique_links.iter() {
            // 检查是否已经抓取过 (去重)
            // 这里简单使用 repository 检查 URL 是否存在
            // 在大规模系统中可能需要 BloomFilter 或 Redis Set
            if self.repository.exists_by_url(link).await? {
                continue;
            }

            // Re-construct with strategy adjustment
            let mut priority = task.priority;
            if let Some(strategy) = &config.strategy {
                if strategy.to_lowercase() == "dfs" {
                    priority = priority.saturating_add(1);
                }
            }

            let new_task = Task {
                id: Uuid::new_v4(),
                task_type: TaskType::Crawl,
                status: TaskStatus::Queued,
                priority,
                team_id: task.team_id,
                url: link.to_string(),
                payload: json!({
                    "crawl_id": crawl_id.to_string(),
                    "depth": current_depth + 1,
                    "config": config
                }),
                retry_count: 0,
                attempt_count: 0,
                max_retries: 3,
                scheduled_at: None,
                created_at: Utc::now().into(),
                started_at: None,
                completed_at: None,
                crawl_id: Some(crawl_id),
                updated_at: Utc::now().into(),
                lock_token: None,
                lock_expires_at: None,
                expires_at: None,
            };

            self.repository.create(&new_task).await?;
            self.crawl_repository
                .increment_total_tasks(crawl_id)
                .await?;
        }

        Ok(())
    }

    fn should_crawl(&self, url: &str, config: &CrawlConfigDto) -> bool {
        // 1. 检查包含模式 (如果有配置，必须匹配其中一个)
        if let Some(includes) = &config.include_patterns {
            let mut matched = false;
            for pattern in includes {
                if let Ok(re) = get_cached_regex(pattern) {
                    if re.is_match(url) {
                        matched = true;
                        break;
                    }
                } else if url.contains(pattern) {
                    // 简单的字符串包含回退
                    matched = true;
                    break;
                }
            }
            if !matched {
                return false;
            }
        }

        // 2. 检查排除模式 (如果有配置，不能匹配任何一个)
        if let Some(excludes) = &config.exclude_patterns {
            for pattern in excludes {
                if let Ok(re) = get_cached_regex(pattern) {
                    if re.is_match(url) {
                        return false;
                    }
                } else if url.contains(pattern) {
                    return false;
                }
            }
        }

        true
    }

    async fn handle_scrape_success(&self, task: &Task, response: &ScrapeResponse) -> Result<()> {
        debug!(task_id = %task.id);

        // 文本编码处理 - 集成文本处理功能
        let processed_content = match self.process_text_encoding(task, response).await {
            Ok(content) => content,
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                response.content.clone()
            }
        };

        // 创建处理后的响应用于后续处理
        let processed_response = ScrapeResponse {
            content: processed_content,
            status_code: response.status_code,
            screenshot: response.screenshot.clone(),
            content_type: response.content_type.clone(),
            headers: response.headers.clone(),
            response_time_ms: response.response_time_ms,
            ..response.clone()
        };

        // 解析 ScrapeRequest 以检查是否有提取规则
        let mut extracted_data = None;
        if let Ok(req) = serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
            if let Some(rules) = &req.extraction_rules {
                match crate::domain::services::extraction_service::ExtractionService::extract(
                    &processed_response.content,
                    rules,
                    &self.settings,
                )
                .await
                {
                    Ok((data, usage)) => {
                        extracted_data = Some(data);
                        // Record usage (PRD-334: Tokens Billing)
                        if usage.total_tokens > 0 {
                            // 1. Record in Redis for real-time tracking
                            let key = format!("team:{}:token_usage", task.team_id);
                            if let Err(e) =
                                self.redis.incr_by(&key, usage.total_tokens as i64).await
                            {
                                error!("Failed to record token usage in Redis: {}", e);
                            }

                            // 2. Convert to credits and deduct from database
                            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
                            let credits_to_deduct =
                                std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
                            if credits_to_deduct > 0 {
                                if let Err(e) = self.credits_repository.deduct_credits(
                                    task.team_id,
                                    credits_to_deduct,
                                    crate::domain::models::credits::CreditsTransactionType::Extract,
                                    format!("Tokens used for extraction ({} tokens)", usage.total_tokens),
                                    Some(task.id),
                                ).await {
                                    error!("Failed to deduct credits for token usage: {}", e);
                                } else {
                                    info!(
                                            "Deducted {} credits for {} tokens for team {}",
                                            credits_to_deduct, usage.total_tokens, task.team_id
                                        );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Extraction failed for url {}: {}", task.url, e);
                    }
                }
            }
        }

        self.save_result(task, &processed_response, extracted_data)
            .await?;
        debug!(task_id = %task.id, "About to mark task as completed");
        self.repository.mark_completed(task.id).await?;
        debug!(task_id = %task.id, "Successfully marked task as completed");

        self.trigger_webhook(task, WebhookEventType::ScrapeCompleted, None)
            .await;
        Ok(())
    }

    /// 处理文本编码转换
    async fn process_text_encoding(
        &self,
        task: &Task,
        response: &ScrapeResponse,
    ) -> Result<String> {
        use tracing::{info, warn};

        info!(
            "开始处理文本编码转换，任务ID: {}, URL: {}",
            task.id, task.url
        );

        // 创建文本处理集成器
        let text_integration = CrawlTextIntegration::new(false); // Disable by default for now

        // 准备输入数据
        let input = ScrapeResponseInput {
            content: response.content.as_bytes().to_vec(),
            url: task.url.clone(),
            content_type: Some(response.content_type.clone()),
            status_code: response.status_code,
        };

        // 处理响应内容
        match text_integration
            .process_scrape_response(
                &input.content,
                &input.url,
                input.content_type.as_deref(),
                input.status_code,
            )
            .await
        {
            Ok(processed_response) => {
                if processed_response.processing_success {
                    info!(
                        "文本编码处理成功，检测到的编码: {:?}, 处理时间: {}ms, 质量评分: {}",
                        processed_response.encoding_detected,
                        processed_response.processing_success as u32,
                        processed_response.processing_error.is_none() as u32
                    );
                    Ok(processed_response.processed_content)
                } else {
                    let error_msg = processed_response
                        .processing_error
                        .unwrap_or_else(|| "未知错误".to_string());
                    warn!("文本编码处理失败: {}", error_msg);
                    Err(anyhow::anyhow!("文本编码处理失败: {}", error_msg))
                }
            }
            Err(e) => {
                warn!("文本编码处理异常: {}", e);
                Err(anyhow::anyhow!("文本编码处理异常: {}", e))
            }
        }
    }

    async fn save_result(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        extra_data: Option<Value>,
    ) -> Result<()> {
        let mut meta_data = Value::Null;
        if let Some(data) = extra_data {
            meta_data = data;
        }

        // Handle large content/screenshot storage if StorageRepository is available
        let content_to_store = response.content.clone();
        let _screenshot_to_store = response.screenshot.clone();

        // Example logic: if content is very large, store in S3/Local and save reference
        if let Some(storage) = &self.storage_repository {
            if content_to_store.len() > 1024 * 1024 {
                // 1MB threshold
                let key = format!("content/{}/{}.html", task.id, Uuid::new_v4());
                if let Err(e) = storage.save(&key, content_to_store.as_bytes()).await {
                    error!("Failed to store content to storage: {}", e);
                    // Fallback to DB or fail? For now, log error and continue with DB
                } else {
                    // Store reference in metadata or specific field if we had one.
                    // Currently ScrapeResult stores content directly.
                    // In a real scenario, we might change ScrapeResult.content to be Option or handle "external" storage.
                    // For this implementation, we'll just log that we *could* offload it.
                    info!("Content stored in external storage: {}", key);
                }
            }
        }

        // Create result entity
        let result = ScrapeResult {
            id: Uuid::new_v4(),
            task_id: task.id,
            url: task.url.clone(),
            status_code: response.status_code,
            content: content_to_store,
            content_type: response.content_type.clone(),
            headers: serde_json::to_value(&response.headers).unwrap_or(Value::Null),
            meta_data,
            screenshot: response.screenshot.clone(),
            response_time_ms: response.response_time_ms,
            created_at: Utc::now(),
        };

        self.result_repository.save(result).await?;
        Ok(())
    }

    async fn trigger_webhook(
        &self,
        task: &Task,
        event_type: WebhookEventType,
        error_msg: Option<String>,
    ) {
        // 尝试从 payload 中解析 ScrapeRequestDto 来获取 webhook url
        // 注意：Crawl 任务的 payload 结构不同，这里主要针对 Scrape 任务
        // 对于 Crawl 任务，通常 webhook 是在 Crawl 级别配置的，这里简化处理

        let webhook_url =
            if let Ok(req) = serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
                req.webhook
            } else {
                // 尝试直接从 payload 获取 webhook 字段 (针对 Crawl 任务的潜在扩展)
                task.payload
                    .get("webhook")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            };

        if let Some(url) = webhook_url {
            info!("Triggering webhook {:?} for task {}", event_type, task.id);

            let mut payload = json!({
                "task_id": task.id,
                "status": if error_msg.is_some() { "failed" } else { "completed" },
                "url": task.url,
                "timestamp": Utc::now().timestamp()
            });

            if let Some(msg) = error_msg {
                payload["error"] = json!(msg);
            }

            let event = WebhookEvent {
                id: Uuid::new_v4(),
                team_id: task.team_id,
                webhook_id: Uuid::nil(), // Direct webhook URL without associated webhook record
                event_type,
                payload,
                webhook_url: url,
                status: WebhookStatus::Pending,
                attempt_count: 0,
                max_retries: 5,
                response_status: None,
                response_body: None,
                error_message: None,
                next_retry_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                delivered_at: None,
            };

            if let Err(e) = self.webhook_event_repository.create(&event).await {
                error!("Failed to create webhook event: {}", e);
            }
        }
    }

    async fn handle_failure(&self, task: &mut Task) -> Result<()> {
        let new_attempt_count = task.attempt_count + 1;

        if !self.retry_policy.should_retry(new_attempt_count as u32) {
            warn!("Task failed after {} retries", task.max_retries);
            self.repository.mark_failed(task.id).await?;
        } else {
            // 使用可配置的重试策略计算退避时间
            let backoff_duration = self
                .retry_policy
                .calculate_backoff(new_attempt_count as u32);
            let next_retry =
                Utc::now() + chrono::Duration::milliseconds(backoff_duration.as_millis() as i64);

            task.attempt_count = new_attempt_count;
            task.scheduled_at = Some(next_retry.into());
            task.status = TaskStatus::Queued;

            self.repository.update(task).await?;
            info!(
                "Scheduled retry {}/{} for task {} in {:?} (backoff: {:?})",
                new_attempt_count, task.max_retries, task.id, backoff_duration, next_retry
            );
        }

        Ok(())
    }

    async fn deduct_feature_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        screenshot: bool,
        proxy: bool,
    ) {
        let mut extra_credits = 0;

        // 2. Screenshot: 2 Credits
        if screenshot {
            extra_credits += 2;
        }

        // 3. Proxy: 1 Credit
        if proxy {
            extra_credits += 1;
        }

        if extra_credits > 0 {
            if let Err(e) = self
                .credits_repository
                .deduct_credits(
                    team_id,
                    extra_credits,
                    crate::domain::models::credits::CreditsTransactionType::Scrape,
                    format!(
                        "Extra credits for scrape (screenshot/proxy) for task {}",
                        task_id
                    ),
                    Some(task_id),
                )
                .await
            {
                error!("Failed to deduct extra credits for task {}: {}", task_id, e);
            }
        }
    }

    async fn deduct_token_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        usage: &crate::domain::services::llm_service::TokenUsage,
        description: &str,
    ) {
        if usage.total_tokens > 0 {
            // 1. Record in Redis for real-time tracking
            let key = format!("team:{}:token_usage", team_id);
            if let Err(e) = self.redis.incr_by(&key, usage.total_tokens as i64).await {
                error!("Failed to record token usage in Redis: {}", e);
            }

            // 2. Convert to credits and deduct from database
            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
            let credits_to_deduct = std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
            if credits_to_deduct > 0 {
                if let Err(e) = self
                    .credits_repository
                    .deduct_credits(
                        team_id,
                        credits_to_deduct,
                        crate::domain::models::credits::CreditsTransactionType::Extract,
                        format!("{} ({} tokens)", description, usage.total_tokens),
                        Some(task_id),
                    )
                    .await
                {
                    error!("Failed to deduct credits for token usage: {}", e);
                } else {
                    info!(
                        "Deducted {} credits for {} tokens for team {}",
                        credits_to_deduct, usage.total_tokens, team_id
                    );
                }
            }
        }
    }

    pub fn build_scrape_request(task: &Task) -> Result<ScrapeRequest> {
        let scrape_request: ScrapeRequestDto =
            serde_json::from_value(task.payload.clone()).context("Failed to parse task payload")?;

        let options = scrape_request.options.as_ref();

        let mut headers = HashMap::with_capacity(16);
        if let Some(opts) = options {
            if let Some(h) = &opts.headers {
                if let Some(obj) = h.as_object() {
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            headers.insert(k.clone(), s.to_string());
                        }
                    }
                }
            }
        }

        let needs_js = scrape_request
            .actions
            .as_ref()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || options.and_then(|o| o.js_rendering).unwrap_or(false);

        let screenshot_config = options.and_then(|o| {
            o.screenshot_options.as_ref().map(|so| ScreenshotConfig {
                full_page: so.full_page.unwrap_or(false),
                selector: so.selector.clone(),
                quality: so.quality,
                format: so.format.clone(),
            })
        });

        Ok(ScrapeRequest {
            url: scrape_request.url.clone(),
            options: ScrapeOptions {
                headers,
                timeout: Duration::from_secs(options.and_then(|o| o.timeout).unwrap_or(30)),
                needs_js,
                needs_screenshot: options.and_then(|o| o.screenshot).unwrap_or(false),
                screenshot_config,
                mobile: options.and_then(|o| o.mobile).unwrap_or(false),
                proxy: options.and_then(|o| o.proxy.clone()),
                skip_tls_verification: options
                    .and_then(|o| o.skip_tls_verification)
                    .unwrap_or(false),
                needs_tls_fingerprint: options
                    .and_then(|o| o.needs_tls_fingerprint)
                    .unwrap_or(false),
                use_fire_engine: options.and_then(|o| o.use_fire_engine).unwrap_or(false),
                actions: scrape_request
                    .actions
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|a| match a {
                        crate::application::dto::scrape_request::ScrapeActionDto::Wait {
                            milliseconds,
                        } => Some(PageAction::Wait { milliseconds }),
                        crate::application::dto::scrape_request::ScrapeActionDto::Click {
                            selector,
                        } => Some(PageAction::Click { selector }),
                        crate::application::dto::scrape_request::ScrapeActionDto::Scroll {
                            direction,
                        } => {
                            // Map string direction to ScrollDirection enum
                            let dir = match direction.to_lowercase().as_str() {
                                "up" => ScrollDirection::Up,
                                "top" => ScrollDirection::Top,
                                "bottom" => ScrollDirection::Bottom,
                                _ => ScrollDirection::Down,
                            };
                            Some(PageAction::Scroll { direction: dir })
                        }
                        crate::application::dto::scrape_request::ScrapeActionDto::Screenshot {
                            ..
                        } => {
                            // Screenshot action is handled by global needs_screenshot option
                            None
                        }
                        crate::application::dto::scrape_request::ScrapeActionDto::Input {
                            selector,
                            text,
                        } => Some(PageAction::Input { selector, text }),
                    })
                    .collect(),
                sync_wait_ms: scrape_request.sync_wait_ms.unwrap_or(0),
            },
        })
    }
}

/// ScrapeWorker 构建器
///
/// 使用 Builder 模式简化复杂对象的创建过程
pub struct ScrapeWorkerBuilder<R, S, C, CRR>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    CRR: CreditsRepository + Send + Sync,
{
    repository: Option<Arc<R>>,
    result_repository: Option<Arc<S>>,
    crawl_repository: Option<Arc<C>>,
    storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    webhook_event_repository: Option<Arc<dyn WebhookEventRepository + Send + Sync>>,
    credits_repository: Option<Arc<CRR>>,
    engine_client: Option<Arc<EngineClient>>,
    create_scrape_use_case: Option<Arc<CreateScrapeUseCase>>,
    #[cfg(feature = "redis-cache")]
    redis: Option<RedisClient>,
    robots_checker: Option<Arc<RobotsChecker>>,
    settings: Option<Arc<Settings>>,
    default_concurrency_limit: usize,
}

impl<R, S, C, CRR> Default for ScrapeWorkerBuilder<R, S, C, CRR>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    CRR: CreditsRepository + Send + Sync,
{
    fn default() -> Self {
        Self {
            repository: None,
            result_repository: None,
            crawl_repository: None,
            storage_repository: None,
            webhook_event_repository: None,
            credits_repository: None,
            engine_client: None,
            create_scrape_use_case: None,
            #[cfg(feature = "redis-cache")]
            redis: None,
            robots_checker: None,
            settings: None,
            default_concurrency_limit: 10,
        }
    }
}

impl<R, S, C, CRR> ScrapeWorkerBuilder<R, S, C, CRR>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    CRR: CreditsRepository + Send + Sync,
{
    /// 创建新的构建器
    #[cfg(feature = "redis-cache")]
    pub fn new() -> Self {
        Self {
            repository: None,
            result_repository: None,
            crawl_repository: None,
            storage_repository: None,
            webhook_event_repository: None,
            credits_repository: None,
            engine_client: None,
            create_scrape_use_case: None,
            redis: None,
            robots_checker: None,
            settings: None,
            default_concurrency_limit: 10,
        }
    }

    /// 设置任务仓储 (必需)
    pub fn with_repository(mut self, repository: Arc<R>) -> Self {
        self.repository = Some(repository);
        self
    }

    /// 设置结果仓储 (必需)
    pub fn with_result_repository(mut self, result_repository: Arc<S>) -> Self {
        self.result_repository = Some(result_repository);
        self
    }

    /// 设置爬取仓储 (必需)
    pub fn with_crawl_repository(mut self, crawl_repository: Arc<C>) -> Self {
        self.crawl_repository = Some(crawl_repository);
        self
    }

    /// 设置存储仓储 (可选)
    pub fn with_storage_repository(
        mut self,
        storage_repository: Arc<dyn StorageRepository + Send + Sync>,
    ) -> Self {
        self.storage_repository = Some(storage_repository);
        self
    }

    /// 设置 Webhook 事件仓储 (必需)
    pub fn with_webhook_event_repository(
        mut self,
        webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    ) -> Self {
        self.webhook_event_repository = Some(webhook_event_repository);
        self
    }

    /// 设置积分仓储 (必需)
    pub fn with_credits_repository(mut self, credits_repository: Arc<CRR>) -> Self {
        self.credits_repository = Some(credits_repository);
        self
    }

    /// 设置引擎客户端 (必需)
    pub fn with_engine_client(mut self, engine_client: Arc<EngineClient>) -> Self {
        self.engine_client = Some(engine_client);
        self
    }

    /// 设置创建抓取用例 (必需)
    pub fn with_create_scrape_use_case(
        mut self,
        create_scrape_use_case: Arc<CreateScrapeUseCase>,
    ) -> Self {
        self.create_scrape_use_case = Some(create_scrape_use_case);
        self
    }

    /// 设置 Redis 客户端 (必需)
    #[cfg(feature = "redis-cache")]
    pub fn with_redis(mut self, redis: RedisClient) -> Self {
        self.redis = Some(redis);
        self
    }

    /// 设置 Robots 检查器 (必需)
    pub fn with_robots_checker(mut self, robots_checker: Arc<RobotsChecker>) -> Self {
        self.robots_checker = Some(robots_checker);
        self
    }

    /// 设置配置 (必需)
    pub fn with_settings(mut self, settings: Arc<Settings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// 设置默认并发限制
    pub fn with_default_concurrency_limit(mut self, limit: usize) -> Self {
        self.default_concurrency_limit = limit;
        self
    }

    /// 构建 ScrapeWorker 实例
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "redis-cache")]
    pub fn build(self) -> Result<ScrapeWorker<R, S, C, CRR>, &'static str> {
        let repository = self.repository.ok_or("repository is required")?;
        let result_repository = self
            .result_repository
            .ok_or("result_repository is required")?;
        let crawl_repository = self
            .crawl_repository
            .ok_or("crawl_repository is required")?;
        let webhook_event_repository = self
            .webhook_event_repository
            .ok_or("webhook_event_repository is required")?;
        let credits_repository = self
            .credits_repository
            .ok_or("credits_repository is required")?;
        let engine_client = self.engine_client.ok_or("engine_client is required")?;
        let create_scrape_use_case = self
            .create_scrape_use_case
            .ok_or("create_scrape_use_case is required")?;
        let redis = self.redis.ok_or("redis is required")?;
        let robots_checker = self.robots_checker.ok_or("robots_checker is required")?;
        let settings = self.settings.ok_or("settings is required")?;

        Ok(ScrapeWorker::new(
            repository,
            result_repository,
            crawl_repository,
            self.storage_repository,
            webhook_event_repository,
            credits_repository,
            engine_client,
            create_scrape_use_case,
            redis,
            robots_checker,
            settings,
            self.default_concurrency_limit,
        ))
    }
}
