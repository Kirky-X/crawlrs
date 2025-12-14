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
use crate::utils::url_utils;

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
    #[allow(dead_code)]
    redis: RedisClient,
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
    pub fn new(
        repository: Arc<R>,
        result_repository: Arc<S>,
        crawl_repository: Arc<C>,
        storage_repository: Option<Arc<ST>>,
        webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
        router: Arc<EngineRouter>,
        redis: RedisClient,
    ) -> Self {
        Self {
            repository,
            result_repository,
            crawl_repository,
            storage_repository,
            webhook_event_repository,
            router,
            redis,
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
        let request = match Self::build_scrape_request(&task) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to build scrape request: {}", e);
                self.handle_failure(&mut task).await?;
                return Ok(());
            }
        };

        let response = self.router.route(&request).await;

        match response {
            Ok(response) => {
                info!("Scrape successful, status: {}", response.status_code);
                self.handle_scrape_success(task, response).await
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
                    if let Ok(absolute_url) = url_utils::resolve_url(&base_url, href) {
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

        for link in unique_links {
            // 检查是否已经抓取过 (去重)
            // 这里简单使用 repository 检查 URL 是否存在
            // 在大规模系统中可能需要 BloomFilter 或 Redis Set
            if self.repository.exists_by_url(&link).await? {
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
                url: link,
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

    async fn handle_scrape_success(&self, task: Task, response: ScrapeResponse) -> Result<()> {
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

        self.save_result(&task, &response, extracted_data).await?;
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

        let result = ScrapeResult {
            id: Uuid::new_v4(),
            task_id: task.id,
            url: task.url.clone(),
            status_code: response.status_code,
            content: response.content.clone(),
            content_type: response
                .headers
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| "text/html".to_string()),
            headers: serde_json::to_value(&response.headers).unwrap_or(Value::Null),
            meta_data,
            created_at: Utc::now(),
            screenshot: response.screenshot.clone(),
            response_time_ms: response.response_time_ms,
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
        if let Some(h) = &scrape_request.headers {
            if let Some(obj) = h.as_object() {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        headers.insert(k.clone(), s.to_string());
                    }
                }
            }
        }

        let screenshot_config = if let Some(options) = &scrape_request.screenshot_options {
            Some(ScreenshotConfig {
                full_page: options.full_page.unwrap_or(true),
                selector: options.selector.clone(),
                quality: options.quality,
                format: options.format.clone(),
            })
        } else if scrape_request.screenshot.unwrap_or(false) {
            Some(ScreenshotConfig::default())
        } else {
            None
        };

        Ok(ScrapeRequest {
            url: scrape_request.url,
            headers,
            timeout: Duration::from_secs(scrape_request.timeout.unwrap_or(30)),
            needs_js: scrape_request.js_rendering.unwrap_or(false),
            needs_screenshot: screenshot_config.is_some(),
            screenshot_config,
            mobile: scrape_request.mobile.unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::crawl::{Crawl, CrawlStatus};
    use crate::domain::models::task::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::storage_repository::{StorageError, StorageRepository};
    use crate::domain::repositories::task_repository::RepositoryError;
    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    // Mock repositories for testing
    struct MockTaskRepository;

    #[async_trait]
    impl crate::domain::repositories::task_repository::TaskRepository for MockTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Ok(_task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            unimplemented!()
        }
        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Ok(_task.clone())
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
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    struct MockScrapeResultRepository;

    #[async_trait]
    impl crate::domain::repositories::scrape_result_repository::ScrapeResultRepository
        for MockScrapeResultRepository
    {
        async fn save(&self, _result: ScrapeResult) -> Result<()> {
            Ok(())
        }
    }

    struct MockCrawlRepository;

    #[async_trait]
    impl crate::domain::repositories::crawl_repository::CrawlRepository for MockCrawlRepository {
        async fn create(&self, _crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(_crawl.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            unimplemented!()
        }
        async fn update(&self, _crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(_crawl.clone())
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
    }

    struct MockStorageRepository;

    #[async_trait]
    impl StorageRepository for MockStorageRepository {
        async fn save(&self, _key: &str, _data: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(None)
        }
        async fn delete(&self, _key: &str) -> Result<(), StorageError> {
            Ok(())
        }
        async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
            Ok(false)
        }
    }

    struct MockWebhookRepository;

    #[allow(dead_code)]
    #[async_trait]
    impl crate::domain::repositories::webhook_event_repository::WebhookEventRepository
        for MockWebhookRepository
    {
        async fn create(&self, _event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(_event.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            unimplemented!()
        }
        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            unimplemented!()
        }
        async fn update(&self, _event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(_event.clone())
        }
    }

    fn create_test_task(payload: serde_json::Value) -> Task {
        Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
            task_type: TaskType::Scrape,
            priority: 0,
            payload,
            scheduled_at: None,
            status: TaskStatus::Queued,
            attempt_count: 0,
            max_retries: 3,
            created_at: Utc::now().into(),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: Utc::now().into(),
            lock_token: None,
            lock_expires_at: None,
        }
    }

    #[test]
    fn test_build_scrape_request_defaults() {
        let payload = json!({
            "url": "http://example.com"
        });
        let task = create_test_task(payload);

        let request = ScrapeWorker::<
            MockTaskRepository,
            MockScrapeResultRepository,
            MockCrawlRepository,
            MockStorageRepository,
        >::build_scrape_request(&task)
        .unwrap();

        assert_eq!(request.url, "http://example.com");
    }
}
