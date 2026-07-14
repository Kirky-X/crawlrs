// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 抓取领域服务
//!
//! 封装抓取业务逻辑，协调 EngineClient 和 TaskRepository。
//! - scrape() 调用 EngineClient 执行抓取
//! - scrape_batch() 并发批量抓取
//! - cancel_scrape() 取消任务
//! - get_scrape_status() 查询任务状态

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::application::dto::scrape_request::{
    ScrapeActionDto, ScrapeOptionsDto, ScrapeRequestDto,
};
use crate::domain::models::TaskStatus;
use crate::domain::repositories::task_repository::{RepositoryError, TaskRepository};
use crate::engines::engine_client::{
    EngineClientTrait, HttpMethod, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
    ScreenshotConfig, ScrollDirection,
};

/// 抓取状态枚举
///
/// 表示抓取任务在 service 层的状态视图，对应底层 TaskStatus。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapeStatus {
    /// 待处理（对应 TaskStatus::Queued）
    Pending,
    /// 运行中（对应 TaskStatus::Active）
    Running,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已取消
    Cancelled,
}

impl From<TaskStatus> for ScrapeStatus {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Queued => ScrapeStatus::Pending,
            TaskStatus::Active => ScrapeStatus::Running,
            TaskStatus::Completed => ScrapeStatus::Completed,
            TaskStatus::Failed => ScrapeStatus::Failed,
            TaskStatus::Cancelled => ScrapeStatus::Cancelled,
        }
    }
}

impl std::fmt::Display for ScrapeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrapeStatus::Pending => write!(f, "pending"),
            ScrapeStatus::Running => write!(f, "running"),
            ScrapeStatus::Completed => write!(f, "completed"),
            ScrapeStatus::Failed => write!(f, "failed"),
            ScrapeStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// 抓取服务 trait
///
/// 定义抓取领域的业务接口，支持依赖注入。
#[async_trait]
pub trait ScrapeService: Send + Sync {
    /// 执行单次抓取
    ///
    /// # 参数
    /// * `request` - 抓取请求 DTO
    ///
    /// # 返回
    /// * `Ok(ScrapeResponse)` - 抓取成功
    /// * `Err(anyhow::Error)` - 抓取失败（引擎错误、URL 验证错误等）
    async fn scrape(&self, request: ScrapeRequestDto) -> Result<ScrapeResponse>;

    /// 批量抓取
    ///
    /// 并发执行多个抓取请求。任意请求失败将导致整个批次返回 Err，
    /// 错误信息包含首个失败详情。
    ///
    /// # 参数
    /// * `requests` - 抓取请求 DTO 列表
    ///
    /// # 返回
    /// * `Ok(Vec<ScrapeResponse>)` - 全部成功（按输入顺序）
    /// * `Err(anyhow::Error)` - 至少一个失败
    async fn scrape_batch(&self, requests: Vec<ScrapeRequestDto>) -> Result<Vec<ScrapeResponse>>;

    /// 取消抓取任务
    ///
    /// # 参数
    /// * `task_id` - 任务 ID
    ///
    /// # 返回
    /// * `Ok(())` - 取消成功
    /// * `Err(anyhow::Error)` - 任务不存在或取消失败
    async fn cancel_scrape(&self, task_id: Uuid) -> Result<()>;

    /// 查询抓取状态
    ///
    /// # 参数
    /// * `task_id` - 任务 ID
    ///
    /// # 返回
    /// * `Ok(ScrapeStatus)` - 任务状态
    /// * `Err(anyhow::Error)` - 任务不存在或查询失败
    async fn get_scrape_status(&self, task_id: Uuid) -> Result<ScrapeStatus>;
}

/// 抓取服务实现
pub struct ScrapeServiceImpl {
    /// 引擎客户端
    engine_client: Arc<dyn EngineClientTrait>,
    /// 任务仓库
    task_repository: Arc<dyn TaskRepository>,
}

impl ScrapeServiceImpl {
    /// 创建新的抓取服务实现
    pub fn new(
        engine_client: Arc<dyn EngineClientTrait>,
        task_repository: Arc<dyn TaskRepository>,
    ) -> Self {
        Self {
            engine_client,
            task_repository,
        }
    }
}

#[async_trait]
impl ScrapeService for ScrapeServiceImpl {
    async fn scrape(&self, request: ScrapeRequestDto) -> Result<ScrapeResponse> {
        let scrape_request = map_dto_to_request(request)?;
        log::info!(
            "ScrapeService: executing scrape for url={}",
            scrape_request.url
        );
        self.engine_client
            .scrape(&scrape_request)
            .await
            .map_err(|e| {
                log::error!(
                    "ScrapeService: scrape failed for url={}: {}",
                    scrape_request.url,
                    e
                );
                anyhow!(e.to_string())
            })
    }

    async fn scrape_batch(&self, requests: Vec<ScrapeRequestDto>) -> Result<Vec<ScrapeResponse>> {
        let total = requests.len();
        if total == 0 {
            return Ok(Vec::new());
        }
        log::info!("ScrapeService: batch scrape {} requests", total);

        // 并发执行：每个请求独立 spawn，按完成顺序收集，但最终按输入索引对齐
        let mut handles = Vec::with_capacity(total);
        for dto in requests {
            let engine = self.engine_client.clone();
            handles.push(tokio::spawn(async move {
                let req = map_dto_to_request(dto)?;
                engine
                    .scrape(&req)
                    .await
                    .map_err(|e| anyhow!(e.to_string()))
            }));
        }

        let mut results = Vec::with_capacity(total);
        let mut first_error: Option<anyhow::Error> = None;
        for (idx, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(resp)) => results.push(resp),
                Ok(Err(e)) => {
                    log::error!("ScrapeService: batch scrape request {} failed: {}", idx, e);
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
                Err(e) => {
                    log::error!(
                        "ScrapeService: batch scrape request {} join error: {}",
                        idx,
                        e
                    );
                    if first_error.is_none() {
                        first_error = Some(anyhow!("join error: {}", e));
                    }
                }
            }
        }

        if let Some(e) = first_error {
            log::error!(
                "ScrapeService: batch scrape failed ({} of {} requests)",
                e,
                total
            );
            return Err(e);
        }

        log::info!(
            "ScrapeService: batch scrape completed {} requests",
            results.len()
        );
        Ok(results)
    }

    async fn cancel_scrape(&self, task_id: Uuid) -> Result<()> {
        log::info!("ScrapeService: cancelling task {}", task_id);
        self.task_repository
            .mark_cancelled(task_id)
            .await
            .map_err(map_repo_error)
    }

    async fn get_scrape_status(&self, task_id: Uuid) -> Result<ScrapeStatus> {
        let task = self
            .task_repository
            .find_by_id(task_id)
            .await
            .map_err(map_repo_error)?;
        match task {
            Some(t) => {
                let status = ScrapeStatus::from(t.status);
                log::debug!("ScrapeService: task {} status = {}", task_id, status);
                Ok(status)
            }
            None => {
                log::warn!("ScrapeService: task {} not found", task_id);
                Err(anyhow!("task {} not found", task_id))
            }
        }
    }
}

/// 将 ScrapeRequestDto 转换为 ScrapeRequest（与 create_scrape.rs 同模式）
fn map_dto_to_request(dto: ScrapeRequestDto) -> Result<ScrapeRequest> {
    let options = dto.options.unwrap_or(ScrapeOptionsDto {
        headers: None,
        wait_for: None,
        timeout: None,
        js_rendering: None,
        screenshot: None,
        screenshot_options: None,
        mobile: None,
        proxy: None,
        skip_tls_verification: None,
        needs_tls_fingerprint: None,
        use_fire_engine: None,
    });

    let headers = parse_headers(options.headers)?;
    let screenshot_config = options.screenshot_options.map(|opts| ScreenshotConfig {
        full_page: opts.full_page.unwrap_or(false),
        selector: opts.selector,
        quality: opts.quality,
        format: opts.format,
    });

    let scrape_options = ScrapeOptions {
        method: HttpMethod::Get,
        needs_js: options.js_rendering.unwrap_or(false),
        needs_screenshot: options.screenshot.unwrap_or(false),
        mobile: options.mobile.unwrap_or(false),
        timeout: Duration::from_secs(options.timeout.unwrap_or(30)),
        body: None,
        sync_wait_ms: dto.sync_wait_ms.unwrap_or(0),
        actions: parse_actions(dto.actions),
        screenshot_config,
        proxy: options.proxy,
        skip_tls_verification: options.skip_tls_verification.unwrap_or(false),
        headers,
        needs_tls_fingerprint: options.needs_tls_fingerprint.unwrap_or(false),
        use_fire_engine: options.use_fire_engine.unwrap_or(false),
    };

    Ok(ScrapeRequest::new(dto.url).with_options(scrape_options))
}

/// 解析 actions DTO 为引擎 PageAction（与 create_scrape.rs 同模式）
fn parse_actions(dto_actions: Option<Vec<ScrapeActionDto>>) -> Vec<PageAction> {
    dto_actions
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| match a {
            ScrapeActionDto::Wait { milliseconds } => Some(PageAction::Wait { milliseconds }),
            ScrapeActionDto::Click { selector } => Some(PageAction::Click { selector }),
            ScrapeActionDto::Scroll { direction } => {
                let rust_direction = match direction.as_str() {
                    "up" => ScrollDirection::Up,
                    "down" => ScrollDirection::Down,
                    "top" => ScrollDirection::Top,
                    "bottom" => ScrollDirection::Bottom,
                    _ => ScrollDirection::Down,
                };
                Some(PageAction::Scroll {
                    direction: rust_direction,
                })
            }
            ScrapeActionDto::Screenshot { .. } => None,
            ScrapeActionDto::Input { selector, text } => Some(PageAction::Input { selector, text }),
        })
        .collect()
}

/// 解析 headers JSON 为 HashMap（与 create_scrape.rs 同模式）
fn parse_headers(headers_value: Option<Value>) -> Result<HashMap<String, String>> {
    match headers_value {
        Some(Value::Object(map)) => map
            .into_iter()
            .map(|(k, v)| {
                let v_str = v
                    .as_str()
                    .ok_or_else(|| anyhow!("Invalid header value for key: {}", k))?;
                Ok((k, v_str.to_string()))
            })
            .collect(),
        Some(_) => Err(anyhow!("Headers must be a map of string key-value pairs")),
        None => Ok(HashMap::new()),
    }
}

/// 将 RepositoryError 映射为 anyhow::Error
fn map_repo_error(e: RepositoryError) -> anyhow::Error {
    match e {
        RepositoryError::Database(msg) => anyhow!("Database error: {}", msg),
        RepositoryError::NotFound => anyhow!("Record not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::TaskQueryParams;
    use crate::engines::engine_client::{EngineError, EngineHealthStatus};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // ============ Mock EngineClient ============

    /// 可配置的 EngineClient mock
    struct MockEngineClient {
        /// scrape 调用次数
        scrape_calls: AtomicU32,
        /// 预设的响应（按调用顺序消费）
        responses: Mutex<Vec<Result<ScrapeResponse, EngineError>>>,
    }

    impl MockEngineClient {
        fn new(responses: Vec<Result<ScrapeResponse, EngineError>>) -> Self {
            Self {
                scrape_calls: AtomicU32::new(0),
                responses: Mutex::new(responses),
            }
        }

        /// 始终成功的 mock
        fn always_ok(content: &str) -> Self {
            Self::new(vec![Ok(ScrapeResponse::new(200, content, "text/html"))])
        }

        /// 始终失败的 mock
        fn always_err(err: EngineError) -> Self {
            Self::new(vec![Err(err)])
        }
    }

    #[async_trait]
    impl EngineClientTrait for MockEngineClient {
        async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
            let count = self.scrape_calls.fetch_add(1, Ordering::SeqCst) as usize;
            let responses = self.responses.lock().unwrap();
            if responses.len() > count {
                // 克隆结果以避免所有权问题
                match &responses[count] {
                    Ok(resp) => Ok(resp.clone()),
                    Err(e) => Err(clone_engine_error(e)),
                }
            } else if !responses.is_empty() {
                // 重复最后一个
                match &responses[responses.len() - 1] {
                    Ok(resp) => Ok(resp.clone()),
                    Err(e) => Err(clone_engine_error(e)),
                }
            } else {
                Ok(ScrapeResponse::new(200, "default", "text/html"))
            }
        }

        async fn health_check(&self) -> EngineHealthStatus {
            EngineHealthStatus::Healthy
        }

        fn engine_count(&self) -> usize {
            0
        }

        fn registered_engines(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// 克隆 EngineError（EngineError 没有实现 Clone）
    fn clone_engine_error(e: &EngineError) -> EngineError {
        match e {
            EngineError::RequestFailed(msg) => EngineError::RequestFailed(msg.clone()),
            EngineError::Timeout(d) => EngineError::Timeout(*d),
            EngineError::AllEnginesFailed(msg) => EngineError::AllEnginesFailed(msg.clone()),
            EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
            EngineError::InvalidUrl(msg) => EngineError::InvalidUrl(msg.clone()),
            EngineError::SsrfProtection(msg) => EngineError::SsrfProtection(msg.clone()),
            EngineError::BrowserError(msg) => EngineError::BrowserError(msg.clone()),
            EngineError::Expired => EngineError::Expired,
            EngineError::Other(msg) => EngineError::Other(msg.clone()),
            EngineError::Internal(msg) => EngineError::Internal(msg.clone()),
        }
    }

    // ============ Mock TaskRepository ============

    /// 可配置的 TaskRepository mock
    struct MockTaskRepository {
        /// find_by_id 返回的任务映射
        tasks: Mutex<std::collections::HashMap<Uuid, Task>>,
        /// mark_cancelled 调用次数
        cancel_calls: AtomicU32,
        /// mark_cancelled 是否失败
        cancel_should_fail: std::sync::atomic::AtomicBool,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                tasks: Mutex::new(std::collections::HashMap::new()),
                cancel_calls: AtomicU32::new(0),
                cancel_should_fail: std::sync::atomic::AtomicBool::new(false),
            }
        }

        fn with_task(self, task: Task) -> Self {
            self.tasks.lock().unwrap().insert(task.id, task);
            self
        }

        fn set_cancel_fails(self, fails: bool) -> Self {
            self.cancel_should_fail.store(fails, Ordering::SeqCst);
            self
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(self.tasks.lock().unwrap().get(&id).cloned())
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
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            if self.cancel_should_fail.load(Ordering::SeqCst) {
                Err(RepositoryError::Database(anyhow::anyhow!("cancel failed")))
            } else {
                Ok(())
            }
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
            Ok(Vec::new())
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((Vec::new(), 0))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((Vec::new(), Vec::new()))
        }
    }

    // ============ Helpers ============

    fn make_dto(url: &str) -> ScrapeRequestDto {
        ScrapeRequestDto {
            url: url.to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: None,
            metadata: None,
            sync_wait_ms: None,
        }
    }

    fn make_service(
        engine: Arc<MockEngineClient>,
        repo: Arc<MockTaskRepository>,
    ) -> ScrapeServiceImpl {
        ScrapeServiceImpl::new(
            engine as Arc<dyn EngineClientTrait>,
            repo as Arc<dyn TaskRepository>,
        )
    }

    fn make_task(id: Uuid, status: TaskStatus) -> Task {
        let mut task = Task::new(
            id,
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        task.status = status;
        task
    }

    // ============ ScrapeStatus tests ============

    #[test]
    fn test_scrape_status_from_task_status_queued_maps_to_pending() {
        assert_eq!(
            ScrapeStatus::from(TaskStatus::Queued),
            ScrapeStatus::Pending
        );
    }

    #[test]
    fn test_scrape_status_from_task_status_active_maps_to_running() {
        assert_eq!(
            ScrapeStatus::from(TaskStatus::Active),
            ScrapeStatus::Running
        );
    }

    #[test]
    fn test_scrape_status_from_task_status_completed_maps_to_completed() {
        assert_eq!(
            ScrapeStatus::from(TaskStatus::Completed),
            ScrapeStatus::Completed
        );
    }

    #[test]
    fn test_scrape_status_from_task_status_failed_maps_to_failed() {
        assert_eq!(ScrapeStatus::from(TaskStatus::Failed), ScrapeStatus::Failed);
    }

    #[test]
    fn test_scrape_status_from_task_status_cancelled_maps_to_cancelled() {
        assert_eq!(
            ScrapeStatus::from(TaskStatus::Cancelled),
            ScrapeStatus::Cancelled
        );
    }

    #[test]
    fn test_scrape_status_display_all_variants() {
        assert_eq!(ScrapeStatus::Pending.to_string(), "pending");
        assert_eq!(ScrapeStatus::Running.to_string(), "running");
        assert_eq!(ScrapeStatus::Completed.to_string(), "completed");
        assert_eq!(ScrapeStatus::Failed.to_string(), "failed");
        assert_eq!(ScrapeStatus::Cancelled.to_string(), "cancelled");
    }

    // ============ map_dto_to_request tests ============

    #[test]
    fn test_map_dto_to_request_minimal_uses_defaults() {
        let dto = make_dto("https://example.com");

        let request = map_dto_to_request(dto).expect("minimal dto should map");

        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.options.method, HttpMethod::Get);
        assert!(!request.options.needs_js);
        assert!(!request.options.needs_screenshot);
        assert!(!request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(30));
        assert!(request.options.actions.is_empty());
        assert!(request.options.screenshot_config.is_none());
        assert!(request.options.proxy.is_none());
        assert!(request.options.headers.is_empty());
    }

    #[test]
    fn test_map_dto_to_request_full_options() {
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: Some(serde_json::json!({"X-Custom": "value"})),
                wait_for: None,
                timeout: Some(60),
                js_rendering: Some(true),
                screenshot: Some(true),
                screenshot_options: None,
                mobile: Some(true),
                proxy: Some("http://proxy:8080".to_string()),
                skip_tls_verification: Some(false),
                needs_tls_fingerprint: Some(true),
                use_fire_engine: Some(true),
            }),
            metadata: None,
            sync_wait_ms: Some(500),
        };

        let request = map_dto_to_request(dto).expect("full options should map");

        assert!(request.options.needs_js);
        assert!(request.options.needs_screenshot);
        assert!(request.options.mobile);
        assert_eq!(request.options.timeout, Duration::from_secs(60));
        assert_eq!(request.options.sync_wait_ms, 500);
        assert_eq!(request.options.proxy.as_deref(), Some("http://proxy:8080"));
        assert!(request.options.needs_tls_fingerprint);
        assert!(request.options.use_fire_engine);
        assert_eq!(
            request.options.headers.get("X-Custom").map(|v| v.as_str()),
            Some("value")
        );
    }

    #[test]
    fn test_map_dto_to_request_invalid_headers_returns_error() {
        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: Some(serde_json::json!(123)),
                wait_for: None,
                timeout: None,
                js_rendering: None,
                screenshot: None,
                screenshot_options: None,
                mobile: None,
                proxy: None,
                skip_tls_verification: None,
                needs_tls_fingerprint: None,
                use_fire_engine: None,
            }),
            metadata: None,
            sync_wait_ms: None,
        };

        let result = map_dto_to_request(dto);
        let err = result.expect_err("non-object headers should error");
        assert!(
            err.to_string().contains("Headers must be a map"),
            "got: {}",
            err
        );
    }

    // ============ scrape() tests ============

    #[tokio::test]
    async fn test_scrape_success_returns_response() {
        let engine = Arc::new(MockEngineClient::new(vec![Ok(ScrapeResponse::new(
            200,
            "<html>hello</html>",
            "text/html",
        ))]));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine.clone(), repo);

        let response = service
            .scrape(make_dto("https://example.com"))
            .await
            .expect("scrape should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "<html>hello</html>");
        assert_eq!(response.content_type, "text/html");
        assert_eq!(engine.scrape_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_scrape_engine_error_propagates() {
        let engine = Arc::new(MockEngineClient::always_err(
            EngineError::NoEnginesAvailable,
        ));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine, repo);

        let result = service.scrape(make_dto("https://example.com")).await;
        let err = result.expect_err("engine failure should propagate");
        assert!(
            err.to_string().contains("No engines available"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_scrape_ssrf_error_propagates() {
        let engine = Arc::new(MockEngineClient::always_err(EngineError::SsrfProtection(
            "blocked localhost".to_string(),
        )));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine, repo);

        let result = service.scrape(make_dto("http://localhost")).await;
        let err = result.expect_err("SSRF should propagate");
        assert!(err.to_string().contains("SSRF protection"), "got: {}", err);
    }

    #[tokio::test]
    async fn test_scrape_invalid_headers_returns_validation_error_before_engine() {
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine.clone(), repo);

        let dto = ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: Some(ScrapeOptionsDto {
                headers: Some(serde_json::json!("not-a-map")),
                wait_for: None,
                timeout: None,
                js_rendering: None,
                screenshot: None,
                screenshot_options: None,
                mobile: None,
                proxy: None,
                skip_tls_verification: None,
                needs_tls_fingerprint: None,
                use_fire_engine: None,
            }),
            metadata: None,
            sync_wait_ms: None,
        };

        let result = service.scrape(dto).await;
        assert!(result.is_err());
        // Engine should not be called because mapping fails first
        assert_eq!(engine.scrape_calls.load(Ordering::SeqCst), 0);
    }

    // ============ scrape_batch() tests ============

    #[tokio::test]
    async fn test_scrape_batch_empty_returns_empty_vec() {
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine, repo);

        let result = service.scrape_batch(Vec::new()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_scrape_batch_all_succeed() {
        let engine = Arc::new(MockEngineClient::new(vec![
            Ok(ScrapeResponse::new(200, "page1", "text/html")),
            Ok(ScrapeResponse::new(200, "page2", "text/html")),
            Ok(ScrapeResponse::new(200, "page3", "text/html")),
        ]));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine.clone(), repo);

        let dtos = vec![
            make_dto("https://a.example.com"),
            make_dto("https://b.example.com"),
            make_dto("https://c.example.com"),
        ];
        let responses = service
            .scrape_batch(dtos)
            .await
            .expect("batch should succeed");

        assert_eq!(responses.len(), 3);
        // Engine should have been called 3 times
        assert_eq!(engine.scrape_calls.load(Ordering::SeqCst), 3);
        // Verify each response content
        assert!(responses.iter().any(|r| r.content == "page1"));
        assert!(responses.iter().any(|r| r.content == "page2"));
        assert!(responses.iter().any(|r| r.content == "page3"));
    }

    #[tokio::test]
    async fn test_scrape_batch_one_failure_returns_error() {
        let engine = Arc::new(MockEngineClient::new(vec![
            Ok(ScrapeResponse::new(200, "ok", "text/html")),
            Err(EngineError::NoEnginesAvailable),
        ]));
        let repo = Arc::new(MockTaskRepository::new());
        let service = make_service(engine, repo);

        let dtos = vec![
            make_dto("https://a.example.com"),
            make_dto("https://b.example.com"),
        ];
        let result = service.scrape_batch(dtos).await;
        let err = result.expect_err("batch with one failure should error");
        assert!(
            err.to_string().contains("No engines available"),
            "got: {}",
            err
        );
    }

    // ============ cancel_scrape() tests ============

    #[tokio::test]
    async fn test_cancel_scrape_success() {
        let repo = Arc::new(MockTaskRepository::new());
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo.clone());

        let task_id = Uuid::new_v4();
        service
            .cancel_scrape(task_id)
            .await
            .expect("cancel should succeed");
        assert_eq!(repo.cancel_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cancel_scrape_repository_failure_propagates() {
        let repo = Arc::new(MockTaskRepository::new().set_cancel_fails(true));
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let result = service.cancel_scrape(Uuid::new_v4()).await;
        let err = result.expect_err("repo failure should propagate");
        assert!(err.to_string().contains("Database error"), "got: {}", err);
    }

    // ============ get_scrape_status() tests ============

    #[tokio::test]
    async fn test_get_scrape_status_returns_pending_for_queued_task() {
        let task_id = Uuid::new_v4();
        let repo =
            Arc::new(MockTaskRepository::new().with_task(make_task(task_id, TaskStatus::Queued)));
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let status = service
            .get_scrape_status(task_id)
            .await
            .expect("status query should succeed");
        assert_eq!(status, ScrapeStatus::Pending);
    }

    #[tokio::test]
    async fn test_get_scrape_status_returns_running_for_active_task() {
        let task_id = Uuid::new_v4();
        let repo =
            Arc::new(MockTaskRepository::new().with_task(make_task(task_id, TaskStatus::Active)));
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let status = service.get_scrape_status(task_id).await.unwrap();
        assert_eq!(status, ScrapeStatus::Running);
    }

    #[tokio::test]
    async fn test_get_scrape_status_returns_completed() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(
            MockTaskRepository::new().with_task(make_task(task_id, TaskStatus::Completed)),
        );
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let status = service.get_scrape_status(task_id).await.unwrap();
        assert_eq!(status, ScrapeStatus::Completed);
    }

    #[tokio::test]
    async fn test_get_scrape_status_returns_failed() {
        let task_id = Uuid::new_v4();
        let repo =
            Arc::new(MockTaskRepository::new().with_task(make_task(task_id, TaskStatus::Failed)));
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let status = service.get_scrape_status(task_id).await.unwrap();
        assert_eq!(status, ScrapeStatus::Failed);
    }

    #[tokio::test]
    async fn test_get_scrape_status_returns_cancelled() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(
            MockTaskRepository::new().with_task(make_task(task_id, TaskStatus::Cancelled)),
        );
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let status = service.get_scrape_status(task_id).await.unwrap();
        assert_eq!(status, ScrapeStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_get_scrape_status_returns_error_when_task_not_found() {
        let repo = Arc::new(MockTaskRepository::new());
        let engine = Arc::new(MockEngineClient::always_ok(""));
        let service = make_service(engine, repo);

        let result = service.get_scrape_status(Uuid::new_v4()).await;
        let err = result.expect_err("missing task should error");
        assert!(err.to_string().contains("not found"), "got: {}", err);
    }

    // ============ parse_actions (free fn) tests ============

    #[test]
    fn test_parse_actions_none_returns_empty() {
        let actions = parse_actions(None);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_parse_actions_filters_screenshot() {
        let actions = parse_actions(Some(vec![
            ScrapeActionDto::Screenshot {
                full_page: Some(true),
            },
            ScrapeActionDto::Click {
                selector: "#btn".to_string(),
            },
        ]));
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            PageAction::Click { selector } if selector == "#btn"
        ));
    }

    #[test]
    fn test_parse_actions_invalid_scroll_defaults_to_down() {
        let actions = parse_actions(Some(vec![ScrapeActionDto::Scroll {
            direction: "sideways".to_string(),
        }]));
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            PageAction::Scroll {
                direction: ScrollDirection::Down
            }
        ));
    }

    // ============ parse_headers (free fn) tests ============

    #[test]
    fn test_parse_headers_none_returns_empty_map() {
        let headers = parse_headers(None).expect("None should succeed");
        assert!(headers.is_empty());
    }

    #[test]
    fn test_parse_headers_object_with_string_values() {
        let headers = parse_headers(Some(serde_json::json!({
            "Authorization": "Bearer token",
            "Accept": "application/json"
        })))
        .expect("object should succeed");
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers.get("Authorization").map(|v| v.as_str()),
            Some("Bearer token")
        );
    }

    #[test]
    fn test_parse_headers_non_string_value_returns_error() {
        let result = parse_headers(Some(serde_json::json!({"X-Count": 42})));
        let err = result.expect_err("non-string value should error");
        assert!(
            err.to_string()
                .contains("Invalid header value for key: X-Count"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_parse_headers_non_object_returns_error() {
        let result = parse_headers(Some(serde_json::json!("string-not-object")));
        let err = result.expect_err("non-object should error");
        assert!(
            err.to_string().contains("Headers must be a map"),
            "got: {}",
            err
        );
    }

    // ============ map_repo_error tests ============

    #[test]
    fn test_map_repo_error_database() {
        let err = map_repo_error(RepositoryError::Database(anyhow::anyhow!("conn lost")));
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("conn lost"));
    }

    #[test]
    fn test_map_repo_error_not_found() {
        let err = map_repo_error(RepositoryError::NotFound);
        assert!(err.to_string().contains("Record not found"));
    }
}
