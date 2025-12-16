// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, instrument, warn};
use url::Url;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::usecases::create_scrape::CreateScrapeUseCase;
use crate::domain::models::crawl::CrawlStatus;
use crate::domain::models::scrape_result::ScrapeResult;
use crate::domain::models::task::{Task, TaskStatus, TaskType};
use crate::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::engines::router::EngineRouter;
use crate::engines::traits::{ScrapeRequest, ScrapeResponse, ScreenshotConfig};
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::queue::task_queue::TaskQueue;
use crate::utils::robots::{RobotsChecker, RobotsCheckerTrait};

/// 抓取工作者
pub struct ScrapeWorker<R, S, C, ST>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    ST: StorageRepository + Send + Sync,
{
    repository: Arc<R>,
    result_repository: Arc<S>,
    crawl_repository: Arc<C>,
    storage_repository: Option<Arc<ST>>,
    webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    router: Arc<EngineRouter>,
    create_scrape_use_case: Arc<CreateScrapeUseCase>,
    #[allow(dead_code)]
    redis: RedisClient,
    robots_checker: Arc<RobotsChecker>,
    worker_id: Uuid,
}

impl<R, S, C, ST> ScrapeWorker<R, S, C, ST>
where
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    ST: StorageRepository + Send + Sync,
{
    /// 创建新的抓取工作器实例
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Arc<R>,
        result_repository: Arc<S>,
        crawl_repository: Arc<C>,
        storage_repository: Option<Arc<ST>>,
        webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
        router: Arc<EngineRouter>,
        create_scrape_use_case: Arc<CreateScrapeUseCase>,
        redis: RedisClient,
        robots_checker: Arc<RobotsChecker>,
    ) -> Self {
        Self {
            repository,
            result_repository,
            crawl_repository,
            storage_repository,
            webhook_event_repository,
            router,
            create_scrape_use_case,
            redis,
            robots_checker,
            worker_id: Uuid::new_v4(),
        }
    }

    /// 运行抓取工作器
    pub async fn run<Q>(&self, queue: Arc<Q>)
    where
        Q: TaskQueue + Send + Sync,
    {
        info!("Scrape worker {} started", self.worker_id);

        loop {
            match self.process_next_task(queue.as_ref()).await {
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

    async fn process_next_task<Q>(&self, _queue: &Q) -> Result<bool>
    where
        Q: TaskQueue,
    {
        let task_opt = self.repository.acquire_next(self.worker_id).await?;

        if let Some(task) = task_opt {
            self.process_task(task).await?;
            return Ok(true);
        }

        Ok(false)
    }

    #[instrument(skip(self, task), fields(task_id = %task.id, url = %task.url, task_type = %task.task_type))]
    async fn process_task(&self, task: Task) -> Result<()> {
        info!("Processing task");

        match task.task_type {
            TaskType::Scrape => self.process_scrape_task(task).await,
            TaskType::Crawl => self.process_crawl_task(task).await,
            _ => {
                warn!("Unsupported task type: {:?}", task.task_type);
                self.repository.mark_failed(task.id).await?;
                Ok(())
            }
        }
    }

    async fn process_scrape_task(&self, mut task: Task) -> Result<()> {
        let request_dto = match serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
            Ok(dto) => dto,
            Err(e) => {
                error!("Failed to deserialize ScrapeRequestDto: {}", e);
                self.handle_failure(&mut task).await?;
                return Ok(());
            }
        };

        let response = self.create_scrape_use_case.execute(request_dto).await;

        match response {
            Ok(response) => {
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

                if let Err(e) = self.handle_scrape_success(task.clone(), &response).await {
                    error!("Scrape success handler failed: {}", e);
                    self.handle_failure(&mut task).await?;
                } else {
                    // Mark as completed only if handle_scrape_success succeeded
                    // handle_scrape_success calls mark_completed internally?
                    // Let's check handle_scrape_success implementation.
                    // It does: self.repository.mark_completed(task.id).await?;
                }
                Ok(())
            }
            Err(e) => {
                error!("Scrape failed: {}", e);
                self.handle_failure(&mut task).await?;
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
        let mut headers = HashMap::new();
        if let Some(h) = &config.headers {
            if let Some(obj) = h.as_object() {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        headers.insert(k.clone(), s.to_string());
                    }
                }
            }
        }

        let request = ScrapeRequest {
            url: task.url.clone(),
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
        };

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

        let response = self.router.route(&request).await;

        match response {
            Ok(response) => {
                info!(
                    "Crawl step successful, url: {}, status: {}",
                    task.url, response.status_code
                );

                // Map ScrapeResponse to ScrapeResult (No need to map here, we use ScrapeResponse directly)

                // 3. 执行数据提取（如果配置了提取规则）
                let mut extracted_data = None;
                if let Some(rules) = &config.extraction_rules {
                    match crate::domain::services::extraction_service::ExtractionService::extract(
                        &response.content,
                        rules,
                    )
                    .await
                    {
                        Ok(data) => {
                            extracted_data = Some(data);
                        }
                        Err(e) => {
                            error!("Extraction failed for url {}: {}", task.url, e);
                        }
                    }
                }

                // 4. 保存结果
                self.save_result(&task, &response, extracted_data).await?;

                // 5. 如果深度未达上限，解析链接并生成子任务
                if depth < config.max_depth {
                    self.extract_and_queue_links(&task, &response, crawl_id, depth, &config)
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

                Ok(())
            }
            Err(e) => {
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
            let selector = Selector::parse("a").unwrap();
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
                if let Ok(re) = Regex::new(pattern) {
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
                if let Ok(re) = Regex::new(pattern) {
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

    async fn handle_scrape_success(&self, task: Task, response: &ScrapeResponse) -> Result<()> {
        // 解析 ScrapeRequest 以检查是否有提取规则
        let mut extracted_data = None;
        if let Ok(req) = serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
            if let Some(rules) = &req.extraction_rules {
                match crate::domain::services::extraction_service::ExtractionService::extract(
                    &response.content,
                    rules,
                )
                .await
                {
                    Ok(data) => {
                        extracted_data = Some(data);
                    }
                    Err(e) => {
                        error!("Extraction failed for url {}: {}", task.url, e);
                    }
                }
            }
        }

        self.save_result(&task, response, extracted_data).await?;
        self.repository.mark_completed(task.id).await?;

        self.trigger_webhook(&task, WebhookEventType::ScrapeCompleted, None)
            .await;
        Ok(())
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
                "timestamp": Utc::now()
            });

            if let Some(msg) = error_msg {
                payload["error"] = json!(msg);
            }

            let event = WebhookEvent {
                id: Uuid::new_v4(),
                team_id: task.team_id,
                webhook_id: Uuid::nil(),
                event_type,
                payload,
                webhook_url: url,
                status: WebhookStatus::Pending,
                attempt_count: 0,
                max_retries: 3,
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

        if new_attempt_count >= task.max_retries {
            warn!("Task failed after {} retries", task.max_retries);
            self.repository.mark_failed(task.id).await?;
        } else {
            let delay_secs = 2u64.pow(new_attempt_count as u32);
            let next_retry = Utc::now() + chrono::Duration::seconds(delay_secs as i64);

            task.attempt_count = new_attempt_count;
            task.scheduled_at = Some(next_retry.into());
            task.status = TaskStatus::Queued;

            self.repository.update(task).await?;
            info!(
                "Scheduled retry {}/{} for task {} in {}s",
                new_attempt_count, task.max_retries, task.id, delay_secs
            );
        }

        Ok(())
    }

    pub fn build_scrape_request(task: &Task) -> Result<ScrapeRequest> {
        let scrape_request: ScrapeRequestDto =
            serde_json::from_value(task.payload.clone()).context("Failed to parse task payload")?;

        let mut headers = HashMap::new();
        let options = scrape_request.options.as_ref();
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

        let mut screenshot_config = None;
        if let Some(opts) = options {
            if let Some(s_opts) = &opts.screenshot_options {
                screenshot_config = Some(ScreenshotConfig {
                    full_page: s_opts.full_page.unwrap_or(true),
                    selector: s_opts.selector.clone(),
                    quality: s_opts.quality,
                    format: s_opts.format.clone(),
                });
            } else if opts.screenshot.unwrap_or(false) {
                // Default screenshot config if screenshot is requested but no options provided
                screenshot_config = Some(ScreenshotConfig::default());
            }
        }

        Ok(ScrapeRequest {
            url: scrape_request.url.clone(),
            headers,
            timeout: Duration::from_secs(options.and_then(|o| o.timeout).unwrap_or(30)),
            needs_js: options.and_then(|o| o.js_rendering).unwrap_or(false),
            needs_screenshot: options.and_then(|o| o.screenshot).unwrap_or(false),
            screenshot_config,
            mobile: options.and_then(|o| o.mobile).unwrap_or(false),
            proxy: options.and_then(|o| o.proxy.clone()),
            skip_tls_verification: options
                .and_then(|o| o.skip_tls_verification)
                .unwrap_or(false),
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        })
    }
}
