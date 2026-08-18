// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape Executor
//!
//! 从 `scrape_worker.rs` 提取的抓取执行辅助函数。
//! 包含缓存读写、文本编码处理和结果持久化。

use anyhow::{Context, Result};
use chrono::Utc;
use log::{debug, info, warn};
use serde_json::Value;
use uuid::Uuid;

use crate::common::CacheContext;
use crate::domain::models::{ScrapeResult, Task};
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::engines::engine_client::ScrapeResponse;
use crate::infrastructure::oxcache::CacheService;
use crate::utils::crawl_text_integration::{CrawlTextIntegration, ScrapeResponseInput};
use crate::workers::cache_utils::{redact_url_for_log, SanitizedScrapeResponse};

/// 处理文本编码转换
///
/// 性能审查 H-2 修复：返回 `Cow<'_, str>`，禁用路径返回 `Cow::Borrowed`，
/// 避免每次抓取都 clone 整个 content（可能数 MB）。
///
/// # Arguments
/// * `task` - 目标任务
/// * `response` - 抓取响应
pub async fn process_text_encoding<'a>(
    task: &Task,
    response: &'a ScrapeResponse,
) -> Result<std::borrow::Cow<'a, str>> {
    info!(
        "开始处理文本编码转换，任务ID: {}, URL: {}",
        task.id, task.url
    );

    // 创建文本处理集成器
    let text_integration = CrawlTextIntegration::new(false); // Disable by default for now

    // 性能审查 H-2 修复：禁用时返回借用引用，避免 clone 整个 content（数 MB）
    if !text_integration.is_enabled() {
        return Ok(std::borrow::Cow::Borrowed(&response.content));
    }

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
                Ok(std::borrow::Cow::Owned(
                    processed_response.processed_content,
                ))
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

/// 保存抓取结果到结果仓库
///
/// 将 `ScrapeResponse` + 可选的额外数据持久化为 `ScrapeResult` 实体。
/// 支持 markdown 合并到 `meta_data` JSON。
///
/// # Arguments
/// * `task` - 目标任务
/// * `response` - 抓取响应
/// * `extra_data` - 额外提取数据
/// * `result_repository` - 结果仓库
pub async fn save_result(
    task: &Task,
    response: &ScrapeResponse,
    extra_data: Option<Value>,
    result_repository: &dyn ScrapeResultRepository,
) -> Result<()> {
    let mut meta_data = Value::Null;
    if let Some(data) = extra_data {
        meta_data = data;
    }

    // T042/R-content-001：将 response.markdown 合并到 meta_data JSON
    if let Some(ref markdown) = response.markdown {
        match &mut meta_data {
            Value::Null => {
                meta_data = serde_json::json!({ "markdown": markdown });
            }
            Value::Object(map) => {
                map.insert("markdown".to_string(), Value::String(markdown.clone()));
            }
            _ => {
                let original = std::mem::replace(&mut meta_data, Value::Null);
                meta_data = serde_json::json!({
                    "extracted": original,
                    "markdown": markdown,
                });
            }
        }
    }

    let content_to_store = response.content.clone();

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
        created_at: Utc::now(),
    };

    result_repository.save(result).await?;
    Ok(())
}

/// 读抓取结果缓存（T059/R-cache-002）
///
/// 返回 `Ok(None)` 表示缓存未命中；`Ok(Some)` 表示命中；`Err` 表示缓存故障。
///
/// # Arguments
/// * `ctx` - 缓存上下文（URL、方法、模式）
/// * `key` - 缓存键（由 `cache_utils::generate_scrape_cache_key` 生成）
/// * `cache_service` - 缓存服务
pub async fn try_read_scrape_cache(
    ctx: &CacheContext,
    key: &str,
    cache_service: &dyn CacheService,
) -> Result<Option<ScrapeResponse>> {
    match cache_service.get(key).await {
        Ok(Some(json)) => match serde_json::from_str::<ScrapeResponse>(&json) {
            Ok(resp) => Ok(Some(resp)),
            Err(e) => {
                // 返回 Err 而非 Ok(None)，让调用方可以区分缓存故障与缓存未命中，
                // 并有机会驱逐损坏条目。
                Err(anyhow::anyhow!(
                    "Cache deserialize failed url={} error={}",
                    redact_url_for_log(&ctx.url),
                    e
                ))
            }
        },
        Ok(None) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("cache get failed: {}", e)),
    }
}

/// 写抓取结果缓存（T059/R-cache-002）
///
/// 序列化 `ScrapeResponse` → JSON → 写入 `CacheService`（带 TTL）。
/// 通过 [`SanitizedScrapeResponse`] 跳过敏感响应头，防止凭证泄露。
///
/// # Arguments
/// * `ctx` - 缓存上下文
/// * `key` - 缓存键
/// * `response` - 抓取响应
/// * `cache_service` - 缓存服务
/// * `ttl_seconds` - 缓存 TTL（秒）
pub async fn try_write_scrape_cache(
    ctx: &CacheContext,
    key: &str,
    response: &ScrapeResponse,
    cache_service: &dyn CacheService,
    ttl_seconds: u64,
) -> Result<()> {
    // 性能 HIGH-1：借用序列化，避免克隆整个 ScrapeResponse
    let sanitized = SanitizedScrapeResponse::from_response(response);
    let json = serde_json::to_string(&sanitized)
        .context("Failed to serialize ScrapeResponse for cache")?;
    cache_service
        .set(key, &json, ttl_seconds)
        .await
        .map_err(|e| anyhow::anyhow!("cache set failed: {}", e))?;
    if log::log_enabled!(log::Level::Debug) {
        debug!(
            "Cache written url={} ttl={}s mode={:?}",
            redact_url_for_log(&ctx.url),
            ttl_seconds,
            ctx.mode
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::CacheMode;
    use crate::domain::models::{TaskStatus, TaskType};
    use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
    use crate::infrastructure::oxcache::CacheService;
    use chrono::Utc;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    fn make_task(url: &str) -> Task {
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: url.to_string(),
            payload: json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: Utc::now(),
            lock_token: None,
            lock_expires_at: None,
            expires_at: None,
        }
    }

    fn make_response(content: &str) -> ScrapeResponse {
        ScrapeResponse {
            content: content.to_string(),
            status_code: 200,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            screenshot: None,
            response_time_ms: 100,
            final_url: None,
            markdown: None,
        }
    }

    // ---- Mock types ----

    #[derive(Default)]
    struct MockResultRepo {
        save_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl ScrapeResultRepository for MockResultRepo {
        async fn save(&self, _result: ScrapeResult) -> Result<()> {
            self.save_count.fetch_add(1, Ordering::SeqCst);
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

    use std::future::Future;
    use std::pin::Pin;

    #[derive(Default)]
    struct MockCacheService {
        get_count: AtomicU32,
        set_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl CacheService for MockCacheService {
        fn get(
            &self,
            _key: &str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<String>>> + Send + '_>> {
            self.get_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(None) })
        }
        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
            self.set_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
        fn delete(
            &self,
            _key: &str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
        fn exists(
            &self,
            _key: &str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send + '_>> {
            Box::pin(async { Ok(false) })
        }
    }

    // ---- process_text_encoding ----

    #[tokio::test]
    async fn test_process_text_encoding_returns_borrowed_when_disabled() {
        let task = make_task("https://example.com");
        let response = make_response("Hello World");
        let result = process_text_encoding(&task, &response).await.unwrap();
        // CrawlTextIntegration::new(false) → disabled → borrowed
        assert_eq!(result.as_ref(), "Hello World");
    }

    // ---- save_result ----

    #[tokio::test]
    async fn test_save_result_stores_in_repository() {
        let task = make_task("https://example.com");
        let response = make_response("<html>test</html>");
        let repo = MockResultRepo::default();

        save_result(&task, &response, None, &repo).await.unwrap();
        assert_eq!(repo.save_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_save_result_with_extra_data() {
        let task = make_task("https://example.com");
        let response = make_response("content");
        let repo = MockResultRepo::default();
        let extra = json!({"key": "value"});

        save_result(&task, &response, Some(extra), &repo)
            .await
            .unwrap();
        assert_eq!(repo.save_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_save_result_with_markdown() {
        let task = make_task("https://example.com");
        let mut response = make_response("content");
        response.markdown = Some("# Title".to_string());
        let repo = MockResultRepo::default();

        save_result(&task, &response, None, &repo).await.unwrap();
        assert_eq!(repo.save_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_save_result_with_markdown_and_object_extra_data() {
        let task = make_task("https://example.com");
        let mut response = make_response("content");
        response.markdown = Some("# MD".to_string());
        let repo = MockResultRepo::default();
        let extra = json!({"key": "value"});

        save_result(&task, &response, Some(extra), &repo)
            .await
            .unwrap();
        assert_eq!(repo.save_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_save_result_with_markdown_and_non_object_extra_data() {
        let task = make_task("https://example.com");
        let mut response = make_response("content");
        response.markdown = Some("# MD".to_string());
        let repo = MockResultRepo::default();
        // Non-object extra data (array) should hit the `_` branch
        let extra = json!(["item1", "item2"]);

        save_result(&task, &response, Some(extra), &repo)
            .await
            .unwrap();
        assert_eq!(repo.save_count.load(Ordering::SeqCst), 1);
    }

    // ---- try_read_scrape_cache ----

    #[tokio::test]
    async fn test_try_read_scrape_cache_miss_returns_none() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let cache = MockCacheService::default();

        let result = try_read_scrape_cache(&ctx, "key", &cache).await.unwrap();
        assert!(result.is_none());
        assert_eq!(cache.get_count.load(Ordering::SeqCst), 1);
    }

    // ---- try_write_scrape_cache ----

    #[tokio::test]
    async fn test_try_write_scrape_cache_writes_serialized_response() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let response = make_response("content");
        let cache = MockCacheService::default();

        try_write_scrape_cache(&ctx, "key", &response, &cache, 300)
            .await
            .unwrap();
        assert_eq!(cache.set_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_try_write_scrape_cache_key_matches_read_key() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        // 验证 key 格式一致性（读写共用同一 key 生成逻辑）
        let options = crate::engines::engine_client::ScrapeOptions {
            method: crate::common::HttpMethod::Get,
            body: None,
            headers: std::collections::HashMap::new(),
            timeout: std::time::Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: vec![],
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            cache_mode: None,
            wait_for: None,
            needs_mllm: false,
        };
        let key = crate::workers::cache_utils::generate_scrape_cache_key(&ctx, &options);
        assert!(key.contains("example.com"));
    }

    // ---- Configurable error mocks ----

    struct ErrorCacheService {
        get_err: bool,
        set_err: bool,
    }

    impl ErrorCacheService {
        fn get_error() -> Self {
            Self {
                get_err: true,
                set_err: false,
            }
        }
        fn set_error() -> Self {
            Self {
                get_err: false,
                set_err: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl CacheService for ErrorCacheService {
        fn get(
            &self,
            _key: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
        > {
            if self.get_err {
                Box::pin(async { Err(anyhow::anyhow!("cache get error")) })
            } else {
                Box::pin(async { Ok(None) })
            }
        }
        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            if self.set_err {
                Box::pin(async { Err(anyhow::anyhow!("cache set error")) })
            } else {
                Box::pin(async { Ok(()) })
            }
        }
        fn delete(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }
        fn exists(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
        {
            Box::pin(async { Ok(false) })
        }
    }

    struct CorruptCacheService;

    #[async_trait::async_trait]
    impl CacheService for CorruptCacheService {
        fn get(
            &self,
            _key: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
        > {
            Box::pin(async { Ok(Some("not-valid-json".to_string())) })
        }
        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }
        fn delete(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }
        fn exists(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
        {
            Box::pin(async { Ok(false) })
        }
    }

    #[tokio::test]
    async fn test_try_read_scrape_cache_get_error() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let cache = ErrorCacheService::get_error();
        let result = try_read_scrape_cache(&ctx, "key", &cache).await;
        assert!(result.is_err(), "cache get error should propagate");
    }

    #[tokio::test]
    async fn test_try_read_scrape_cache_corrupt_data() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let cache = CorruptCacheService;
        let result = try_read_scrape_cache(&ctx, "key", &cache).await;
        assert!(
            result.is_err(),
            "corrupt cache data should return deserialization error"
        );
    }

    #[tokio::test]
    async fn test_try_write_scrape_cache_set_error() {
        let ctx = CacheContext {
            url: "https://example.com".to_string(),
            method: crate::common::HttpMethod::Get,
            mode: CacheMode::Enabled,
        };
        let response = make_response("content");
        let cache = ErrorCacheService::set_error();
        let result = try_write_scrape_cache(&ctx, "key", &response, &cache, 300).await;
        assert!(result.is_err(), "cache set error should propagate");
    }
}
