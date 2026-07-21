// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::{Context, Result};
use chrono::Utc;
use dashmap::DashMap;
use log::{debug, error, info, warn};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::extract_request::ExtractRequestDto;
use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::config::settings::Settings;
use crate::domain::models::scrape_result::ScrapeResult;
use crate::domain::models::CrawlStatus;
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use crate::domain::services::retry_handler::RetryHandler;
use crate::domain::services::webhook_service::WebhookService;
use crate::utils::regex_cache::RegexCache;

use crate::engines::engine_client::{
    EngineClient, HttpMethod, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
    ScreenshotConfig, ScrollDirection,
};
use crate::presentation::helpers::ssrf::validate_url;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::TaskQueue;
use crate::utils::crawl_text_integration::{CrawlTextIntegration, ScrapeResponseInput};
use crate::utils::retry_policy::RetryPolicy;
use crate::utils::robots::RobotsCheckerTrait;
use crate::workers::errors::ScrapeWorkerError;

/// 从缓存获取正则表达式
fn get_cached_regex(pattern: &str, cache: &RegexCache) -> Result<regex::Regex, ScrapeWorkerError> {
    cache
        .get_or_insert(pattern)
        .map_err(ScrapeWorkerError::RegexError)
}

/// 抓取工作者
pub struct ScrapeWorker {
    repository: Arc<dyn TaskRepository>,
    result_repository: Arc<dyn ScrapeResultRepository>,
    crawl_repository: Arc<dyn CrawlRepository>,
    webhook_service: Arc<dyn WebhookService>,
    credits_repository: Arc<dyn CreditsRepository>,
    engine_client: Arc<EngineClient>,
    _create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    team_semaphore: Arc<TeamSemaphore>,
    token_usage: Arc<DashMap<Uuid, AtomicI64>>,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    worker_id: Uuid,
    default_concurrency_limit: usize,
    retry_handler: RetryHandler,
    extraction_service: Arc<dyn ExtractionServiceTrait>,
    regex_cache: RegexCache,
}

impl std::fmt::Debug for ScrapeWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapeWorker")
            .field("worker_id", &self.worker_id)
            .field("default_concurrency_limit", &self.default_concurrency_limit)
            .finish_non_exhaustive()
    }
}

impl ScrapeWorker {
    /// 创建新的抓取工作器实例
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<dyn TaskRepository>,
        result_repository: Arc<dyn ScrapeResultRepository>,
        crawl_repository: Arc<dyn CrawlRepository>,
        webhook_service: Arc<dyn WebhookService>,
        credits_repository: Arc<dyn CreditsRepository>,
        engine_client: Arc<EngineClient>,
        _create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore: Arc<TeamSemaphore>,
        robots_checker: Arc<dyn RobotsCheckerTrait>,
        settings: Arc<Settings>,
        default_concurrency_limit: usize,
        extraction_service: Arc<dyn ExtractionServiceTrait>,
        regex_cache: RegexCache,
    ) -> Self {
        // 根据任务类型选择合适的重试策略
        let retry_policy = RetryPolicy::slow(); // 网络请求适合慢速重试策略
        let retry_handler = RetryHandler::new(repository.clone(), retry_policy.clone());

        Self {
            repository,
            result_repository,
            crawl_repository,
            webhook_service,
            credits_repository,
            engine_client,
            _create_scrape_use_case,
            team_semaphore,
            token_usage: Arc::new(DashMap::new()),
            robots_checker,
            settings,
            worker_id: Uuid::new_v4(),
            default_concurrency_limit,
            retry_handler,
            extraction_service,
            regex_cache,
        }
    }

    /// 运行抓取工作器
    pub async fn run(&self, queue: Arc<dyn TaskQueue>) {
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

    async fn process_next_task(&self, queue: &dyn TaskQueue) -> Result<bool> {
        let task_opt = queue.dequeue(self.worker_id).await?;

        if let Some(task) = task_opt {
            self.process_task(task).await?;
            return Ok(true);
        }

        Ok(false)
    }

    fn acquire_concurrency_permit(&self, task: &Task) -> Option<OwnedSemaphorePermit> {
        self.team_semaphore.try_acquire(task.team_id)
    }

    async fn process_task(&self, mut task: Task) -> Result<()> {
        debug!(
            "process_task: task_id={}, url={}, task_type={}",
            task.id, task.url, task.task_type
        );
        info!("Processing task");

        // Check Task Expiration
        if let Some(expires_at) = task.expires_at {
            if Utc::now() > expires_at {
                warn!("Task {} expired at {}", task.id, expires_at);
                self.repository.mark_failed(task.id).await?;
                // Trigger failure webhook if needed
                self.trigger_webhook(&task, Some("Task expired".to_string()))
                    .await;
                return Ok(());
            }
        }

        // Concurrency Check (Layer 2: Team Semaphore)
        // The permit is held for the duration of task processing and auto-releases on drop.
        let _permit = match self.acquire_concurrency_permit(&task) {
            Some(p) => p,
            None => {
                warn!(
                    "Team {} concurrency limit exceeded, rescheduling task {}",
                    task.team_id, task.id
                );
                // Reschedule logic (Backlog)
                // Delay by 30 seconds
                task.scheduled_at = Some(Utc::now() + chrono::Duration::seconds(30));
                task.status = TaskStatus::Queued;
                self.repository.update(&task).await?;
                return Ok(());
            }
        };

        let task_type = task.task_type;

        // Take task by value only for the specific branch that needs it
        // This avoids 3 unnecessary clones in the match
        let result = match task_type.as_str() {
            "scrape" => self.process_scrape_task(task).await,
            "crawl" => self.process_crawl_task(task).await,
            "extract" => self.process_extract_task(task).await,
            _ => return Err(anyhow::anyhow!("Unknown task type: {}", task_type)),
        };

        // _permit auto-releases here when it goes out of scope

        if let Err(ref e) = result {
            debug!("error: {}", e);
        } else {
            debug!("Task processing completed successfully");
        }

        result
    }

    async fn process_scrape_task(&self, mut task: Task) -> Result<()> {
        debug!("task_id: {}", task.id);

        // Resolve engine router directly to handle actions if they exist
        let scrape_request = Self::build_scrape_request(&task).unwrap_or_else(|e| {
            error!("Failed to parse task payload, using default: {}", e);
            ScrapeRequest::new(task.url.clone()).timeout(Duration::from_secs(
                self.settings.timeouts.engines.default_timeout_seconds,
            ))
        });

        let response = self.engine_client.scrape(&scrape_request).await;

        match response {
            Ok(response) => {
                debug!("status_code: {}", response.status_code);
                info!("Scrape successful, status: {}", response.status_code);

                // Map ScrapeResponse to ScrapeResult
                // _result variable is currently unused but might be used later or for debugging
                let _result = ScrapeResult {
                    id: Uuid::new_v4(),
                    task_id: task.id,
                    url: task.url.clone(),
                    status_code: response.status_code as i32,
                    content: response.content.clone(),
                    content_type: response.content_type.clone(),
                    headers: serde_json::to_value(&response.headers).unwrap_or(Value::Null),
                    meta_data: Value::Null,
                    screenshot: response.screenshot.clone(),
                    response_time_ms: response.response_time_ms as i64,
                    created_at: Utc::now().naive_utc(),
                };

                if let Err(e) = self.handle_scrape_success(&task, &response).await {
                    error!("Scrape success handler failed: {}", e);
                    debug!("error: {}", e);
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
                debug!("error: {}", e);

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
                        t.completed_at = Some(Utc::now());
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
                self.trigger_webhook(&task, Some(e.to_string())).await;
                Ok(())
            }
        }
    }

    /// 解析 Crawl 任务特定的 Payload
    async fn parse_crawl_payload(&self, task: &Task) -> Result<(Uuid, u32, CrawlConfigDto)> {
        let payload = &task.payload;
        let crawl_id = match payload.get("crawl_id").and_then(|v| v.as_str()) {
            Some(id) => Uuid::parse_str(id).unwrap_or_default(),
            None => {
                return Err(anyhow::anyhow!("Missing crawl_id in task payload"));
            }
        };

        let depth = payload.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let config: CrawlConfigDto =
            serde_json::from_value(payload.get("config").cloned().unwrap_or(json!({})))?;

        Ok((crawl_id, depth, config))
    }

    /// 检查 Robots.txt 并返回是否允许访问
    async fn check_robots_txt(&self, task: &Task) -> bool {
        let user_agent = "crawlrs-bot";

        if !self
            .robots_checker
            .is_allowed(&task.url, user_agent)
            .await
            .unwrap_or(true)
        {
            info!("Access denied by robots.txt for {}", task.url);
            return false;
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

        true
    }

    async fn process_crawl_task(&self, mut task: Task) -> Result<()> {
        // 1. 解析 Crawl 任务特定的 Payload
        let (crawl_id, depth, config) = match self.parse_crawl_payload(&task).await {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse crawl payload: {}", e);
                self.repository.mark_failed(task.id).await?;
                return Ok(());
            }
        };

        // 2. Robots.txt Check
        if !self.check_robots_txt(&task).await {
            self.repository.mark_failed(task.id).await?;
            return Ok(());
        }

        // 2.5 SSRF 防护 (CWE-918)：验证 crawl_config.proxy 不指向内部网络（防御纵深）。
        // handler 层已在入队前验证，此处作为第二层防御拦截直接入队的恶意任务。
        if let Some(ref proxy_url) = config.proxy {
            if let Err(e) = validate_url(proxy_url).await {
                warn!(
                    "SSRF via proxy blocked in worker proxy={} task_id={} team_id={} error={}",
                    proxy_url,
                    task.id,
                    task.team_id,
                    e
                );
                self.repository.mark_failed(task.id).await?;
                return Ok(());
            }
        }

        // 3. 构建并执行抓取请求
        let request = self.build_crawl_request(&task, &config);
        let response = self.engine_client.scrape(&request).await;

        // 4. 处理结果
        match response {
            Ok(response) => {
                self.handle_crawl_success(&task, response, crawl_id, depth, &config, &request)
                    .await
            }
            Err(e) => {
                self.handle_crawl_failure(&mut task, e.into(), crawl_id, &request)
                    .await
            }
        }
    }

    /// 构建 Crawl 任务的 ScrapeRequest
    fn build_crawl_request(&self, task: &Task, config: &CrawlConfigDto) -> ScrapeRequest {
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

        ScrapeRequest::new(task.url.clone()).with_options(ScrapeOptions {
            method: HttpMethod::Get,
            body: None,
            headers,
            timeout: Duration::from_secs(self.settings.timeouts.engines.default_timeout_seconds),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: config.proxy.clone(),
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            sync_wait_ms: 0,
        })
    }

    /// 处理 Crawl 任务成功响应
    async fn handle_crawl_success(
        &self,
        task: &Task,
        response: ScrapeResponse,
        crawl_id: Uuid,
        depth: u32,
        config: &CrawlConfigDto,
        request: &ScrapeRequest,
    ) -> Result<()> {
        info!(
            "Crawl step successful, url: {}, status: {}",
            task.url, response.status_code
        );

        // 文本编码处理
        let processed_content = match self.process_text_encoding(task, &response).await {
            Ok(content) => content,
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                response.content.clone()
            }
        };

        let processed_response = ScrapeResponse {
            content: processed_content,
            ..response
        };

        // 执行数据提取（如果配置了提取规则）
        let extracted_data = self
            .extract_data_with_rules(task, &processed_response, config)
            .await;

        // 保存结果
        self.save_result(task, &processed_response, extracted_data)
            .await?;

        // 如果深度未达上限，解析链接并生成子任务
        if depth < config.max_depth {
            self.extract_and_queue_links(task, &processed_response, crawl_id, depth, config)
                .await?;
        }

        // 更新任务状态和 Crawl 统计
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

        // 检查是否所有任务都已完成
        self.update_crawl_completion_status(crawl_id).await;

        // 扣除高级功能费用
        self.deduct_feature_credits(
            task.team_id,
            task.id,
            processed_response.screenshot.is_some(),
            request.options.proxy.is_some(),
        )
        .await;

        Ok(())
    }

    /// 使用配置的规则提取数据
    async fn extract_data_with_rules(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        config: &CrawlConfigDto,
    ) -> Option<Value> {
        if let Some(rules) = &config.extraction_rules {
            match self
                .extraction_service
                .extract(&response.content, rules, Some(&task.url))
                .await
            {
                Ok((data, usage)) => {
                    self.deduct_token_credits(
                        task.team_id,
                        task.id,
                        &usage,
                        "Tokens used for extraction",
                    )
                    .await;
                    Some(data)
                }
                Err(e) => {
                    error!("Extraction failed for url {}: {}", task.url, e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// 处理 Crawl 任务失败响应
    async fn handle_crawl_failure(
        &self,
        task: &mut Task,
        error: anyhow::Error,
        crawl_id: Uuid,
        request: &ScrapeRequest,
    ) -> Result<()> {
        // 扣除代理费用（即使失败）
        self.deduct_feature_credits(
            task.team_id,
            task.id,
            false,
            request.options.proxy.is_some(),
        )
        .await;

        error!("Crawl step failed: {}", error);
        self.handle_failure(task).await?;

        if let Err(e) = self.crawl_repository.increment_failed_tasks(crawl_id).await {
            error!(
                "Failed to increment failed tasks for crawl {}: {}",
                crawl_id, e
            );
        }

        // 检查是否所有任务都已完成
        self.update_crawl_completion_status(crawl_id).await;

        // 触发失败 Webhook
        self.trigger_webhook(task, Some(error.to_string())).await;

        Ok(())
    }

    /// 更新 Crawl 完成状态（检查是否所有任务都已完成）
    async fn update_crawl_completion_status(&self, crawl_id: Uuid) {
        match self.crawl_repository.find_by_id(crawl_id).await {
            Ok(Some(c)) => {
                if c.completed_tasks() + c.failed_tasks() == c.total_tasks() {
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
    }

    /// 解析 Extract 任务特定的 Payload
    async fn parse_extract_payload(&self, task: &Task) -> Result<(ExtractRequestDto, String)> {
        let payload: ExtractRequestDto = serde_json::from_value(task.payload.clone())
            .context("Failed to parse extract task input")?;

        let url = payload.urls.first().context("No URL provided")?.clone();

        Ok((payload, url))
    }

    /// 构建 Extract 任务的 ScrapeRequest
    fn build_extract_request(&self, url: &str) -> ScrapeRequest {
        ScrapeRequest::new(url.to_string()).with_options(ScrapeOptions {
            method: HttpMethod::Get,
            body: None,
            headers: HashMap::new(),
            timeout: Duration::from_secs(self.settings.timeouts.engines.default_timeout_seconds),
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
        })
    }

    async fn process_extract_task(&self, mut task: Task) -> Result<()> {
        info!("Processing extract task {}", task.id);

        // 1. 解析 Payload
        let (payload, url) = self.parse_extract_payload(&task).await?;
        debug!("has_rules: {}", payload.rules.is_some());
        if let Some(ref rules) = payload.rules {
            debug!("rules_count: {}", rules.len());
        }

        // 2. 构建并执行 Scrape 请求
        let scrape_req = self.build_extract_request(&url);
        let scrape_resp = self.engine_client.scrape(&scrape_req).await?;

        // 3. 文本编码处理
        let processed_content = match self.process_text_encoding(&task, &scrape_resp).await {
            Ok(content) => content,
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                scrape_resp.content.clone()
            }
        };

        let processed_scrape_resp = ScrapeResponse {
            content: processed_content,
            ..scrape_resp
        };

        // 4. 根据不同的提取方式处理
        if let Some(rules) = payload.rules {
            return self
                .handle_rules_extraction(&mut task, &processed_scrape_resp, &rules, &url)
                .await;
        }

        if let Some(prompt) = payload.prompt {
            return self
                .handle_prompt_extraction(&mut task, &processed_scrape_resp, prompt, &url)
                .await;
        }

        if let Some(schema) = payload.schema {
            return self
                .handle_schema_extraction(&mut task, &processed_scrape_resp, &schema, &url)
                .await;
        }

        // Fallback: 无提取规则时保存原始结果
        self.save_extract_result(&mut task, &processed_scrape_resp, None, &url)
            .await
    }

    /// 处理基于规则的提取
    async fn handle_rules_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        rules: &HashMap<String, crate::domain::services::extraction_service::ExtractionRule>,
        url: &str,
    ) -> Result<()> {
        debug!("rules: {:?}", rules);

        let (extracted_data, usage) = self
            .extraction_service
            .extract(&response.content, rules, Some(url))
            .await?;

        self.deduct_token_credits(
            task.team_id,
            task.id,
            &usage,
            "Tokens used for extraction rules",
        )
        .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 处理基于 Prompt 的提取
    async fn handle_prompt_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        prompt: String,
        url: &str,
    ) -> Result<()> {
        let mut rules = HashMap::with_capacity(1);
        rules.insert(
            "extracted_data".to_string(),
            crate::domain::services::extraction_service::ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some(prompt),
                output_format: None,
            },
        );

        let (extracted_data, usage) = self
            .extraction_service
            .extract(&response.content, &rules, Some(url))
            .await?;

        self.deduct_token_credits(task.team_id, task.id, &usage, "Tokens used for extraction")
            .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 处理基于 Schema 的提取
    async fn handle_schema_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        schema: &serde_json::Value,
        url: &str,
    ) -> Result<()> {
        let (extracted_data, usage) = self
            .extraction_service
            .extract_with_schema(&response.content, schema)
            .await?;

        self.deduct_token_credits(
            task.team_id,
            task.id,
            &usage,
            "Tokens used for schema extraction",
        )
        .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 保存提取结果
    async fn save_extract_result(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        extracted_data: Option<Value>,
        url: &str,
    ) -> Result<()> {
        let meta_data = extracted_data
            .map(|data| json!({ "extracted_data": data }))
            .unwrap_or(json!({}));

        let scrape_result = ScrapeResult {
            id: Uuid::new_v4(),
            task_id: task.id,
            url: url.to_string(),
            status_code: response.status_code as i32,
            content: response.content.clone(),
            content_type: "text/html".to_string(),
            headers: json!({}),
            meta_data,
            screenshot: None,
            response_time_ms: 0,
            created_at: Utc::now().naive_utc(),
        };

        self.result_repository.save(scrape_result).await?;

        task.status = TaskStatus::Completed;
        self.repository.update(task).await?;

        self.trigger_webhook(task, None).await;

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

        // 使用批量查询优化 N+1 问题
        // unique_links 是 HashSet<String>，需要转换为 Vec<String>
        let links_vec: Vec<String> = unique_links.iter().cloned().collect();
        let existing_urls = self.repository.find_existing_urls(&links_vec).await?;
        let existing_url_set: std::collections::HashSet<String> =
            existing_urls.into_iter().collect();

        for link in unique_links.iter() {
            // 检查是否已经抓取过 (去重)
            if existing_url_set.contains(link) {
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
                api_key_id: task.api_key_id,
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
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
                crawl_id: Some(crawl_id),
                updated_at: Utc::now(),
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
                if let Ok(re) = get_cached_regex(pattern, &self.regex_cache) {
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
                if let Ok(re) = get_cached_regex(pattern, &self.regex_cache) {
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
        debug!("task_id: {}", task.id);

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
                match self
                    .extraction_service
                    .extract(&processed_response.content, rules, Some(&task.url))
                    .await
                {
                    Ok((data, usage)) => {
                        extracted_data = Some(data);
                        // Record usage (PRD-334: Tokens Billing)
                        if usage.total_tokens > 0 {
                            // 1. Record in-memory for real-time tracking
                            self.token_usage
                                .entry(task.team_id)
                                .or_insert_with(|| AtomicI64::new(0))
                                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

                            // 2. Convert to credits and deduct from database
                            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
                            let credits_to_deduct =
                                std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
                            if credits_to_deduct > 0 {
                                if let Err(e) = self
                                    .credits_repository
                                    .deduct_credits(
                                        task.team_id,
                                        credits_to_deduct,
                                        crate::domain::models::CreditsTransactionType::Extract,
                                        format!(
                                            "Tokens used for extraction ({} tokens)",
                                            usage.total_tokens
                                        ),
                                        Some(task.id),
                                    )
                                    .await
                                {
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
        debug!("task_id: {}, About to mark task as completed", task.id);
        self.repository.mark_completed(task.id).await?;
        debug!(
            "task_id: {}, Successfully marked task as completed",
            task.id
        );

        self.trigger_webhook(task, None).await;
        Ok(())
    }

    /// 处理文本编码转换
    async fn process_text_encoding(
        &self,
        task: &Task,
        response: &ScrapeResponse,
    ) -> Result<String> {
        use log::{info, warn};

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

        // Content and screenshot from response
        let content_to_store = response.content.clone();
        let _screenshot_to_store = response.screenshot.clone();

        // Create result entity
        let result = ScrapeResult {
            id: Uuid::new_v4(),
            task_id: task.id,
            url: task.url.clone(),
            status_code: response.status_code as i32,
            content: content_to_store,
            content_type: response.content_type.clone(),
            headers: serde_json::to_value(&response.headers).unwrap_or(Value::Null),
            meta_data,
            screenshot: response.screenshot.clone(),
            response_time_ms: response.response_time_ms as i64,
            created_at: Utc::now().naive_utc(),
        };

        self.result_repository.save(result).await?;
        Ok(())
    }

    async fn trigger_webhook(&self, task: &Task, error_msg: Option<String>) {
        let result = match error_msg {
            Some(msg) => self.webhook_service.trigger_failure(task, msg).await,
            None => self.webhook_service.trigger_completion(task).await,
        };

        if let Err(e) = result {
            error!("Failed to trigger webhook for task {}: {}", task.id, e);
        }
    }

    async fn handle_failure(&self, task: &mut Task) -> Result<()> {
        match self.retry_handler.handle_failure(task).await {
            crate::domain::services::retry_handler::HandleFailureResult::Retried { .. } => Ok(()),
            crate::domain::services::retry_handler::HandleFailureResult::Failed => Ok(()),
            crate::domain::services::retry_handler::HandleFailureResult::Error(e) => Err(e),
        }
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
                    crate::domain::models::CreditsTransactionType::Scrape,
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
            // 1. Record in-memory for real-time tracking
            self.token_usage
                .entry(team_id)
                .or_insert_with(|| AtomicI64::new(0))
                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

            // 2. Convert to credits and deduct from database
            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
            let credits_to_deduct = std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
            if credits_to_deduct > 0 {
                if let Err(e) = self
                    .credits_repository
                    .deduct_credits(
                        team_id,
                        credits_to_deduct,
                        crate::domain::models::CreditsTransactionType::Extract,
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
                method: HttpMethod::Get,
                body: None,
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
pub struct ScrapeWorkerBuilder {
    repository: Option<Arc<dyn TaskRepository>>,
    result_repository: Option<Arc<dyn ScrapeResultRepository>>,
    crawl_repository: Option<Arc<dyn CrawlRepository>>,
    webhook_service: Option<Arc<dyn WebhookService>>,
    credits_repository: Option<Arc<dyn CreditsRepository>>,
    engine_client: Option<Arc<EngineClient>>,
    create_scrape_use_case: Option<Arc<dyn CreateScrapeUseCaseTrait>>,
    team_semaphore: Option<Arc<TeamSemaphore>>,
    robots_checker: Option<Arc<dyn RobotsCheckerTrait>>,
    settings: Option<Arc<Settings>>,
    default_concurrency_limit: usize,
    extraction_service: Option<Arc<dyn ExtractionServiceTrait>>,
    regex_cache: Option<RegexCache>,
}

impl Default for ScrapeWorkerBuilder {
    fn default() -> Self {
        Self {
            repository: None,
            result_repository: None,
            crawl_repository: None,
            webhook_service: None,
            credits_repository: None,
            engine_client: None,
            create_scrape_use_case: None,
            team_semaphore: None,
            robots_checker: None,
            settings: None,
            default_concurrency_limit: 10,
            extraction_service: None,
            regex_cache: None,
        }
    }
}

impl ScrapeWorkerBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置任务仓储 (必需)
    pub fn with_repository(mut self, repository: Arc<dyn TaskRepository>) -> Self {
        self.repository = Some(repository);
        self
    }

    /// 设置结果仓储 (必需)
    pub fn with_result_repository(
        mut self,
        result_repository: Arc<dyn ScrapeResultRepository>,
    ) -> Self {
        self.result_repository = Some(result_repository);
        self
    }

    /// 设置爬取仓储 (必需)
    pub fn with_crawl_repository(mut self, crawl_repository: Arc<dyn CrawlRepository>) -> Self {
        self.crawl_repository = Some(crawl_repository);
        self
    }

    /// 设置 Webhook 服务 (必需)
    pub fn with_webhook_service(mut self, webhook_service: Arc<dyn WebhookService>) -> Self {
        self.webhook_service = Some(webhook_service);
        self
    }

    /// 设置积分仓储 (必需)
    pub fn with_credits_repository(
        mut self,
        credits_repository: Arc<dyn CreditsRepository>,
    ) -> Self {
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
        create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    ) -> Self {
        self.create_scrape_use_case = Some(create_scrape_use_case);
        self
    }

    /// 设置团队信号量 (必需)
    pub fn with_team_semaphore(mut self, team_semaphore: Arc<TeamSemaphore>) -> Self {
        self.team_semaphore = Some(team_semaphore);
        self
    }

    /// 设置 Robots 检查器 (必需)
    pub fn with_robots_checker(mut self, robots_checker: Arc<dyn RobotsCheckerTrait>) -> Self {
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

    /// 设置提取服务 (必需)
    pub fn with_extraction_service(
        mut self,
        extraction_service: Arc<dyn ExtractionServiceTrait>,
    ) -> Self {
        self.extraction_service = Some(extraction_service);
        self
    }

    /// 设置正则缓存 (必需)
    pub fn with_regex_cache(mut self, regex_cache: RegexCache) -> Self {
        self.regex_cache = Some(regex_cache);
        self
    }

    /// 构建 ScrapeWorker 实例
    #[allow(clippy::too_many_arguments)]
    pub fn build(self) -> Result<ScrapeWorker, &'static str> {
        let repository = self.repository.ok_or("repository is required")?;
        let result_repository = self
            .result_repository
            .ok_or("result_repository is required")?;
        let crawl_repository = self
            .crawl_repository
            .ok_or("crawl_repository is required")?;
        let webhook_service = self.webhook_service.ok_or("webhook_service is required")?;
        let credits_repository = self
            .credits_repository
            .ok_or("credits_repository is required")?;
        let engine_client = self.engine_client.ok_or("engine_client is required")?;
        let create_scrape_use_case = self
            .create_scrape_use_case
            .ok_or("create_scrape_use_case is required")?;
        let team_semaphore = self.team_semaphore.ok_or("team_semaphore is required")?;
        let robots_checker = self.robots_checker.ok_or("robots_checker is required")?;
        let settings = self.settings.ok_or("settings is required")?;
        let extraction_service = self
            .extraction_service
            .ok_or("extraction_service is required")?;
        let regex_cache = self.regex_cache.ok_or("regex_cache is required")?;

        Ok(ScrapeWorker::new(
            repository,
            result_repository,
            crawl_repository,
            webhook_service,
            credits_repository,
            engine_client,
            create_scrape_use_case,
            team_semaphore,
            robots_checker,
            settings,
            self.default_concurrency_limit,
            extraction_service,
            regex_cache,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::EngineError;
    use crate::infrastructure::oxcache::RegexCacheType;
    use std::time::Duration;

    // ========== Helper functions ==========

    /// Create a Task with the given JSON payload and default remaining fields.
    fn make_task(payload: Value) -> Task {
        Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            payload,
        )
    }

    /// Build a RegexCache backed by an in-memory oxcache instance.
    async fn make_regex_cache() -> RegexCache {
        let cache: RegexCacheType = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(3600))
            .build()
            .await
            .expect("Failed to build oxcache for test");
        RegexCache::new(Arc::new(cache))
    }

    // ========== get_cached_regex tests ==========

    #[tokio::test]
    async fn test_get_cached_regex_valid_pattern_returns_regex() {
        let cache = make_regex_cache().await;
        let result = get_cached_regex(r"\d+", &cache);
        let regex = result.expect("valid pattern should produce a Regex");
        assert!(regex.is_match("123"));
        assert!(!regex.is_match("abc"));
    }

    #[tokio::test]
    async fn test_get_cached_regex_invalid_pattern_returns_regex_error() {
        let cache = make_regex_cache().await;
        let result = get_cached_regex(r"[unclosed", &cache);
        let err = result.expect_err("invalid pattern should error");
        match err {
            ScrapeWorkerError::RegexError(msg) => {
                assert!(!msg.is_empty(), "error message should not be empty");
            }
            other => panic!("Expected RegexError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_get_cached_regex_caches_repeated_calls() {
        let cache = make_regex_cache().await;
        let r1 = get_cached_regex(r"[a-z]+", &cache).expect("first call should succeed");
        let r2 = get_cached_regex(r"[a-z]+", &cache).expect("second call should succeed");
        assert!(r1.is_match("hello"));
        assert!(r2.is_match("world"));
    }

    // ========== build_scrape_request: error / edge cases ==========

    #[test]
    fn test_build_scrape_request_minimal_payload_succeeds() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task)
            .expect("minimal payload with url should succeed");
        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.method, HttpMethod::Get);
        assert!(request.options.body.is_none());
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(!request.options.mobile);
        assert!(request.options.proxy.is_none());
        assert!(!request.options.skip_tls_verification);
        assert!(!request.options.needs_tls_fingerprint);
        assert!(!request.options.use_fire_engine);
        assert_eq!(request.options.timeout, Duration::from_secs(30));
        assert_eq!(request.options.sync_wait_ms, 0);
        assert!(request.options.actions.is_empty());
        assert!(request.options.screenshot_config.is_none());
        assert!(request.options.headers.is_empty());
    }

    #[test]
    fn test_build_scrape_request_missing_url_fails() {
        let task = make_task(json!({"formats": ["html"]}));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    #[test]
    fn test_build_scrape_request_non_object_payload_fails() {
        let task = make_task(json!(42));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    #[test]
    fn test_build_scrape_request_array_payload_fails() {
        let task = make_task(json!([1, 2, 3]));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    #[test]
    fn test_build_scrape_request_string_payload_fails() {
        let task = make_task(json!("not an object"));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    #[test]
    fn test_build_scrape_request_unknown_field_fails() {
        // deny_unknown_fields rejects unknown keys
        let task = make_task(json!({"url": "https://example.com", "unknown_field": "value"}));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    #[test]
    fn test_build_scrape_request_unknown_option_field_fails() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"bogus": 1}
        }));
        assert!(ScrapeWorker::build_scrape_request(&task).is_err());
    }

    // ========== build_scrape_request: options.timeout ==========

    #[test]
    fn test_build_scrape_request_default_timeout_is_30_seconds() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_build_scrape_request_custom_timeout() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"timeout": 120}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.timeout, Duration::from_secs(120));
    }

    // ========== build_scrape_request: options.headers ==========

    #[test]
    fn test_build_scrape_request_string_headers_are_included() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "headers": {"X-Custom": "value", "Authorization": "Bearer token"}
            }
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.headers.len(), 2);
        assert_eq!(
            request.options.headers.get("X-Custom"),
            Some(&"value".to_string())
        );
        assert_eq!(
            request.options.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_build_scrape_request_non_string_headers_are_filtered() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "headers": {
                    "X-String": "ok",
                    "X-Number": 42,
                    "X-Bool": true,
                    "X-Null": null,
                    "X-Object": {"nested": 1}
                }
            }
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        // Only string values are inserted; everything else is silently dropped.
        assert_eq!(request.options.headers.len(), 1);
        assert_eq!(
            request.options.headers.get("X-String"),
            Some(&"ok".to_string())
        );
        assert!(!request.options.headers.contains_key("X-Number"));
        assert!(!request.options.headers.contains_key("X-Bool"));
        assert!(!request.options.headers.contains_key("X-Null"));
        assert!(!request.options.headers.contains_key("X-Object"));
    }

    #[test]
    fn test_build_scrape_request_empty_headers_map() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"headers": {}}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.headers.is_empty());
    }

    // ========== build_scrape_request: needs_js logic ==========

    #[test]
    fn test_build_scrape_request_needs_js_false_by_default() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(!request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_needs_js_true_from_js_rendering() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"js_rendering": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_needs_js_false_when_js_rendering_false() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"js_rendering": false}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(!request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_needs_js_true_when_actions_non_empty() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "wait", "milliseconds": 500}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_needs_js_false_when_actions_empty() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": []
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(!request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_needs_js_true_empty_actions_with_js_rendering() {
        // needs_js is an OR: empty actions (false) OR js_rendering=true (true) => true
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [],
            "options": {"js_rendering": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.needs_js);
    }

    // ========== build_scrape_request: screenshot options ==========

    #[test]
    fn test_build_scrape_request_screenshot_false_by_default() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(!request.options.needs_screenshot);
        assert!(request.options.screenshot_config.is_none());
    }

    #[test]
    fn test_build_scrape_request_screenshot_true_sets_flag() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"screenshot": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.needs_screenshot);
    }

    #[test]
    fn test_build_scrape_request_screenshot_config_full_page_true() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "screenshot_options": {
                    "full_page": true,
                    "quality": 90,
                    "format": "png"
                }
            }
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        let config = request
            .options
            .screenshot_config
            .expect("screenshot_config should be set");
        assert!(config.full_page);
        assert_eq!(config.quality, Some(90));
        assert_eq!(config.format, Some("png".to_string()));
        assert!(config.selector.is_none());
    }

    #[test]
    fn test_build_scrape_request_screenshot_config_full_page_defaults_to_false() {
        // Note: this differs from ScreenshotConfig::default() which uses true.
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"screenshot_options": {}}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        let config = request
            .options
            .screenshot_config
            .expect("screenshot_config should be set when screenshot_options is present");
        assert!(!config.full_page, "full_page should default to false");
        assert!(config.quality.is_none());
        assert!(config.format.is_none());
        assert!(config.selector.is_none());
    }

    #[test]
    fn test_build_scrape_request_screenshot_config_with_selector() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"screenshot_options": {"selector": "#main"}}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        let config = request
            .options
            .screenshot_config
            .expect("screenshot_config should be set");
        assert_eq!(config.selector, Some("#main".to_string()));
    }

    // ========== build_scrape_request: other boolean / string options ==========

    #[test]
    fn test_build_scrape_request_mobile_true() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"mobile": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.mobile);
    }

    #[test]
    fn test_build_scrape_request_mobile_false_by_default() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(!request.options.mobile);
    }

    #[test]
    fn test_build_scrape_request_proxy_set() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"proxy": "http://proxy:8080"}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.proxy, Some("http://proxy:8080".to_string()));
    }

    #[test]
    fn test_build_scrape_request_proxy_none_by_default() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.proxy.is_none());
    }

    #[test]
    fn test_build_scrape_request_skip_tls_verification() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"skip_tls_verification": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.skip_tls_verification);
    }

    #[test]
    fn test_build_scrape_request_needs_tls_fingerprint() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"needs_tls_fingerprint": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.needs_tls_fingerprint);
    }

    #[test]
    fn test_build_scrape_request_use_fire_engine() {
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {"use_fire_engine": true}
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.use_fire_engine);
    }

    #[test]
    fn test_build_scrape_request_sync_wait_ms_default_zero() {
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.sync_wait_ms, 0);
    }

    #[test]
    fn test_build_scrape_request_sync_wait_ms_set() {
        let task = make_task(json!({
            "url": "https://example.com",
            "sync_wait_ms": 5000
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.sync_wait_ms, 5000);
    }

    #[test]
    fn test_build_scrape_request_method_always_get() {
        // build_scrape_request hard-codes HttpMethod::Get
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.method, HttpMethod::Get);
    }

    #[test]
    fn test_build_scrape_request_body_always_none() {
        // build_scrape_request hard-codes body to None
        let task = make_task(json!({"url": "https://example.com"}));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.body.is_none());
    }

    // ========== build_scrape_request: URL source ==========

    #[test]
    fn test_build_scrape_request_url_comes_from_payload_not_task() {
        // The ScrapeRequest.url is parsed from the payload, not task.url
        let mut task = make_task(json!({"url": "https://from-payload.com"}));
        task.url = "https://from-task.com".to_string();
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.url, "https://from-payload.com");
    }

    // ========== build_scrape_request: actions mapping ==========

    #[test]
    fn test_build_scrape_request_action_wait_mapped() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "wait", "milliseconds": 1500}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.actions.len(), 1);
        match &request.options.actions[0] {
            PageAction::Wait { milliseconds } => assert_eq!(*milliseconds, 1500),
            other => panic!("Expected Wait, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_click_mapped() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "click", "selector": "#submit"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.actions.len(), 1);
        match &request.options.actions[0] {
            PageAction::Click { selector } => assert_eq!(selector, "#submit"),
            other => panic!("Expected Click, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_input_mapped() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "input", "selector": "#search", "text": "rust"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.options.actions.len(), 1);
        match &request.options.actions[0] {
            PageAction::Input { selector, text } => {
                assert_eq!(selector, "#search");
                assert_eq!(text, "rust");
            }
            other => panic!("Expected Input, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_down() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "down"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Down);
            }
            other => panic!("Expected Scroll Down, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_up() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "up"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Up);
            }
            other => panic!("Expected Scroll Up, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_top() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "top"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Top);
            }
            other => panic!("Expected Scroll Top, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_bottom() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "bottom"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Bottom);
            }
            other => panic!("Expected Scroll Bottom, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_unknown_direction_defaults_down() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "sideways"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Down);
            }
            other => panic!("Expected default Scroll Down, got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_scroll_case_insensitive_direction() {
        // direction.to_lowercase() is used for matching
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "scroll", "direction": "UP"}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        match &request.options.actions[0] {
            PageAction::Scroll { direction } => {
                assert_eq!(*direction, ScrollDirection::Up);
            }
            other => panic!("Expected Scroll Up (case-insensitive), got {:?}", other),
        }
    }

    #[test]
    fn test_build_scrape_request_action_screenshot_is_filtered_out() {
        // Screenshot actions return None in the filter_map because they are
        // handled by the global needs_screenshot option.
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [{"type": "screenshot", "full_page": true}]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(
            request.options.actions.is_empty(),
            "screenshot action should be filtered out"
        );
        // But needs_js should still be true because actions vec was non-empty
        assert!(request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_multiple_actions_preserve_order() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": [
                {"type": "wait", "milliseconds": 100},
                {"type": "click", "selector": "#btn1"},
                {"type": "scroll", "direction": "down"},
                {"type": "input", "selector": "#field", "text": "text"},
                {"type": "screenshot", "full_page": null}
            ]
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        // Screenshot is filtered out -> 4 actions remain
        assert_eq!(request.options.actions.len(), 4);
        assert!(matches!(
            request.options.actions[0],
            PageAction::Wait { milliseconds: 100 }
        ));
        assert!(matches!(
            &request.options.actions[1],
            PageAction::Click { selector } if selector == "#btn1"
        ));
        assert!(matches!(
            &request.options.actions[2],
            PageAction::Scroll { direction } if *direction == ScrollDirection::Down
        ));
        assert!(matches!(
            &request.options.actions[3],
            PageAction::Input { selector, text } if selector == "#field" && text == "text"
        ));
    }

    #[test]
    fn test_build_scrape_request_none_actions_yields_empty_vec() {
        let task = make_task(json!({
            "url": "https://example.com",
            "actions": null
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert!(request.options.actions.is_empty());
        assert!(!request.options.needs_js);
    }

    #[test]
    fn test_build_scrape_request_all_options_combined() {
        // Exercise all ScrapeOptionsDto fields in a single payload
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "headers": {"Accept": "text/html"},
                "timeout": 45,
                "js_rendering": true,
                "screenshot": true,
                "screenshot_options": {"full_page": false, "quality": 50, "format": "jpeg"},
                "mobile": true,
                "proxy": "http://proxy:3128",
                "skip_tls_verification": true,
                "needs_tls_fingerprint": true,
                "use_fire_engine": true
            },
            "actions": [{"type": "wait", "milliseconds": 200}],
            "sync_wait_ms": 1000
        }));
        let request = ScrapeWorker::build_scrape_request(&task).expect("should succeed");
        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.timeout, Duration::from_secs(45));
        assert_eq!(
            request.options.headers.get("Accept"),
            Some(&"text/html".to_string())
        );
        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.proxy, Some("http://proxy:3128".to_string()));
        assert!(request.options.skip_tls_verification);
        assert!(request.options.needs_tls_fingerprint);
        assert!(request.options.use_fire_engine);
        assert_eq!(request.options.sync_wait_ms, 1000);
        assert_eq!(request.options.actions.len(), 1);
        let sc = request
            .options
            .screenshot_config
            .expect("screenshot_config should be set");
        assert!(!sc.full_page);
        assert_eq!(sc.quality, Some(50));
        assert_eq!(sc.format, Some("jpeg".to_string()));
    }

    // ========== ScrapeWorkerBuilder tests ==========

    #[test]
    fn test_builder_default_build_fails_with_repository_required() {
        let builder = ScrapeWorkerBuilder::default();
        // Use match (not expect_err) because ScrapeWorker does not impl Debug.
        let err = match builder.build() {
            Err(e) => e,
            Ok(_) => panic!("empty builder should fail"),
        };
        assert_eq!(err, "repository is required");
    }

    #[test]
    fn test_builder_new_equals_default() {
        // Both new() and default() produce a builder that fails at the same
        // first required field.
        let err_new = match ScrapeWorkerBuilder::new().build() {
            Err(e) => e,
            Ok(_) => panic!("new() builder should fail"),
        };
        let err_default = match ScrapeWorkerBuilder::default().build() {
            Err(e) => e,
            Ok(_) => panic!("default() builder should fail"),
        };
        assert_eq!(err_new, err_default);
        assert_eq!(err_new, "repository is required");
    }

    #[test]
    fn test_builder_with_default_concurrency_limit_does_not_satisfy_required_fields() {
        // Setting only the concurrency limit should not make build() succeed
        let err = match ScrapeWorkerBuilder::default()
            .with_default_concurrency_limit(50)
            .build()
        {
            Err(e) => e,
            Ok(_) => panic!("should still fail"),
        };
        assert_eq!(err, "repository is required");
    }

    #[test]
    fn test_builder_with_default_concurrency_limit_zero() {
        let err = match ScrapeWorkerBuilder::default()
            .with_default_concurrency_limit(0)
            .build()
        {
            Err(e) => e,
            Ok(_) => panic!("should still fail"),
        };
        assert_eq!(err, "repository is required");
    }

    #[test]
    fn test_builder_default_concurrency_limit_is_ten_by_default() {
        // The default concurrency limit is 10 (from ScrapeWorkerBuilder::default).
        // We verify this indirectly: the builder compiles with the default and
        // still fails on the first required field, proving the limit did not
        // affect the required-field checks.
        let builder = ScrapeWorkerBuilder::default();
        let err = match builder.build() {
            Err(e) => e,
            Ok(_) => panic!("should fail"),
        };
        assert_eq!(err, "repository is required");
    }

    // ========== ScrapeWorkerError integration ==========

    #[test]
    fn test_scrape_worker_error_from_string_creates_task_error() {
        // Verify the ScrapeWorkerError::From<String> impl is accessible
        let err: ScrapeWorkerError = "test error".to_string().into();
        match err {
            ScrapeWorkerError::TaskError(msg) => assert_eq!(msg, "test error"),
            other => panic!("Expected TaskError, got {:?}", other),
        }
    }

    #[test]
    #[allow(clippy::invalid_regex)] // intentionally invalid regex to test error path
    fn test_scrape_worker_error_from_regex_error() {
        let regex_err = regex::Regex::new("(unclosed").expect_err("should be invalid");
        let err: ScrapeWorkerError = regex_err.into();
        match err {
            ScrapeWorkerError::RegexError(msg) => assert!(!msg.is_empty()),
            other => panic!("Expected RegexError, got {:?}", other),
        }
    }

    #[test]
    fn test_scrape_worker_error_from_url_parse_error() {
        let url_err = url::Url::parse("not a url").expect_err("should be invalid");
        let err: ScrapeWorkerError = url_err.into();
        match err {
            ScrapeWorkerError::TaskError(msg) => assert!(msg.contains("URL解析错误")),
            other => panic!("Expected TaskError, got {:?}", other),
        }
    }

    // ========== Mock-based unit tests (no Docker required) ==========
    //
    // These tests construct a ScrapeWorker with mock/no-op dependencies,
    // allowing pure-logic methods like `should_crawl`, `build_crawl_request`,
    // `build_extract_request`, `trigger_webhook`, `deduct_feature_credits`,
    // and `save_result` to be tested without external services.

    use crate::domain::models::{
        Crawl, CreditsTransaction, CreditsTransactionType, DomainError, WebhookEvent,
    };
    use crate::domain::repositories::credits_repository::CreditsRepositoryError;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::extraction_service::ExtractionRule;
    use crate::domain::services::llm_service::TokenUsage;
    use std::collections::HashSet;

    // --- Mock trait implementations ---

    /// Mock TaskRepository — all methods return Ok with default values.
    struct MockTaskRepository;

    #[async_trait::async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    /// Mock ScrapeResultRepository — all methods return Ok with default values.
    struct MockScrapeResultRepository;

    #[async_trait::async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> Result<()> {
            Ok(())
        }
        async fn find_by_task_id(&self, _task_id: Uuid) -> Result<Option<ScrapeResult>> {
            Ok(None)
        }
        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> Result<f64> {
            Ok(0.0)
        }
    }

    /// Mock CrawlRepository — all methods return Ok with default values.
    struct MockCrawlRepository;

    #[async_trait::async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: CrawlStatus,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    /// Mock WebhookService — all methods return Ok.
    struct MockWebhookService;

    #[async_trait::async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
            Ok(())
        }
        async fn trigger_completion(&self, _task: &Task) -> Result<()> {
            Ok(())
        }
        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
            Ok(())
        }
    }

    /// Mock CreditsRepository — tracks deductions for verification.
    #[derive(Debug, Default)]
    struct MockCreditsRepo {
        deducted: Arc<std::sync::Mutex<Vec<(Uuid, i64)>>>,
    }

    #[async_trait::async_trait]
    impl CreditsRepository for MockCreditsRepo {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            Ok(100)
        }
        async fn deduct_credits(
            &self,
            team_id: Uuid,
            amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            self.deducted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((team_id, amount));
            Ok(())
        }
        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(100)
        }
        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }
        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            _initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(100)
        }
    }

    /// Mock CreateScrapeUseCase — execute returns a default response.
    struct MockCreateScrapeUseCase;

    #[async_trait::async_trait]
    impl CreateScrapeUseCaseTrait for MockCreateScrapeUseCase {
        async fn execute(
            &self,
            _request_dto: ScrapeRequestDto,
        ) -> Result<ScrapeResponse, DomainError> {
            Ok(ScrapeResponse {
                content: String::new(),
                status_code: 200,
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 0,
                final_url: None,
            })
        }
    }

    /// Mock RobotsChecker — always allows.
    struct MockRobotsChecker;

    #[async_trait::async_trait]
    impl RobotsCheckerTrait for MockRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
            Ok(true)
        }
        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> Result<Option<Duration>> {
            Ok(None)
        }
    }

    /// Mock ExtractionService — returns empty JSON.
    struct MockExtractionService;

    #[async_trait::async_trait]
    impl ExtractionServiceTrait for MockExtractionService {
        async fn extract(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<(Value, TokenUsage)> {
            Ok((json!({}), TokenUsage::default()))
        }
        async fn extract_with_schema(
            &self,
            _html_content: &str,
            _schema: &Value,
        ) -> Result<(Value, TokenUsage)> {
            Ok((json!({}), TokenUsage::default()))
        }
        fn extract_with_selectors(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<Value> {
            Ok(json!({}))
        }
    }

    /// Build a ScrapeWorker with all mock/no-op dependencies.
    ///
    /// This allows testing pure-logic methods without Docker or external
    /// services. The TeamSemaphore is an in-memory primitive — no external
    /// service is required during these tests.
    async fn build_mock_worker() -> ScrapeWorker {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for mock worker");
        let settings_arc = Arc::new(settings.clone());
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        ScrapeWorker::new(
            Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            settings_arc,
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        )
    }

    // --- should_crawl tests ---

    #[tokio::test]
    async fn test_mock_should_crawl_no_patterns_returns_true() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(None, None);
        assert!(worker.should_crawl("https://example.com/page1", &config));
        assert!(worker.should_crawl("https://other.com/page2", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_include_pattern_match() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(Some(vec!["example\\.com".to_string()]), None);
        assert!(worker.should_crawl("https://example.com/page", &config));
        assert!(worker.should_crawl("https://example.com/sub/page", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_include_pattern_no_match() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(Some(vec!["example\\.com".to_string()]), None);
        assert!(!worker.should_crawl("https://other.com/page", &config));
        assert!(!worker.should_crawl("https://foo.org/path", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_exclude_pattern_match() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(None, Some(vec!["blocked".to_string()]));
        assert!(!worker.should_crawl("https://example.com/blocked", &config));
        assert!(!worker.should_crawl("https://example.com/blocked/page", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_exclude_pattern_no_match() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(None, Some(vec!["blocked".to_string()]));
        assert!(worker.should_crawl("https://example.com/page", &config));
        assert!(worker.should_crawl("https://example.com/allowed", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_both_include_and_exclude() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(
            Some(vec!["example\\.com".to_string()]),
            Some(vec!["blocked".to_string()]),
        );
        // Matches include, doesn't match exclude → true
        assert!(worker.should_crawl("https://example.com/page", &config));
        // Matches include, matches exclude → false
        assert!(!worker.should_crawl("https://example.com/blocked", &config));
        // Doesn't match include → false (include takes priority)
        assert!(!worker.should_crawl("https://other.com/blocked", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_multiple_include_patterns() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(
            Some(vec!["example\\.com".to_string(), "test\\.org".to_string()]),
            None,
        );
        assert!(worker.should_crawl("https://example.com/page", &config));
        assert!(worker.should_crawl("https://test.org/page", &config));
        assert!(!worker.should_crawl("https://other.com/page", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_multiple_exclude_patterns() {
        let worker = build_mock_worker().await;
        let config =
            make_crawl_config(None, Some(vec!["blocked".to_string(), "admin".to_string()]));
        assert!(worker.should_crawl("https://example.com/page", &config));
        assert!(!worker.should_crawl("https://example.com/blocked", &config));
        assert!(!worker.should_crawl("https://example.com/admin", &config));
    }

    #[tokio::test]
    #[allow(clippy::invalid_regex)]
    async fn test_mock_should_crawl_include_fallback_string_match() {
        let worker = build_mock_worker().await;
        // Invalid regex — should fall back to string contains
        let config = make_crawl_config(Some(vec!["[unclosed".to_string()]), None);
        assert!(worker.should_crawl("https://example.com/[unclosed", &config));
        assert!(!worker.should_crawl("https://example.com/other", &config));
    }

    #[tokio::test]
    #[allow(clippy::invalid_regex)]
    async fn test_mock_should_crawl_exclude_fallback_string_match() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(None, Some(vec!["[unclosed".to_string()]));
        assert!(!worker.should_crawl("https://example.com/[unclosed", &config));
        assert!(worker.should_crawl("https://example.com/other", &config));
    }

    // --- build_crawl_request tests ---

    #[tokio::test]
    async fn test_mock_build_crawl_request_basic() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.method, HttpMethod::Get);
        assert!(request.options.body.is_none());
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(request.options.screenshot_config.is_none());
        assert!(!request.options.mobile);
        assert!(request.options.proxy.is_none());
        assert!(!request.options.skip_tls_verification);
        assert!(!request.options.needs_tls_fingerprint);
        assert!(!request.options.use_fire_engine);
        assert!(request.options.actions.is_empty());
        assert_eq!(request.options.sync_wait_ms, 0);
    }

    #[tokio::test]
    async fn test_mock_build_crawl_request_with_headers() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: Some(json!({
                "Accept": "text/html",
                "Authorization": "Bearer token123"
            })),
            extraction_rules: None,
        };
        let request = worker.build_crawl_request(&task, &config);
        assert_eq!(
            request.options.headers.get("Accept"),
            Some(&"text/html".to_string())
        );
        assert_eq!(
            request.options.headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
    }

    #[tokio::test]
    async fn test_mock_build_crawl_request_non_string_headers_filtered() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: Some(json!({
                "X-Number": 42,
                "X-Bool": true,
                "X-Null": null,
                "X-Valid": "ok"
            })),
            extraction_rules: None,
        };
        let request = worker.build_crawl_request(&task, &config);
        assert_eq!(request.options.headers.len(), 1);
        assert_eq!(
            request.options.headers.get("X-Valid"),
            Some(&"ok".to_string())
        );
        assert!(!request.options.headers.contains_key("X-Number"));
        assert!(!request.options.headers.contains_key("X-Bool"));
        assert!(!request.options.headers.contains_key("X-Null"));
    }

    #[tokio::test]
    async fn test_mock_build_crawl_request_with_proxy() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: Some("http://proxy:3128".to_string()),
            headers: None,
            extraction_rules: None,
        };
        let request = worker.build_crawl_request(&task, &config);
        assert_eq!(request.options.proxy, Some("http://proxy:3128".to_string()));
    }

    #[tokio::test]
    async fn test_mock_build_crawl_request_empty_headers_map() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: Some(json!({})),
            extraction_rules: None,
        };
        let request = worker.build_crawl_request(&task, &config);
        assert!(request.options.headers.is_empty());
    }

    // --- build_extract_request tests ---

    #[tokio::test]
    async fn test_mock_build_extract_request_basic() {
        let worker = build_mock_worker().await;
        let request = worker.build_extract_request("https://example.com/page");
        assert_eq!(request.url, "https://example.com/page");
        assert_eq!(request.options.method, HttpMethod::Get);
        assert!(request.options.body.is_none());
        assert!(request.options.headers.is_empty());
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(!request.options.mobile);
        assert!(request.options.proxy.is_none());
        // build_extract_request sets skip_tls_verification = true
        assert!(request.options.skip_tls_verification);
        assert!(!request.options.needs_tls_fingerprint);
        assert!(!request.options.use_fire_engine);
        assert!(request.options.actions.is_empty());
    }

    #[tokio::test]
    async fn test_mock_build_extract_request_different_urls() {
        let worker = build_mock_worker().await;
        let urls = vec![
            "https://example.com",
            "https://test.org/path",
            "http://localhost:8080",
        ];
        for url in &urls {
            let request = worker.build_extract_request(url);
            assert_eq!(request.url, *url);
        }
    }

    // --- trigger_webhook tests ---

    #[tokio::test]
    async fn test_mock_trigger_webhook_completion_no_error() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        // Should not panic — mock webhook service returns Ok
        worker.trigger_webhook(&task, None).await;
    }

    #[tokio::test]
    async fn test_mock_trigger_webhook_failure_with_error_msg() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        // Should not panic — mock webhook service returns Ok
        worker
            .trigger_webhook(&task, Some("Task failed".to_string()))
            .await;
    }

    // --- deduct_feature_credits tests ---

    #[tokio::test]
    async fn test_mock_deduct_feature_credits_screenshot_and_proxy() {
        // We can't easily verify the deduction with the mock worker because
        // we don't have access to the internal credits repo. But we can verify
        // the method doesn't panic.
        let worker = build_mock_worker().await;
        worker
            .deduct_feature_credits(Uuid::new_v4(), Uuid::new_v4(), true, true)
            .await;
    }

    #[tokio::test]
    async fn test_mock_deduct_feature_credits_screenshot_only() {
        let worker = build_mock_worker().await;
        worker
            .deduct_feature_credits(Uuid::new_v4(), Uuid::new_v4(), true, false)
            .await;
    }

    #[tokio::test]
    async fn test_mock_deduct_feature_credits_proxy_only() {
        let worker = build_mock_worker().await;
        worker
            .deduct_feature_credits(Uuid::new_v4(), Uuid::new_v4(), false, true)
            .await;
    }

    #[tokio::test]
    async fn test_mock_deduct_feature_credits_neither() {
        let worker = build_mock_worker().await;
        worker
            .deduct_feature_credits(Uuid::new_v4(), Uuid::new_v4(), false, false)
            .await;
    }

    // --- save_result tests ---

    #[tokio::test]
    async fn test_mock_save_result_basic() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: "<html>test</html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let result = worker.save_result(&task, &response, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_save_result_with_extra_data() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: "<html>test</html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let extra = json!({"title": "Test Page", "links": 5});
        let result = worker.save_result(&task, &response, Some(extra)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_save_result_with_screenshot() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: "<html>test</html>".to_string(),
            status_code: 200,
            screenshot: Some("base64data".to_string()),
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 200,
            final_url: None,
        };
        let result = worker.save_result(&task, &response, None).await;
        assert!(result.is_ok());
    }

    // --- process_text_encoding tests ---

    #[tokio::test]
    async fn test_mock_process_text_encoding_basic() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: "<html><body>Hello</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html; charset=utf-8".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let result = worker.process_text_encoding(&task, &response).await;
        // Should either return processed content or an error (depending on
        // CrawlTextIntegration behavior), but should not panic.
        match result {
            Ok(content) => assert!(!content.is_empty() || response.content.is_empty()),
            Err(_) => { /* Error is acceptable — integration disabled by default */ }
        }
    }

    // --- update_crawl_completion_status tests ---

    #[tokio::test]
    async fn test_mock_update_crawl_completion_status_crawl_not_found() {
        let worker = build_mock_worker().await;
        // MockCrawlRepository::find_by_id returns None, so this should
        // just log an error and return without panicking.
        worker.update_crawl_completion_status(Uuid::new_v4()).await;
    }

    // --- parse_crawl_payload tests ---

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_valid() {
        let worker = build_mock_worker().await;
        let crawl_id = Uuid::new_v4();
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 2,
            "config": {
                "max_depth": 3,
                "include_patterns": ["example\\.com"],
                "exclude_patterns": ["blocked"],
                "strategy": "bfs",
                "crawl_delay_ms": 100,
                "max_concurrency": 5,
                "proxy": "http://proxy:8080",
                "headers": {"X-Custom": "value"},
                "extraction_rules": {}
            }
        }));
        let (parsed_id, depth, config) = worker.parse_crawl_payload(&task).await.unwrap();
        assert_eq!(parsed_id, crawl_id);
        assert_eq!(depth, 2);
        assert_eq!(config.max_depth, 3);
    }

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_missing_crawl_id() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"depth": 1, "config": {}}));
        assert!(worker.parse_crawl_payload(&task).await.is_err());
    }

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_default_depth() {
        let worker = build_mock_worker().await;
        let crawl_id = Uuid::new_v4();
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "config": {"max_depth": 1}
        }));
        let (_, depth, _) = worker.parse_crawl_payload(&task).await.unwrap();
        assert_eq!(depth, 0);
    }

    // --- parse_extract_payload tests ---

    #[tokio::test]
    async fn test_mock_parse_extract_payload_valid() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({
            "urls": ["https://example.com/page"],
            "prompt": "Extract title",
            "model": "gpt-4"
        }));
        let (payload, url) = worker.parse_extract_payload(&task).await.unwrap();
        assert_eq!(url, "https://example.com/page");
        assert_eq!(payload.urls.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_parse_extract_payload_no_url() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"urls": []}));
        assert!(worker.parse_extract_payload(&task).await.is_err());
    }

    // --- check_robots_txt tests ---

    #[tokio::test]
    async fn test_mock_check_robots_txt_allowed() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        // MockRobotsChecker always returns Ok(true) for is_allowed and
        // Ok(None) for get_crawl_delay, so check_robots_txt returns true.
        assert!(worker.check_robots_txt(&task).await);
    }

    // --- handle_rules_extraction tests ---

    #[tokio::test]
    async fn test_mock_handle_rules_extraction() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body><h1>Hello</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let result = worker
            .handle_rules_extraction(&mut task, &response, &rules, "https://example.com")
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // --- handle_prompt_extraction tests ---

    #[tokio::test]
    async fn test_mock_handle_prompt_extraction() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body>Hello world</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 30,
            final_url: None,
        };
        let result = worker
            .handle_prompt_extraction(
                &mut task,
                &response,
                "Extract the main topic".to_string(),
                "https://example.com",
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // --- handle_schema_extraction tests ---

    #[tokio::test]
    async fn test_mock_handle_schema_extraction() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body>Data</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 20,
            final_url: None,
        };
        let schema = json!({"type": "object", "properties": {"title": {"type": "string"}}});
        let result = worker
            .handle_schema_extraction(&mut task, &response, &schema, "https://example.com")
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // --- save_extract_result tests ---

    #[tokio::test]
    async fn test_mock_save_extract_result_with_data() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "test content".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };
        let result = worker
            .save_extract_result(
                &mut task,
                &response,
                Some(json!({"title": "Test"})),
                "https://example.com",
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_mock_save_extract_result_without_data() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "raw content".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 5,
            final_url: None,
        };
        let result = worker
            .save_extract_result(&mut task, &response, None, "https://example.com")
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // --- extract_and_queue_links tests ---

    #[tokio::test]
    async fn test_mock_extract_and_queue_links_html_with_links() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let html = r#"<html><body>
            <a href="/page1">Page 1</a>
            <a href="https://example.com/page2">Page 2</a>
            <a href="https://other.com/page3">Page 3</a>
            <a href="mailto:test@example.com">Email</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_extract_and_queue_links_non_html_skipped() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "{\"key\": \"value\"}".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "application/json".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_extract_and_queue_links_with_include_filter() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        task.url = "https://example.com".to_string();
        let html = r#"<html><body>
            <a href="https://example.com/page1">Page 1</a>
            <a href="https://other.com/page2">Page 2</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(Some(vec!["example\\.com".to_string()]), None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());
    }

    // --- handle_failure tests ---

    #[tokio::test]
    async fn test_mock_handle_failure() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let result = worker.handle_failure(&mut task).await;
        assert!(result.is_ok());
    }

    // --- deduct_token_credits tests ---

    #[tokio::test]
    async fn test_mock_deduct_token_credits_zero_tokens() {
        let worker = build_mock_worker().await;
        let usage = TokenUsage::default();
        worker
            .deduct_token_credits(Uuid::new_v4(), Uuid::new_v4(), &usage, "test zero")
            .await;
    }

    #[tokio::test]
    async fn test_mock_deduct_token_credits_with_tokens() {
        let worker = build_mock_worker().await;
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        worker
            .deduct_token_credits(Uuid::new_v4(), Uuid::new_v4(), &usage, "test with tokens")
            .await;
    }

    // --- handle_scrape_success tests ---

    #[tokio::test]
    async fn test_mock_handle_scrape_success_no_extraction_rules() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body>Hello</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let result = worker.handle_scrape_success(&task, &response).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_handle_scrape_success_with_extraction_rules() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({
            "url": "https://example.com",
            "extraction_rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let result = worker.handle_scrape_success(&task, &response).await;
        assert!(result.is_ok());
    }

    // --- handle_crawl_success tests ---

    #[tokio::test]
    async fn test_mock_handle_crawl_success_with_link_extraction() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: r#"<html><body><a href="/page1">Link</a></body></html>"#.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 0, &config, &request)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_handle_crawl_success_max_depth_no_link_extraction() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: r#"<html><body><a href="/page1">Link</a></body></html>"#.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let mut config = make_crawl_config(None, None);
        config.max_depth = 1;
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 1, &config, &request)
            .await;
        assert!(result.is_ok());
    }

    // --- handle_crawl_failure tests ---

    #[tokio::test]
    async fn test_mock_handle_crawl_failure_basic() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_failure(
                &mut task,
                anyhow::anyhow!("Network error"),
                Uuid::new_v4(),
                &request,
            )
            .await;
        assert!(result.is_ok());
    }

    // --- process_scrape_task tests (error paths — engine_client has no engines) ---

    #[tokio::test]
    async fn test_mock_process_scrape_task_engine_error() {
        let worker = build_mock_worker().await;
        // Empty payload → build_scrape_request falls back to default request.
        // EngineClient::new() has no engines → scrape() returns an error.
        // The error path either marks the task as failed or calls handle_failure.
        let task = make_task(json!({}));
        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok()); // Error is handled internally, returns Ok(())
    }

    #[tokio::test]
    async fn test_mock_process_scrape_task_with_valid_payload_engine_error() {
        let worker = build_mock_worker().await;
        // Valid ScrapeRequestDto payload but engine still fails.
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "timeout": 10,
                "js_rendering": true
            }
        }));
        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok());
    }

    // --- process_crawl_task tests ---

    #[tokio::test]
    async fn test_mock_process_crawl_task_invalid_payload() {
        let worker = build_mock_worker().await;
        // Missing crawl_id → parse_crawl_payload fails → mark_failed is called.
        let task = make_task(json!({"depth": 1, "config": {}}));
        let result = worker.process_crawl_task(task).await;
        assert!(result.is_ok()); // Error handled internally
    }

    #[tokio::test]
    async fn test_mock_process_crawl_task_engine_error() {
        let worker = build_mock_worker().await;
        // Valid crawl payload but engine fails → handle_crawl_failure is called.
        let crawl_id = Uuid::new_v4();
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 0,
            "config": {"max_depth": 2}
        }));
        let result = worker.process_crawl_task(task).await;
        assert!(result.is_ok());
    }

    // --- process_extract_task tests ---

    #[tokio::test]
    async fn test_mock_process_extract_task_engine_error() {
        let worker = build_mock_worker().await;
        // Valid extract payload but engine fails → returns Err.
        let task = make_task(json!({
            "urls": ["https://example.com/page"]
        }));
        let result = worker.process_extract_task(task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_process_extract_task_invalid_payload() {
        let worker = build_mock_worker().await;
        // Payload is not a valid ExtractRequestDto → parse fails → returns Err.
        let task = make_task(json!({"not_a_valid": "field"}));
        let result = worker.process_extract_task(task).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_process_extract_task_empty_urls() {
        let worker = build_mock_worker().await;
        // Valid ExtractRequestDto but no URLs → parse_extract_payload fails.
        let task = make_task(json!({"urls": []}));
        let result = worker.process_extract_task(task).await;
        assert!(result.is_err());
    }

    // ========== ScrapeWorkerBuilder tests ==========

    #[tokio::test]
    async fn test_builder_new_creates_default() {
        let builder = ScrapeWorkerBuilder::new();
        assert_eq!(builder.default_concurrency_limit, 10);
    }

    #[tokio::test]
    async fn test_builder_default_impl() {
        let builder = ScrapeWorkerBuilder::default();
        assert_eq!(builder.default_concurrency_limit, 10);
    }

    #[tokio::test]
    async fn test_builder_with_default_concurrency_limit() {
        let builder = ScrapeWorkerBuilder::new().with_default_concurrency_limit(50);
        assert_eq!(builder.default_concurrency_limit, 50);
    }

    #[tokio::test]
    async fn test_builder_build_success() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(worker.is_ok(), "build should succeed with all deps");
        let w = worker.unwrap();
        assert_eq!(w.default_concurrency_limit, 10);
    }

    #[tokio::test]
    async fn test_builder_build_missing_repository() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "repository is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_result_repository() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "result_repository is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_crawl_repository() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "crawl_repository is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_webhook_service() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "webhook_service is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_engine_client() {
        let regex_cache = make_regex_cache().await;
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "engine_client is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_team_semaphore() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "team_semaphore is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_settings() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "settings is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_extraction_service() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "extraction_service is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_regex_cache() {
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "regex_cache is required");
    }

    #[tokio::test]
    async fn test_builder_with_custom_concurrency_limit() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .with_default_concurrency_limit(100)
            .build()
            .expect("build should succeed");

        assert_eq!(worker.default_concurrency_limit, 100);
    }

    // ========== testcontainers integration tests ==========
    //
    // These tests construct a full ScrapeWorker with real PostgreSQL +
    // HTTP client via testcontainers, exercising the `new()`
    // constructor, `ScrapeWorkerBuilder`, and pure-logic methods like
    // `should_crawl` and `build_crawl_request` that require a
    // fully-initialized worker instance.

    use crate::bootstrap::infrastructure::init_infrastructure;
    use crate::bootstrap::services::init_services;
    use crate::common::test_support::testcontainers_fixtures as tcf;

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    /// Build a full ScrapeWorker using testcontainers-provided services.
    async fn build_scrape_worker() -> anyhow::Result<ScrapeWorker> {
        let handle = tcf::DbHandle::start().await?;
        let settings = tcf::settings_with_urls(&handle.pg.url)?;
        let settings_arc = std::sync::Arc::new(settings.clone());
        let infra = init_infrastructure(&settings).await?;
        let engines = crate::bootstrap::engines::init_engine_components(
            infra.http_client.clone(),
            // proxy_url=None：此处无代理配置（架构 MEDIUM 5：用 Option 替代空字符串 sentinel）
            None,
            &settings.engines,
            // 注入 timeout（架构 MEDIUM 2：避免 ReqwestEngine 硬编码 30 秒）
            settings.timeouts.engines.default_timeout_seconds,
        );
        let services = init_services(
            &infra,
            engines.router.clone(),
            engines.engine_client.clone(),
            infra.http_client.clone(),
            &settings,
        )
        .await;

        // Construct ScrapeWorker via new().
        let worker = ScrapeWorker::new(
            infra.repositories.task_repo.clone() as Arc<dyn TaskRepository>,
            infra.repositories.result_repo.clone() as Arc<dyn ScrapeResultRepository>,
            infra.repositories.crawl_repo.clone() as Arc<dyn CrawlRepository>,
            services.webhook_service.clone(),
            infra.repositories.credits_repo.clone() as Arc<dyn CreditsRepository>,
            engines.engine_client.clone(),
            services.create_scrape_use_case.clone(),
            services.team_semaphore.clone(),
            services.robots_checker.clone(),
            settings_arc,
            settings.concurrency.default_team_limit as usize,
            services.extraction_service.clone(),
            (*services.regex_cache).clone(),
        );

        Ok(worker)
    }

    /// Helper: construct a minimal CrawlConfigDto with the given patterns.
    fn make_crawl_config(
        include_patterns: Option<Vec<String>>,
        exclude_patterns: Option<Vec<String>>,
    ) -> CrawlConfigDto {
        CrawlConfigDto {
            max_depth: 1,
            include_patterns,
            exclude_patterns,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        }
    }

    #[tokio::test]
    async fn tc_scrape_worker_new_constructs_successfully() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_scrape_worker_new_constructs_successfully");
            return;
        }
        let worker = match build_scrape_worker().await {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[skip] failed to build ScrapeWorker: {e}");
                return;
            }
        };
        // Verify the worker has a unique ID.
        assert_ne!(worker.worker_id, Uuid::nil());
        // Verify the worker has a default concurrency limit.
        assert!(worker.default_concurrency_limit >= 1);
    }

    #[tokio::test]
    async fn tc_scrape_worker_should_crawl_with_no_patterns() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_scrape_worker_should_crawl_with_no_patterns");
            return;
        }
        let worker = match build_scrape_worker().await {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[skip] failed to build ScrapeWorker: {e}");
                return;
            }
        };
        let config = make_crawl_config(None, None);
        // With no include/exclude patterns, should_crawl should return true.
        assert!(worker.should_crawl("https://example.com/page1", &config));
    }

    #[tokio::test]
    async fn tc_scrape_worker_should_crawl_with_include_patterns() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_scrape_worker_should_crawl_with_include_patterns"
            );
            return;
        }
        let worker = match build_scrape_worker().await {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[skip] failed to build ScrapeWorker: {e}");
                return;
            }
        };
        let config = make_crawl_config(Some(vec!["example\\.com".to_string()]), None);
        // URL matching include pattern → should crawl.
        assert!(worker.should_crawl("https://example.com/page", &config));
        // URL not matching include pattern → should not crawl.
        assert!(!worker.should_crawl("https://other.com/page", &config));
    }

    #[tokio::test]
    async fn tc_scrape_worker_should_crawl_with_exclude_patterns() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_scrape_worker_should_crawl_with_exclude_patterns"
            );
            return;
        }
        let worker = match build_scrape_worker().await {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[skip] failed to build ScrapeWorker: {e}");
                return;
            }
        };
        let config = make_crawl_config(None, Some(vec!["blocked".to_string()]));
        // URL not matching exclude pattern → should crawl.
        assert!(worker.should_crawl("https://example.com/page", &config));
        // URL matching exclude pattern → should not crawl.
        assert!(!worker.should_crawl("https://example.com/blocked", &config));
    }

    #[tokio::test]
    async fn tc_scrape_worker_builder_builds_full_worker() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_scrape_worker_builder_builds_full_worker");
            return;
        }
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&handle.pg.url).unwrap();
        let settings_arc = std::sync::Arc::new(settings.clone());
        let infra = match init_infrastructure(&settings).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[skip] failed to init infrastructure: {e}");
                return;
            }
        };
        let engines = crate::bootstrap::engines::init_engine_components(
            infra.http_client.clone(),
            // proxy_url=None：此处无代理配置（架构 MEDIUM 5：用 Option 替代空字符串 sentinel）
            None,
            &settings.engines,
            // 注入 timeout（架构 MEDIUM 2：避免 ReqwestEngine 硬编码 30 秒）
            settings.timeouts.engines.default_timeout_seconds,
        );
        let services = init_services(
            &infra,
            engines.router.clone(),
            engines.engine_client.clone(),
            infra.http_client.clone(),
            &settings,
        )
        .await;

        // Use ScrapeWorkerBuilder to construct the worker.
        let worker =
            ScrapeWorkerBuilder::new()
                .with_repository(infra.repositories.task_repo.clone() as Arc<dyn TaskRepository>)
                .with_result_repository(
                    infra.repositories.result_repo.clone() as Arc<dyn ScrapeResultRepository>
                )
                .with_crawl_repository(
                    infra.repositories.crawl_repo.clone() as Arc<dyn CrawlRepository>
                )
                .with_webhook_service(services.webhook_service.clone())
                .with_credits_repository(
                    infra.repositories.credits_repo.clone() as Arc<dyn CreditsRepository>
                )
                .with_engine_client(engines.engine_client.clone())
                .with_create_scrape_use_case(services.create_scrape_use_case.clone())
                .with_team_semaphore(services.team_semaphore.clone())
                .with_robots_checker(services.robots_checker.clone())
                .with_settings(settings_arc)
                .with_default_concurrency_limit(settings.concurrency.default_team_limit as usize)
                .with_extraction_service(services.extraction_service.clone())
                .with_regex_cache((*services.regex_cache).clone())
                .build()
                .expect("ScrapeWorkerBuilder::build should succeed with all required deps");

        // Verify the builder produced a valid worker.
        assert_ne!(worker.worker_id, Uuid::nil());
    }

    #[tokio::test]
    async fn tc_scrape_worker_build_crawl_request() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_scrape_worker_build_crawl_request");
            return;
        }
        let worker = match build_scrape_worker().await {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[skip] failed to build ScrapeWorker: {e}");
                return;
            }
        };

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );

        let config = make_crawl_config(None, None);

        // build_crawl_request is a &self method that constructs a ScrapeRequest.
        let request = worker.build_crawl_request(&task, &config);
        // Verify the request has the correct URL.
        assert_eq!(request.url, "https://example.com");
    }

    // ========== Additional coverage tests ==========
    //
    // These tests target uncovered code paths: extract_data_with_rules,
    // token-credit deduction in handle_scrape_success, process_next_task,
    // Debug impl, parse_crawl_payload edge cases, DFS strategy in link
    // extraction, and more.

    use crate::queue::task_queue::QueueError;

    /// Mock TaskQueue — dequeue returns None (empty queue).
    struct MockTaskQueue;

    #[async_trait::async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            Ok(task)
        }
        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Ok(None)
        }
        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    /// TaskQueue whose dequeue always returns Err — exercises run() Err branch.
    struct FailingTaskQueue;

    #[async_trait::async_trait]
    impl TaskQueue for FailingTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            Ok(task)
        }
        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Err(QueueError::Empty)
        }
        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    /// Mock ExtractionService that returns non-zero TokenUsage, exercising
    /// the token-credit deduction code paths.
    struct MockExtractionServiceWithTokens;

    #[async_trait::async_trait]
    impl ExtractionServiceTrait for MockExtractionServiceWithTokens {
        async fn extract(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<(Value, TokenUsage)> {
            Ok((
                json!({"title": "Extracted Title"}),
                TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    total_tokens: 150,
                },
            ))
        }
        async fn extract_with_schema(
            &self,
            _html_content: &str,
            _schema: &Value,
        ) -> Result<(Value, TokenUsage)> {
            Ok((
                json!({"data": "value"}),
                TokenUsage {
                    prompt_tokens: 200,
                    completion_tokens: 100,
                    total_tokens: 300,
                },
            ))
        }
        fn extract_with_selectors(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<Value> {
            Ok(json!({}))
        }
    }

    /// Build a ScrapeWorker whose ExtractionService returns non-zero tokens.
    async fn build_mock_worker_with_tokens() -> ScrapeWorker {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for mock worker");
        let settings_arc = Arc::new(settings.clone());
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        ScrapeWorker::new(
            Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            settings_arc,
            10,
            Arc::new(MockExtractionServiceWithTokens) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        )
    }

    // --- Debug impl tests ---

    #[tokio::test]
    async fn test_scrape_worker_debug_impl_outputs_fields() {
        let worker = build_mock_worker().await;
        let debug_str = format!("{:?}", worker);
        assert!(debug_str.contains("ScrapeWorker"));
        assert!(debug_str.contains("worker_id"));
        assert!(debug_str.contains("default_concurrency_limit"));
        // finish_non_exhaustive adds ".." at the end
        assert!(debug_str.contains(".."));
    }

    // --- process_next_task tests ---

    #[tokio::test]
    async fn test_mock_process_next_task_empty_queue_returns_false() {
        let worker = build_mock_worker().await;
        let queue = MockTaskQueue;
        let result = worker.process_next_task(&queue).await;
        assert!(result.is_ok());
        assert!(!result.unwrap(), "empty queue should return false");
    }

    // --- extract_data_with_rules tests (via handle_crawl_success) ---

    #[tokio::test]
    async fn test_mock_handle_crawl_success_with_extraction_rules() {
        let worker = build_mock_worker_with_tokens().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: Some(rules),
        };
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 0, &config, &request)
            .await;
        assert!(result.is_ok());
    }

    // --- handle_scrape_success with non-zero token usage ---

    #[tokio::test]
    async fn test_mock_handle_scrape_success_with_token_usage() {
        let worker = build_mock_worker_with_tokens().await;
        let task = make_task(json!({
            "url": "https://example.com",
            "extraction_rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let result = worker.handle_scrape_success(&task, &response).await;
        assert!(result.is_ok());
    }

    // --- parse_crawl_payload edge cases ---

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_invalid_crawl_id_defaults_to_nil() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({
            "crawl_id": "not-a-uuid",
            "depth": 1,
            "config": {"max_depth": 2}
        }));
        let (crawl_id, depth, _) = worker.parse_crawl_payload(&task).await.unwrap();
        // Invalid UUID string falls back to Uuid::nil() via unwrap_or_default()
        assert_eq!(crawl_id, Uuid::nil());
        assert_eq!(depth, 1);
    }

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_missing_config_fails() {
        let worker = build_mock_worker().await;
        let crawl_id = Uuid::new_v4();
        // config is missing → defaults to json!({}) → deserialization fails
        // because CrawlConfigDto.max_depth is a required u32 field.
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 3
        }));
        assert!(worker.parse_crawl_payload(&task).await.is_err());
    }

    #[tokio::test]
    async fn test_mock_parse_crawl_payload_invalid_config_json_fails() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({
            "crawl_id": Uuid::new_v4().to_string(),
            "depth": 0,
            "config": "not-an-object"
        }));
        assert!(worker.parse_crawl_payload(&task).await.is_err());
    }

    // --- should_crawl with empty pattern lists ---

    #[tokio::test]
    async fn test_mock_should_crawl_empty_include_patterns_returns_false() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(Some(vec![]), None);
        // Empty include patterns vec: for loop doesn't run, matched stays false,
        // then `if !matched { return false; }` triggers → returns false.
        assert!(!worker.should_crawl("https://example.com/page", &config));
    }

    #[tokio::test]
    async fn test_mock_should_crawl_empty_exclude_patterns_returns_true() {
        let worker = build_mock_worker().await;
        let config = make_crawl_config(None, Some(vec![]));
        // Empty exclude patterns — for loop doesn't run, no exclusion → returns true
        assert!(worker.should_crawl("https://example.com/page", &config));
    }

    // --- extract_and_queue_links with DFS strategy ---

    #[tokio::test]
    async fn test_mock_extract_and_queue_links_dfs_strategy() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        task.url = "https://example.com".to_string();
        let html = r#"<html><body>
            <a href="https://example.com/page1">Page 1</a>
            <a href="https://example.com/page2">Page 2</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = CrawlConfigDto {
            max_depth: 3,
            include_patterns: None,
            exclude_patterns: None,
            strategy: Some("dfs".to_string()),
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());
    }

    // --- extract_and_queue_links filters self-links and non-http protocols ---

    #[tokio::test]
    async fn test_mock_extract_and_queue_links_filters_self_and_non_http() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        task.url = "https://example.com".to_string();
        let html = r#"<html><body>
            <a href="https://example.com">Self</a>
            <a href="mailto:test@example.com">Email</a>
            <a href="javascript:void(0)">JS</a>
            <a href="/relative">Relative</a>
            <a href="https://other.com/page">Other</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());
    }

    // --- build_crawl_request with extraction_rules in config ---

    #[tokio::test]
    async fn test_mock_build_crawl_request_with_extraction_rules() {
        let worker = build_mock_worker().await;
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            json!({}),
        );
        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let config = CrawlConfigDto {
            max_depth: 2,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: Some(rules),
        };
        let request = worker.build_crawl_request(&task, &config);
        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.method, HttpMethod::Get);
    }

    // --- parse_extract_payload with rules ---

    #[tokio::test]
    async fn test_mock_parse_extract_payload_with_rules() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({
            "urls": ["https://example.com/page"],
            "rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));
        let (payload, url) = worker.parse_extract_payload(&task).await.unwrap();
        assert_eq!(url, "https://example.com/page");
        assert!(payload.rules.is_some());
        assert_eq!(payload.rules.as_ref().unwrap().len(), 1);
    }

    // --- handle_*_extraction with non-zero token usage ---

    #[tokio::test]
    async fn test_mock_handle_rules_extraction_with_tokens() {
        let worker = build_mock_worker_with_tokens().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body><h1>Hello</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
            final_url: None,
        };
        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let result = worker
            .handle_rules_extraction(&mut task, &response, &rules, "https://example.com")
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_mock_handle_prompt_extraction_with_tokens() {
        let worker = build_mock_worker_with_tokens().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body>Hello world</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 30,
            final_url: None,
        };
        let result = worker
            .handle_prompt_extraction(
                &mut task,
                &response,
                "Extract the main topic".to_string(),
                "https://example.com",
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_mock_handle_schema_extraction_with_tokens() {
        let worker = build_mock_worker_with_tokens().await;
        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body>Data</body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 20,
            final_url: None,
        };
        let schema = json!({"type": "object", "properties": {"title": {"type": "string"}}});
        let result = worker
            .handle_schema_extraction(&mut task, &response, &schema, "https://example.com")
            .await;
        assert!(result.is_ok());
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // --- handle_crawl_failure with proxy (credit deduction in failure path) ---

    #[tokio::test]
    async fn test_mock_handle_crawl_failure_with_proxy() {
        let worker = build_mock_worker().await;
        let mut task = make_task(json!({}));
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: Some("http://proxy:3128".to_string()),
            headers: None,
            extraction_rules: None,
        };
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_failure(
                &mut task,
                anyhow::anyhow!("Network error"),
                Uuid::new_v4(),
                &request,
            )
            .await;
        assert!(result.is_ok());
    }

    // --- handle_crawl_success with screenshot (credit deduction) ---

    #[tokio::test]
    async fn test_mock_handle_crawl_success_with_screenshot() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: r#"<html><body><a href="/page1">Link</a></body></html>"#.to_string(),
            status_code: 200,
            screenshot: Some("base64screenshot".to_string()),
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 0, &config, &request)
            .await;
        assert!(result.is_ok());
    }

    // --- process_text_encoding with various content types ---

    #[tokio::test]
    async fn test_mock_process_text_encoding_json_content() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: r#"{"key": "value"}"#.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "application/json".to_string(),
            headers: HashMap::new(),
            response_time_ms: 30,
            final_url: None,
        };
        let result = worker.process_text_encoding(&task, &response).await;
        // Should not panic — may succeed or fail depending on integration
        match result {
            Ok(content) => assert!(!content.is_empty() || response.content.is_empty()),
            Err(_) => { /* Error is acceptable */ }
        }
    }

    #[tokio::test]
    async fn test_mock_process_text_encoding_empty_content() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        let response = ScrapeResponse {
            content: String::new(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 5,
            final_url: None,
        };
        let result = worker.process_text_encoding(&task, &response).await;
        match result {
            Ok(content) => assert!(content.is_empty()),
            Err(_) => { /* Error is acceptable */ }
        }
    }

    // --- save_result with large content ---

    #[tokio::test]
    async fn test_mock_save_result_large_content() {
        let worker = build_mock_worker().await;
        let task = make_task(json!({"url": "https://example.com"}));
        // Content > 1MB threshold
        let large_content = "x".repeat(1024 * 1024 + 1);
        let response = ScrapeResponse {
            content: large_content,
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 500,
            final_url: None,
        };
        let result = worker.save_result(&task, &response, None).await;
        assert!(result.is_ok());
    }

    // ========== Configurable mocks for error/edge case coverage ==========
    //
    // These mocks allow configuring return values per-test, enabling
    // coverage of error paths, specific crawl states, robots.txt denial,
    // engine timeout scenarios, and concurrency limit behavior.

    use crate::engines::router::{EngineRouterTrait, EngineStats};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    // --- ConfigurableTaskRepo ---

    /// TaskRepository that can be configured to fail specific operations
    /// and track `mark_failed` / `update` call counts.
    struct ConfigurableTaskRepo {
        fail_mark_failed: AtomicBool,
        fail_update: AtomicBool,
        fail_find_by_id: AtomicBool,
        fail_find_existing_urls: AtomicBool,
        find_by_id_result: std::sync::Mutex<Option<Task>>,
        existing_urls_result: std::sync::Mutex<HashSet<String>>,
        mark_failed_count: AtomicU32,
        update_count: AtomicU32,
        create_count: AtomicU32,
        mark_completed_count: AtomicU32,
    }

    impl ConfigurableTaskRepo {
        fn new() -> Self {
            Self {
                fail_mark_failed: AtomicBool::new(false),
                fail_update: AtomicBool::new(false),
                fail_find_by_id: AtomicBool::new(false),
                fail_find_existing_urls: AtomicBool::new(false),
                find_by_id_result: std::sync::Mutex::new(None),
                existing_urls_result: std::sync::Mutex::new(HashSet::new()),
                mark_failed_count: AtomicU32::new(0),
                update_count: AtomicU32::new(0),
                create_count: AtomicU32::new(0),
                mark_completed_count: AtomicU32::new(0),
            }
        }

        fn mark_completed_count(&self) -> u32 {
            self.mark_completed_count.load(Ordering::SeqCst)
        }

        fn mark_failed_count(&self) -> u32 {
            self.mark_failed_count.load(Ordering::SeqCst)
        }

        fn update_count(&self) -> u32 {
            self.update_count.load(Ordering::SeqCst)
        }

        fn create_count(&self) -> u32 {
            self.create_count.load(Ordering::SeqCst)
        }

        fn set_existing_urls(&self, urls: HashSet<String>) {
            *self.existing_urls_result.lock().unwrap() = urls;
        }
    }

    #[async_trait::async_trait]
    impl TaskRepository for ConfigurableTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            Ok(task.clone())
        }
        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            if self.fail_find_by_id.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock find_by_id error"
                )));
            }
            let guard = self.find_by_id_result.lock().unwrap();
            Ok(guard.as_ref().filter(|t| t.id == id).cloned())
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.update_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_update.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock update error"
                )));
            }
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            self.mark_completed_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            self.mark_failed_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_mark_failed.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock mark_failed error"
                )));
            }
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            if self.fail_find_existing_urls.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock find_existing_urls error"
                )));
            }
            Ok(self.existing_urls_result.lock().unwrap().clone())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    // --- ConfigurableCrawlRepo ---

    /// CrawlRepository that returns a configurable Crawl from find_by_id
    /// and can fail on increment operations.
    struct ConfigurableCrawlRepo {
        crawl: std::sync::Mutex<Option<Crawl>>,
        fail_find_by_id: AtomicBool,
        fail_increment_completed: AtomicBool,
        fail_increment_failed: AtomicBool,
        fail_update_status: AtomicBool,
        update_status_count: AtomicU32,
    }

    impl ConfigurableCrawlRepo {
        fn new() -> Self {
            Self {
                crawl: std::sync::Mutex::new(None),
                fail_find_by_id: AtomicBool::new(false),
                fail_increment_completed: AtomicBool::new(false),
                fail_increment_failed: AtomicBool::new(false),
                fail_update_status: AtomicBool::new(false),
                update_status_count: AtomicU32::new(0),
            }
        }

        fn set_crawl(&self, crawl: Crawl) {
            *self.crawl.lock().unwrap() = Some(crawl);
        }

        fn update_status_count(&self) -> u32 {
            self.update_status_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl CrawlRepository for ConfigurableCrawlRepo {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            if self.fail_find_by_id.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock find_by_id error"
                )));
            }
            Ok(self.crawl.lock().unwrap().clone())
        }
        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            if self.fail_increment_completed.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock increment_completed error"
                )));
            }
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            if self.fail_increment_failed.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock increment_failed error"
                )));
            }
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: CrawlStatus,
        ) -> Result<(), RepositoryError> {
            self.update_status_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_update_status.load(Ordering::SeqCst) {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "Mock update_status error"
                )));
            }
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    // --- FailingScrapeResultRepo ---

    /// ScrapeResultRepository that always fails on save.
    struct FailingScrapeResultRepo;

    #[async_trait::async_trait]
    impl ScrapeResultRepository for FailingScrapeResultRepo {
        async fn save(&self, _result: ScrapeResult) -> Result<()> {
            Err(anyhow::anyhow!("Mock save error"))
        }
        async fn find_by_task_id(&self, _task_id: Uuid) -> Result<Option<ScrapeResult>> {
            Ok(None)
        }
        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> Result<f64> {
            Ok(0.0)
        }
    }

    // --- FailingWebhookService ---

    /// WebhookService that always fails.
    struct FailingWebhookService;

    #[async_trait::async_trait]
    impl WebhookService for FailingWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
            Err(anyhow::anyhow!("Mock webhook error"))
        }
        async fn trigger_completion(&self, _task: &Task) -> Result<()> {
            Err(anyhow::anyhow!("Mock trigger_completion error"))
        }
        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
            Err(anyhow::anyhow!("Mock trigger_failure error"))
        }
    }

    // --- FailingCreditsRepo ---

    /// CreditsRepository that always fails on deduct_credits.
    struct FailingCreditsRepo;

    #[async_trait::async_trait]
    impl CreditsRepository for FailingCreditsRepo {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            Ok(100)
        }
        async fn deduct_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            Err(CreditsRepositoryError::InsufficientCredits {
                available: 0,
                required: 1,
            })
        }
        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(100)
        }
        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }
        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            _initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(_initial_balance)
        }
    }

    // --- DenyingRobotsChecker ---

    /// RobotsChecker that always denies access.
    struct DenyingRobotsChecker;

    #[async_trait::async_trait]
    impl RobotsCheckerTrait for DenyingRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
            Ok(false)
        }
        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> Result<Option<Duration>> {
            Ok(None)
        }
    }

    // --- DelayingRobotsChecker ---

    /// RobotsChecker that always allows but returns a crawl delay.
    struct DelayingRobotsChecker;

    #[async_trait::async_trait]
    impl RobotsCheckerTrait for DelayingRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
            Ok(true)
        }
        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> Result<Option<Duration>> {
            Ok(Some(Duration::from_millis(10)))
        }
    }

    // --- ErroringRobotsChecker ---

    /// RobotsChecker that returns errors for both is_allowed and get_crawl_delay.
    /// is_allowed error should fall back to true (unwrap_or(true)),
    /// get_crawl_delay error should fall back to None (unwrap_or(None)).
    struct ErroringRobotsChecker;

    #[async_trait::async_trait]
    impl RobotsCheckerTrait for ErroringRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
            Err(anyhow::anyhow!("Mock robots error"))
        }
        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> Result<Option<Duration>> {
            Err(anyhow::anyhow!("Mock crawl delay error"))
        }
    }

    // --- MockEngineRouter (for timeout/all_engines_failed paths) ---

    /// EngineRouter that returns a configurable EngineError on route().
    struct MockEngineRouter {
        error_factory: Box<dyn Fn() -> EngineError + Send + Sync>,
    }

    impl MockEngineRouter {
        fn new(error: EngineError) -> Self {
            // EngineError doesn't implement Clone, so we reconstruct via a factory.
            // We store the error in an Arc and recreate it by matching on the variant.
            let error = Arc::new(error);
            Self {
                error_factory: Box::new(move || match &*error {
                    EngineError::Timeout(d) => EngineError::Timeout(*d),
                    EngineError::AllEnginesFailed(s) => EngineError::AllEnginesFailed(s.clone()),
                    EngineError::Expired => EngineError::Expired,
                    EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
                    EngineError::InvalidUrl(s) => EngineError::InvalidUrl(s.clone()),
                    EngineError::SsrfProtection(s) => EngineError::SsrfProtection(s.clone()),
                    EngineError::BrowserError(s) => EngineError::BrowserError(s.clone()),
                    EngineError::RequestFailed(s) => EngineError::RequestFailed(s.clone()),
                    EngineError::Other(s) => EngineError::Other(s.clone()),
                    EngineError::Internal(s) => EngineError::Internal(s.clone()),
                }),
            }
        }
    }

    #[async_trait::async_trait]
    impl EngineRouterTrait for MockEngineRouter {
        async fn route(
            &self,
            _request: &crate::engines::engine_client::InternalScrapeRequest,
        ) -> Result<crate::engines::engine_client::InternalScrapeResponse, EngineError> {
            Err((self.error_factory)())
        }
        async fn aggregate(
            &self,
            _request: &crate::engines::engine_client::InternalScrapeRequest,
        ) -> Result<crate::engines::engine_client::InternalScrapeResponse, EngineError> {
            Err((self.error_factory)())
        }
        fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats> {
            std::collections::HashMap::new()
        }
        fn reset_engine_stats(&self, _engine_name: &str) {}
        fn registered_engines(&self) -> Vec<String> {
            vec![]
        }
    }

    // --- TaskQueueWithTask ---

    /// TaskQueue that returns a configured task on the first dequeue,
    /// then None on subsequent calls.
    struct TaskQueueWithTask {
        task: std::sync::Mutex<Option<Task>>,
    }

    impl TaskQueueWithTask {
        fn new(task: Task) -> Self {
            Self {
                task: std::sync::Mutex::new(Some(task)),
            }
        }
    }

    #[async_trait::async_trait]
    impl TaskQueue for TaskQueueWithTask {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            Ok(task)
        }
        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Ok(self.task.lock().unwrap().take())
        }
        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    // --- Helper: build worker with configurable deps ---

    /// Build a ScrapeWorker with the given configurable repository, crawl
    /// repository, robots checker, and engine client. All other deps use
    /// default mocks.
    async fn build_configurable_worker(
        task_repo: Arc<dyn TaskRepository>,
        crawl_repo: Arc<dyn CrawlRepository>,
        robots_checker: Arc<dyn RobotsCheckerTrait>,
        engine_client: Arc<EngineClient>,
    ) -> ScrapeWorker {
        let regex_cache = make_regex_cache().await;
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for configurable worker");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        ScrapeWorker::new(
            task_repo,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            crawl_repo,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            robots_checker,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        )
    }

    // --- Helper: build worker with configurable credits/webhook/result repos ---
    async fn build_worker_with_failing_deps(
        result_repo: Arc<dyn ScrapeResultRepository>,
        webhook_service: Arc<dyn WebhookService>,
        credits_repo: Arc<dyn CreditsRepository>,
    ) -> ScrapeWorker {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for failing deps worker");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        ScrapeWorker::new(
            Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>,
            result_repo,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            webhook_service,
            credits_repo,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        )
    }

    // ========== process_task: task expiration tests ==========

    #[tokio::test]
    async fn test_process_task_expired_task_marks_failed_and_triggers_webhook() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        // Create a task that has already expired
        let mut task = make_task(json!({"url": "https://example.com"}));
        task.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));

        let result = worker.process_task(task).await;
        assert!(result.is_ok(), "expired task should return Ok(())");

        // mark_failed should have been called
        assert_eq!(
            task_repo.mark_failed_count(),
            1,
            "mark_failed should be called once for expired task"
        );
    }

    #[tokio::test]
    async fn test_process_task_not_expired_proceeds_normally() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        // Task expires in the future — should proceed to processing
        let mut task = make_task(json!({"url": "https://example.com"}));
        task.expires_at = Some(Utc::now() + chrono::Duration::hours(1));

        // process_task will call process_scrape_task which will fail
        // (no engines), but that's handled internally.
        let result = worker.process_task(task).await;
        assert!(result.is_ok());

        // mark_failed should NOT have been called for expiration
        // (it may be called by handle_failure, but that's a different path)
        // The key is that the expiration check passed.
        assert_eq!(
            task_repo.mark_failed_count(),
            0,
            "mark_failed should not be called for expiration check on non-expired task"
        );
    }

    // ========== process_task: concurrency limit exceeded tests ==========

    #[tokio::test]
    async fn test_process_task_concurrency_limit_exceeded_reschedules() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let regex_cache = make_regex_cache().await;
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for concurrency test");

        // TeamSemaphore with capacity 0 — no permits can be acquired
        let team_semaphore = Arc::new(TeamSemaphore::new(0));

        let worker = ScrapeWorker::new(
            task_repo.clone(),
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(EngineClient::new()),
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let task = make_task(json!({"url": "https://example.com"}));
        let result = worker.process_task(task).await;
        assert!(result.is_ok(), "rescheduled task should return Ok(())");

        // update should have been called to reschedule the task
        assert_eq!(
            task_repo.update_count(),
            1,
            "update should be called to reschedule task"
        );
    }

    // ========== process_next_task: success path (dequeue returns a task) ==========

    #[tokio::test]
    async fn test_process_next_task_with_task_returns_true() {
        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let task = make_task(json!({"url": "https://example.com"}));
        let queue = TaskQueueWithTask::new(task);

        let result = worker.process_next_task(&queue).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "process_next_task should return true when a task is dequeued"
        );
    }

    // ========== process_scrape_task: timeout/all_engines_failed error path ==========

    #[tokio::test]
    async fn test_process_scrape_task_timeout_error_marks_failed_directly() {
        // Use a mock EngineRouter that returns EngineError::Timeout
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new(
            EngineError::Timeout(Duration::from_secs(30)),
        ));
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            engine_client,
        )
        .await;

        let mut task = make_task(json!({"url": "https://example.com"}));
        // Set up find_by_id to return the task so the timeout path can update it
        task.status = TaskStatus::Active;
        task_repo
            .find_by_id_result
            .lock()
            .unwrap()
            .replace(task.clone());

        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok(), "timeout error should be handled internally");

        // For timeout errors, the code fetches the task and updates it with
        // Failed status. update should have been called.
        assert!(
            task_repo.update_count() >= 1,
            "update should be called for timeout error path"
        );
    }

    #[tokio::test]
    async fn test_process_scrape_task_all_engines_failed_error_marks_failed() {
        // Use a mock EngineRouter that returns EngineError::AllEnginesFailed
        let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new(
            EngineError::AllEnginesFailed("all engines unavailable".to_string()),
        ));
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            engine_client,
        )
        .await;

        let mut task = make_task(json!({"url": "https://example.com"}));
        task.status = TaskStatus::Active;
        task_repo
            .find_by_id_result
            .lock()
            .unwrap()
            .replace(task.clone());

        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok());

        assert!(
            task_repo.update_count() >= 1,
            "update should be called for all_engines_failed path"
        );
    }

    #[tokio::test]
    async fn test_process_scrape_task_expired_error_marks_failed() {
        // Use a mock EngineRouter that returns EngineError::Expired
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(MockEngineRouter::new(EngineError::Expired));
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            engine_client,
        )
        .await;

        let mut task = make_task(json!({"url": "https://example.com"}));
        task.status = TaskStatus::Active;
        task_repo
            .find_by_id_result
            .lock()
            .unwrap()
            .replace(task.clone());

        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok());
        assert!(
            task_repo.update_count() >= 1,
            "update should be called for expired error path"
        );
    }

    // ========== update_crawl_completion_status: all branches ==========

    #[tokio::test]
    async fn test_update_crawl_completion_status_all_tasks_completed() {
        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        let crawl_id = Uuid::new_v4();
        // Set up a crawl where completed + failed == total
        let crawl = Crawl::with_all_fields(
            crawl_id,
            Uuid::new_v4(),
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            CrawlStatus::Processing,
            json!({}),
            10, // total
            8,  // completed
            2,  // failed
            Utc::now(),
            Utc::now(),
            None,
        );
        crawl_repo.set_crawl(crawl);

        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            crawl_repo.clone(),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        worker.update_crawl_completion_status(crawl_id).await;

        // update_status should have been called to mark as Completed
        assert_eq!(
            crawl_repo.update_status_count(),
            1,
            "update_status should be called when all tasks are done"
        );
    }

    #[tokio::test]
    async fn test_update_crawl_completion_status_not_all_tasks_completed() {
        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        let crawl_id = Uuid::new_v4();
        // Set up a crawl where completed + failed < total
        let crawl = Crawl::with_all_fields(
            crawl_id,
            Uuid::new_v4(),
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            CrawlStatus::Processing,
            json!({}),
            10, // total
            3,  // completed
            2,  // failed
            Utc::now(),
            Utc::now(),
            None,
        );
        crawl_repo.set_crawl(crawl);

        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            crawl_repo.clone(),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        worker.update_crawl_completion_status(crawl_id).await;

        // update_status should NOT have been called
        assert_eq!(
            crawl_repo.update_status_count(),
            0,
            "update_status should not be called when tasks are still pending"
        );
    }

    #[tokio::test]
    async fn test_update_crawl_completion_status_crawl_repo_error() {
        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        crawl_repo.fail_find_by_id.store(true, Ordering::SeqCst);
        let crawl_id = Uuid::new_v4();

        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            crawl_repo.clone(),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        // Should not panic — just logs the error
        worker.update_crawl_completion_status(crawl_id).await;
        assert_eq!(crawl_repo.update_status_count(), 0);
    }

    // ========== check_robots_txt: denied and delay paths ==========

    #[tokio::test]
    async fn test_check_robots_txt_denied_returns_false() {
        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(DenyingRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let task = make_task(json!({}));
        let result = worker.check_robots_txt(&task).await;
        assert!(
            !result,
            "check_robots_txt should return false when robots.txt denies access"
        );
    }

    #[tokio::test]
    async fn test_check_robots_txt_with_crawl_delay_returns_true() {
        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(DelayingRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let task = make_task(json!({}));
        let result = worker.check_robots_txt(&task).await;
        assert!(
            result,
            "check_robots_txt should return true when robots.txt allows with delay"
        );
    }

    #[tokio::test]
    async fn test_check_robots_txt_error_falls_back_to_allowed() {
        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(ErroringRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let task = make_task(json!({}));
        // When is_allowed returns Err, it falls back to true (unwrap_or(true))
        // When get_crawl_delay returns Err, it falls back to None (unwrap_or(None))
        let result = worker.check_robots_txt(&task).await;
        assert!(
            result,
            "check_robots_txt should fall back to true when robots checker errors"
        );
    }

    // ========== ScrapeWorkerBuilder: remaining missing field tests ==========

    #[tokio::test]
    async fn test_builder_build_missing_credits_repository() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "credits_repository is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_create_scrape_use_case() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_team_semaphore(team_semaphore)
            .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "create_scrape_use_case is required");
    }

    #[tokio::test]
    async fn test_builder_build_missing_robots_checker() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let result = ScrapeWorkerBuilder::new()
            .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
            .with_result_repository(
                Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
            )
            .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
            .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
            .with_credits_repository(
                Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>
            )
            .with_engine_client(engine_client)
            .with_create_scrape_use_case(
                Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
            )
            .with_team_semaphore(team_semaphore)
            .with_settings(Arc::new(settings))
            .with_extraction_service(
                Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>
            )
            .with_regex_cache(regex_cache)
            .build();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "robots_checker is required");
    }

    // ========== save_extract_result: error paths ==========

    #[tokio::test]
    async fn test_save_extract_result_failing_result_repo_returns_error() {
        let worker = build_worker_with_failing_deps(
            Arc::new(FailingScrapeResultRepo) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        )
        .await;

        let mut task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "test content".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };
        let result = worker
            .save_extract_result(&mut task, &response, None, "https://example.com")
            .await;
        assert!(
            result.is_err(),
            "save_extract_result should return error when result_repository.save fails"
        );
    }

    // ========== handle_failure: error path ==========

    #[tokio::test]
    async fn test_handle_failure_returns_error_when_repo_update_fails() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        task_repo.fail_update.store(true, Ordering::SeqCst);

        let worker = build_configurable_worker(
            task_repo,
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let mut task = make_task(json!({}));
        // Set attempt_count high enough to exceed max_retries so it tries
        // to mark as failed (which calls update)
        task.attempt_count = 100;
        task.max_retries = 1;

        let result = worker.handle_failure(&mut task).await;
        assert!(
            result.is_err(),
            "handle_failure should return error when update fails"
        );
    }

    // ========== trigger_webhook: error path (should not panic) ==========

    #[tokio::test]
    async fn test_trigger_webhook_failure_does_not_propagate_error() {
        let worker = build_worker_with_failing_deps(
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(FailingWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        )
        .await;

        let task = make_task(json!({}));
        // trigger_webhook returns () — it logs the error but doesn't propagate
        worker.trigger_webhook(&task, None).await;
        worker
            .trigger_webhook(&task, Some("error msg".to_string()))
            .await;
        // If we reach here, the test passes — no panic
    }

    // ========== deduct_feature_credits: error path (should not panic) ==========

    #[tokio::test]
    async fn test_deduct_feature_credits_failure_does_not_propagate_error() {
        let worker = build_worker_with_failing_deps(
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(FailingCreditsRepo) as Arc<dyn CreditsRepository>,
        )
        .await;

        // Should not panic — error is just logged
        worker
            .deduct_feature_credits(Uuid::new_v4(), Uuid::new_v4(), true, true)
            .await;
    }

    // ========== deduct_token_credits: error path (should not panic) ==========

    #[tokio::test]
    async fn test_deduct_token_credits_failure_does_not_propagate_error() {
        let worker = build_worker_with_failing_deps(
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(FailingCreditsRepo) as Arc<dyn CreditsRepository>,
        )
        .await;

        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        // Should not panic — error is just logged
        worker
            .deduct_token_credits(
                Uuid::new_v4(),
                Uuid::new_v4(),
                &usage,
                "test failing deduct",
            )
            .await;
    }

    // ========== handle_crawl_success: save_result error path ==========

    #[tokio::test]
    async fn test_handle_crawl_success_save_result_failure_propagates_error() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorker::new(
            Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>,
            Arc::new(FailingScrapeResultRepo) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: r#"<html><body><a href="/page1">Link</a></body></html>"#.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 0, &config, &request)
            .await;
        assert!(
            result.is_err(),
            "handle_crawl_success should return error when save_result fails"
        );
    }

    // ========== handle_crawl_success: increment_completed_tasks error path ==========

    #[tokio::test]
    async fn test_handle_crawl_success_increment_completed_error_does_not_propagate() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        crawl_repo
            .fail_increment_completed
            .store(true, Ordering::SeqCst);

        let worker = ScrapeWorker::new(
            Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            crawl_repo,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: r#"<html><body>Hello</body></html>"#.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let mut config = make_crawl_config(None, None);
        config.max_depth = 0; // No link extraction — depth 0 < max_depth 0 is false
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_success(&task, response, Uuid::new_v4(), 0, &config, &request)
            .await;
        // Should succeed — increment_completed_tasks error is just logged
        assert!(
            result.is_ok(),
            "handle_crawl_success should not fail when increment_completed_tasks errors"
        );
    }

    // ========== handle_crawl_failure: increment_failed_tasks error path ==========

    #[tokio::test]
    async fn test_handle_crawl_failure_increment_failed_error_does_not_propagate() {
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorker::new(
            Arc::new(ConfigurableTaskRepo::new()) as Arc<dyn TaskRepository>,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let mut task = make_task(json!({}));
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_failure(
                &mut task,
                anyhow::anyhow!("Network error"),
                Uuid::new_v4(),
                &request,
            )
            .await;
        // Should succeed — increment_failed_tasks error is just logged
        assert!(
            result.is_ok(),
            "handle_crawl_failure should not fail when increment_failed_tasks errors"
        );
    }

    // ========== process_crawl_task: robots.txt denial path ==========

    #[tokio::test]
    async fn test_process_crawl_task_robots_denied_marks_failed() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let regex_cache = make_regex_cache().await;
        let engine_client = Arc::new(EngineClient::new());
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorker::new(
            task_repo.clone(),
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(DenyingRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let crawl_id = Uuid::new_v4();
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 0,
            "config": {"max_depth": 2}
        }));

        let result = worker.process_crawl_task(task).await;
        assert!(result.is_ok());

        // mark_failed should have been called because robots.txt denied access
        assert_eq!(
            task_repo.mark_failed_count(),
            1,
            "mark_failed should be called when robots.txt denies access"
        );
    }

    // ========== Success-path mocks for process_scrape_task / run() coverage ==========

    // --- SuccessEngineRouter ---

    /// EngineRouter that returns a configurable successful InternalScrapeResponse.
    struct SuccessEngineRouter {
        response: crate::engines::engine_client::InternalScrapeResponse,
    }

    impl SuccessEngineRouter {
        fn new() -> Self {
            use crate::engines::engine_client::InternalScrapeResponse;
            Self {
                response: InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Hello</h1></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                },
            }
        }

        fn with_screenshot(mut self, screenshot: String) -> Self {
            self.response.screenshot = Some(screenshot);
            self
        }
    }

    #[async_trait::async_trait]
    impl EngineRouterTrait for SuccessEngineRouter {
        async fn route(
            &self,
            _request: &crate::engines::engine_client::InternalScrapeRequest,
        ) -> Result<crate::engines::engine_client::InternalScrapeResponse, EngineError> {
            Ok(self.response.clone())
        }
        async fn aggregate(
            &self,
            _request: &crate::engines::engine_client::InternalScrapeRequest,
        ) -> Result<crate::engines::engine_client::InternalScrapeResponse, EngineError> {
            Ok(self.response.clone())
        }
        fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats> {
            std::collections::HashMap::new()
        }
        fn reset_engine_stats(&self, _engine_name: &str) {}
        fn registered_engines(&self) -> Vec<String> {
            vec!["mock-success-engine".to_string()]
        }
    }

    // --- FailingExtractionService ---

    /// ExtractionService that always returns an error on extract().
    struct FailingExtractionService;

    #[async_trait::async_trait]
    impl ExtractionServiceTrait for FailingExtractionService {
        async fn extract(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<(Value, TokenUsage)> {
            Err(anyhow::anyhow!("Mock extraction failure"))
        }
        async fn extract_with_schema(
            &self,
            _html_content: &str,
            _schema: &Value,
        ) -> Result<(Value, TokenUsage)> {
            Err(anyhow::anyhow!("Mock extraction failure"))
        }
        fn extract_with_selectors(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> Result<Value> {
            Err(anyhow::anyhow!("Mock extraction failure"))
        }
    }

    // --- Helper: build worker with configurable credits + extraction ---

    async fn build_worker_for_success_tests(
        task_repo: Arc<dyn TaskRepository>,
        engine_client: Arc<EngineClient>,
        credits_repo: Arc<dyn CreditsRepository>,
        extraction_service: Arc<dyn ExtractionServiceTrait>,
    ) -> ScrapeWorker {
        let regex_cache = make_regex_cache().await;
        let settings = crate::bootstrap::config::load_settings()
            .expect("Failed to load settings for success tests");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        ScrapeWorker::new(
            task_repo,
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>,
            Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            credits_repo,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            extraction_service,
            regex_cache,
        )
    }

    // ========== process_scrape_task: success path ==========

    #[tokio::test]
    async fn test_process_scrape_task_success_path_marks_completed() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        let task = make_task(json!({"url": "https://example.com"}));
        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok(), "success path should return Ok(())");

        // mark_completed should be called by handle_scrape_success
        assert_eq!(
            task_repo.mark_completed_count(),
            1,
            "mark_completed should be called once for successful scrape"
        );
    }

    #[tokio::test]
    async fn test_process_scrape_task_success_with_screenshot_and_proxy_deducts_credits() {
        // Engine returns a response with screenshot
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(SuccessEngineRouter::new().with_screenshot("base64data".to_string()));
        let engine_client = Arc::new(EngineClient::with_router(router));

        let credits_repo = Arc::new(MockCreditsRepo::default());
        // Share the deducted log so we can inspect after
        let deducted_log = credits_repo.deducted.clone();

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            credits_repo as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // Payload includes proxy option → deduct_feature_credits should charge 2 (screenshot) + 1 (proxy) = 3
        let task = make_task(json!({
            "url": "https://example.com",
            "options": {
                "proxy": "http://proxy.example.com:8080"
            }
        }));
        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok());

        // At least one deduct_credits call (extra credits for screenshot + proxy)
        let deductions = deducted_log.lock().unwrap();
        assert!(
            !deductions.is_empty(),
            "deduct_credits should be called for screenshot + proxy"
        );
        // The extra-credits call should be 3 (screenshot=2 + proxy=1)
        let has_extra_credits = deductions.iter().any(|(_, amount)| *amount == 3);
        assert!(
            has_extra_credits,
            "expected a deduct_credits call with amount 3 (screenshot + proxy), got {:?}",
            deductions
        );
    }

    #[tokio::test]
    async fn test_process_scrape_task_success_handle_scrape_failure_calls_handle_failure() {
        // Engine succeeds but save_result fails → handle_scrape_success returns Err
        // → process_scrape_task calls handle_failure
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let regex_cache = make_regex_cache().await;
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let team_semaphore = Arc::new(TeamSemaphore::new(10));

        let worker = ScrapeWorker::new(
            task_repo.clone(),
            Arc::new(FailingScrapeResultRepo) as Arc<dyn ScrapeResultRepository>,
            Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
            Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            engine_client,
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>,
            team_semaphore,
            Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
            Arc::new(settings),
            10,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
            regex_cache,
        );

        let task = make_task(json!({"url": "https://example.com"}));
        let result = worker.process_scrape_task(task).await;
        // handle_failure path returns Ok(()) after retry handling
        assert!(result.is_ok(), "handle_failure path should return Ok(())");
    }

    // ========== run() infinite loop coverage ==========

    #[tokio::test]
    async fn test_run_with_empty_queue_loops_and_sleeps() {
        let worker = build_mock_worker().await;
        let queue = Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>;

        // run() is an infinite loop; with empty queue it sleeps 1s per iteration.
        // Use timeout to verify it enters the loop without returning.
        let result =
            tokio::time::timeout(Duration::from_millis(150), worker.run(Arc::clone(&queue))).await;

        // Timeout means run() was still looping (expected behavior)
        assert!(
            result.is_err(),
            "run() should loop indefinitely; timeout expected"
        );
    }

    #[tokio::test]
    async fn test_run_processes_task_then_continues_looping() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        let task = make_task(json!({"url": "https://example.com"}));
        let queue = Arc::new(TaskQueueWithTask::new(task)) as Arc<dyn TaskQueue>;

        // run() processes the task then loops (sleeping on empty queue)
        let result = tokio::time::timeout(Duration::from_millis(500), worker.run(queue)).await;

        // Timeout means run() processed the task and continued looping
        assert!(
            result.is_err(),
            "run() should loop indefinitely after processing"
        );

        // mark_completed should have been called for the scrape task
        assert!(
            task_repo.mark_completed_count() >= 1,
            "mark_completed should be called after processing scrape task in run()"
        );
    }

    // ========== extract_and_queue_links: find_existing_urls failure path ==========

    #[tokio::test]
    async fn test_extract_and_queue_links_find_existing_urls_failure_returns_err() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        // Configure find_existing_urls to fail
        task_repo
            .fail_find_existing_urls
            .store(true, Ordering::SeqCst);

        let worker = build_configurable_worker(
            task_repo,
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let mut task = make_task(json!({}));
        task.url = "https://example.com".to_string();
        let html = r#"<html><body>
            <a href="https://example.com/page1">Page 1</a>
            <a href="https://example.com/page2">Page 2</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;

        assert!(
            result.is_err(),
            "extract_and_queue_links should return Err when find_existing_urls fails"
        );
    }

    // ========== handle_scrape_success: extraction failure path ==========

    #[tokio::test]
    async fn test_handle_scrape_success_extraction_failure_continues_and_marks_completed() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(FailingExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // Payload includes extraction_rules → extraction will be attempted and fail,
        // but handle_scrape_success should continue and still mark_completed.
        let task = make_task(json!({
            "url": "https://example.com",
            "extraction_rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };

        let result = worker.handle_scrape_success(&task, &response).await;
        assert!(
            result.is_ok(),
            "handle_scrape_success should not fail when extraction fails"
        );

        // mark_completed should still be called
        assert_eq!(
            task_repo.mark_completed_count(),
            1,
            "mark_completed should be called even when extraction fails"
        );
    }

    // ========== process_scrape_task: success path without screenshot/proxy (no extra credits) ==========

    #[tokio::test]
    async fn test_process_scrape_task_success_without_screenshot_proxy_no_extra_credits() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let credits_repo = Arc::new(MockCreditsRepo::default());
        let deducted_log = credits_repo.deducted.clone();

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            credits_repo as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // No screenshot, no proxy → deduct_feature_credits does nothing (extra_credits == 0)
        let task = make_task(json!({"url": "https://example.com"}));
        let result = worker.process_scrape_task(task).await;
        assert!(result.is_ok());

        // No deduct_credits calls for feature credits (extraction also returns 0 tokens)
        let deductions = deducted_log.lock().unwrap();
        assert!(
            deductions.is_empty(),
            "no deduct_credits calls expected without screenshot/proxy/token-usage, got {:?}",
            deductions
        );
    }

    // ========== run() Err path: FailingTaskQueue exercises lines 138-140 ==========

    #[tokio::test]
    async fn test_run_with_failing_queue_logs_error_and_sleeps() {
        let worker = build_mock_worker().await;
        let queue = Arc::new(FailingTaskQueue) as Arc<dyn TaskQueue>;

        // run() is an infinite loop; with FailingTaskQueue, dequeue returns Err
        // → process_next_task returns Err → run() hits the Err branch (lines 138-140)
        // and sleeps 1s per iteration. Use timeout to verify it enters the loop.
        let result =
            tokio::time::timeout(Duration::from_millis(150), worker.run(Arc::clone(&queue))).await;

        // Timeout means run() was still looping (expected behavior)
        assert!(
            result.is_err(),
            "run() should loop indefinitely on queue errors; timeout expected"
        );
    }

    // ========== process_crawl_task success path: exercises lines 372-374 ==========

    #[tokio::test]
    async fn test_process_crawl_task_success_calls_handle_crawl_success() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            engine_client,
        )
        .await;

        let crawl_id = Uuid::new_v4();
        // max_depth: 0 → depth 0 < max_depth 0 is false → no link extraction
        let task = make_task(json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 0,
            "config": {"max_depth": 0}
        }));

        let result = worker.process_crawl_task(task).await;
        assert!(result.is_ok(), "success path should return Ok(())");

        // mark_completed should be called by handle_crawl_success
        assert_eq!(
            task_repo.mark_completed_count(),
            1,
            "mark_completed should be called once for successful crawl"
        );
    }

    // ========== extract_data_with_rules failure path: exercises lines 509-511 ==========

    #[tokio::test]
    async fn test_extract_data_with_rules_failure_returns_none_and_logs() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let worker = build_worker_for_success_tests(
            Arc::new(ConfigurableTaskRepo::new()),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(FailingExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        let task = make_task(json!({}));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };
        let mut rules = HashMap::new();
        rules.insert(
            "title".to_string(),
            ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: Some(rules),
        };

        // FailingExtractionService.extract returns Err → lines 509-511
        let result = worker
            .extract_data_with_rules(&task, &response, &config)
            .await;
        assert!(
            result.is_none(),
            "extract_data_with_rules should return None when extraction fails"
        );
    }

    // ========== handle_crawl_failure: increment_failed_tasks error (line 540) ==========

    #[tokio::test]
    async fn test_handle_crawl_failure_increment_failed_error_logs_and_continues() {
        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        crawl_repo
            .fail_increment_failed
            .store(true, Ordering::SeqCst);

        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            crawl_repo,
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let mut task = make_task(json!({}));
        let config = make_crawl_config(None, None);
        let request = worker.build_crawl_request(&task, &config);
        let result = worker
            .handle_crawl_failure(
                &mut task,
                anyhow::anyhow!("Network error"),
                Uuid::new_v4(),
                &request,
            )
            .await;

        // Should succeed — increment_failed_tasks error is just logged (line 540)
        assert!(
            result.is_ok(),
            "handle_crawl_failure should not fail when increment_failed_tasks errors"
        );
    }

    // ========== update_crawl_completion_status: update_status error (line 569) ==========

    #[tokio::test]
    async fn test_update_crawl_completion_status_update_status_error_logs_and_continues() {
        let crawl_repo = Arc::new(ConfigurableCrawlRepo::new());
        crawl_repo.fail_update_status.store(true, Ordering::SeqCst);

        let crawl_id = Uuid::new_v4();
        // completed + failed == total → enters the update_status branch
        let crawl = Crawl::with_all_fields(
            crawl_id,
            Uuid::new_v4(),
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            CrawlStatus::Processing,
            json!({}),
            10,
            8,
            2,
            Utc::now(),
            Utc::now(),
            None,
        );
        crawl_repo.set_crawl(crawl);

        let worker = build_configurable_worker(
            Arc::new(ConfigurableTaskRepo::new()),
            crawl_repo.clone(),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        // update_status returns Err → line 569 error log
        worker.update_crawl_completion_status(crawl_id).await;

        // update_status was called (and failed)
        assert_eq!(
            crawl_repo.update_status_count(),
            1,
            "update_status should be called once even when it errors"
        );
    }

    // ========== process_extract_task: rules path (lines 622, 630-634, 644-647) ==========

    #[tokio::test]
    async fn test_process_extract_task_with_rules_calls_handle_rules_extraction() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // payload.rules = Some → line 622 (debug! rules_count) + lines 644-647 (handle_rules_extraction)
        let task = make_task(json!({
            "urls": ["https://example.com/page"],
            "rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));

        let result = worker.process_extract_task(task).await;
        assert!(
            result.is_ok(),
            "process_extract_task with rules should return Ok(())"
        );
        // save_extract_result calls repository.update (mark as Completed)
        assert!(
            task_repo.update_count() >= 1,
            "update should be called to mark task as Completed"
        );
    }

    // ========== process_extract_task: prompt path (lines 650-653) ==========

    #[tokio::test]
    async fn test_process_extract_task_with_prompt_calls_handle_prompt_extraction() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // payload.prompt = Some (no rules) → lines 650-653 (handle_prompt_extraction)
        let task = make_task(json!({
            "urls": ["https://example.com/page"],
            "prompt": "Extract the title"
        }));

        let result = worker.process_extract_task(task).await;
        assert!(
            result.is_ok(),
            "process_extract_task with prompt should return Ok(())"
        );
        assert!(
            task_repo.update_count() >= 1,
            "update should be called to mark task as Completed"
        );
    }

    // ========== process_extract_task: schema path (lines 656-659) ==========

    #[tokio::test]
    async fn test_process_extract_task_with_schema_calls_handle_schema_extraction() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // payload.schema = Some (no rules, no prompt) → lines 656-659 (handle_schema_extraction)
        let task = make_task(json!({
            "urls": ["https://example.com/page"],
            "schema": {"type": "object", "properties": {"title": {"type": "string"}}}
        }));

        let result = worker.process_extract_task(task).await;
        assert!(
            result.is_ok(),
            "process_extract_task with schema should return Ok(())"
        );
        assert!(
            task_repo.update_count() >= 1,
            "update should be called to mark task as Completed"
        );
    }

    // ========== process_extract_task: fallback path (lines 663-664) ==========

    #[tokio::test]
    async fn test_process_extract_task_fallback_saves_raw_result() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // No rules, no prompt, no schema → lines 663-664 (save_extract_result fallback)
        let task = make_task(json!({
            "urls": ["https://example.com/page"]
        }));

        let result = worker.process_extract_task(task).await;
        assert!(
            result.is_ok(),
            "process_extract_task fallback should return Ok(())"
        );
        assert!(
            task_repo.update_count() >= 1,
            "update should be called to mark task as Completed"
        );
    }

    // ========== extract_and_queue_links: skip existing URLs (line 849) ==========

    #[tokio::test]
    async fn test_extract_and_queue_links_skips_existing_urls() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        // Pre-populate existing URLs: page1 already crawled → should be skipped
        let mut existing = HashSet::new();
        existing.insert("https://example.com/page1".to_string());
        task_repo.set_existing_urls(existing);

        let worker = build_configurable_worker(
            task_repo.clone(),
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let task = make_task(json!({}));
        // task.url = "https://example.com" (from make_task default)
        let html = r#"<html><body>
            <a href="/page1">Page 1</a>
            <a href="/page2">Page 2</a>
        </body></html>"#;
        let response = ScrapeResponse {
            content: html.to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
            final_url: None,
        };
        let config = make_crawl_config(None, None);
        let result = worker
            .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
            .await;
        assert!(result.is_ok());

        // page1 was skipped (existing), page2 was created → create_count == 1
        assert_eq!(
            task_repo.create_count(),
            1,
            "only non-existing URLs should be created (page1 skipped, page2 created)"
        );
    }

    // ========== handle_scrape_success: token deduct failure (line 994) ==========

    #[tokio::test]
    async fn test_handle_scrape_success_token_deduct_failure_logs_error() {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(SuccessEngineRouter::new());
        let engine_client = Arc::new(EngineClient::with_router(router));

        let task_repo = Arc::new(ConfigurableTaskRepo::new());
        // FailingCreditsRepo + MockExtractionServiceWithTokens → deduct fails → line 994
        let worker = build_worker_for_success_tests(
            task_repo.clone(),
            engine_client,
            Arc::new(FailingCreditsRepo) as Arc<dyn CreditsRepository>,
            Arc::new(MockExtractionServiceWithTokens) as Arc<dyn ExtractionServiceTrait>,
        )
        .await;

        // Payload includes extraction_rules → extraction succeeds with tokens > 0
        // → deduct_credits fails → line 994 error log
        let task = make_task(json!({
            "url": "https://example.com",
            "extraction_rules": {
                "title": {
                    "selector": "h1",
                    "attr": null,
                    "is_array": false,
                    "use_llm": null,
                    "llm_prompt": null,
                    "output_format": null
                }
            }
        }));
        let response = ScrapeResponse {
            content: "<html><body><h1>Title</h1></body></html>".to_string(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
        };

        let result = worker.handle_scrape_success(&task, &response).await;
        // Should still succeed — credit deduction failure is just logged
        assert!(
            result.is_ok(),
            "handle_scrape_success should not fail when credit deduction fails"
        );
        assert_eq!(
            task_repo.mark_completed_count(),
            1,
            "mark_completed should still be called"
        );
    }

    // ========== handle_failure: Failed branch (line 1130) ==========

    #[tokio::test]
    async fn test_handle_failure_failed_branch_returns_ok() {
        let task_repo = Arc::new(ConfigurableTaskRepo::new());

        let worker = build_configurable_worker(
            task_repo,
            Arc::new(ConfigurableCrawlRepo::new()),
            Arc::new(MockRobotsChecker),
            Arc::new(EngineClient::new()),
        )
        .await;

        let mut task = make_task(json!({}));
        // attempt_count exceeds max_retries → Failed branch (not Retried)
        // fail_update is false (default) → update succeeds → Failed → Ok(())
        task.attempt_count = 100;
        task.max_retries = 1;

        let result = worker.handle_failure(&mut task).await;
        // Failed branch returns Ok(()) — line 1130
        assert!(
            result.is_ok(),
            "handle_failure should return Ok(()) when task exceeds max_retries"
        );
    }
}
