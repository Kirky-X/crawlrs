// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape Response Builder
//!
//! 从 `scrape_worker.rs` 提取的 payload 解析与请求构建函数。
//! 均为无状态纯函数，不依赖 `ScrapeWorker` 内部字段。

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::extract_request::ExtractRequestDto;
use crate::common::HttpMethod;
use crate::domain::models::Task;
use crate::engines::engine_client::{ScrapeOptions, ScrapeRequest};

/// 解析 Crawl 任务的 payload，提取 crawl_id、depth 和配置
///
/// # Arguments
/// * `task` - 待解析的任务（payload 须含 `crawl_id`、`depth`、`config`）
///
/// # Errors
/// * payload 缺少 `crawl_id` 或 JSON 解析失败时返回错误
pub fn parse_crawl_payload(task: &Task) -> Result<(Uuid, u32, CrawlConfigDto)> {
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

/// 解析 Extract 任务的 payload，提取请求 DTO 和首个 URL
///
/// # Arguments
/// * `task` - 待解析的任务（payload 须为 `ExtractRequestDto` 格式）
///
/// # Errors
/// * payload 不是合法 `ExtractRequestDto` 或无 URL 时返回错误
pub fn parse_extract_payload(task: &Task) -> Result<(ExtractRequestDto, String)> {
    let payload: ExtractRequestDto = serde_json::from_value(task.payload.clone())
        .context("Failed to parse extract task input")?;

    let url = payload.urls.first().context("No URL provided")?.clone();

    Ok((payload, url))
}

/// 构建 Crawl 任务的 `ScrapeRequest`
///
/// # Arguments
/// * `task` - 目标任务（提供 URL）
/// * `config` - 爬取配置（headers、proxy 等）
/// * `timeout_seconds` - 请求超时秒数（来自 settings）
pub fn build_crawl_request(
    task: &Task,
    config: &CrawlConfigDto,
    timeout_seconds: u64,
) -> ScrapeRequest {
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
        timeout: Duration::from_secs(timeout_seconds),
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
        block_ads: false,
        block_media: false,
        session_id: None,
        cache_mode: None,
        wait_for: None,
    })
}

/// 构建 Extract 任务的 `ScrapeRequest`
///
/// # Arguments
/// * `url` - 目标 URL
/// * `timeout_seconds` - 请求超时秒数（来自 settings）
pub fn build_extract_request(url: &str, timeout_seconds: u64) -> ScrapeRequest {
    ScrapeRequest::new(url.to_string()).with_options(ScrapeOptions {
        method: HttpMethod::Get,
        body: None,
        headers: HashMap::new(),
        timeout: Duration::from_secs(timeout_seconds),
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
        block_ads: false,
        block_media: false,
        session_id: None,
        cache_mode: None,
        wait_for: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{TaskStatus, TaskType};
    use chrono::Utc;

    fn make_task(payload: serde_json::Value) -> Task {
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload,
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

    // ---- parse_crawl_payload ----

    #[test]
    fn test_parse_crawl_payload_valid() {
        let crawl_id = Uuid::new_v4();
        let payload = json!({
            "crawl_id": crawl_id.to_string(),
            "depth": 2,
            "config": { "max_depth": 5 }
        });
        let task = make_task(payload);
        let (id, depth, config) = parse_crawl_payload(&task).unwrap();
        assert_eq!(id, crawl_id);
        assert_eq!(depth, 2);
        assert_eq!(config.max_depth, 5);
    }

    #[test]
    fn test_parse_crawl_payload_missing_crawl_id() {
        let payload = json!({ "depth": 0 });
        let task = make_task(payload);
        assert!(parse_crawl_payload(&task).is_err());
    }

    #[test]
    fn test_parse_crawl_payload_defaults() {
        let crawl_id = Uuid::new_v4();
        let payload = json!({
            "crawl_id": crawl_id.to_string(),
        });
        let task = make_task(payload);
        let (id, depth, _config) = parse_crawl_payload(&task).unwrap();
        assert_eq!(id, crawl_id);
        assert_eq!(depth, 0);
    }

    // ---- parse_extract_payload ----

    #[test]
    fn test_parse_extract_payload_valid() {
        let payload = json!({
            "urls": ["https://example.com"],
            "rules": null,
            "prompt": null,
            "schema": null
        });
        let task = make_task(payload);
        let (dto, url) = parse_extract_payload(&task).unwrap();
        assert_eq!(url, "https://example.com");
        assert!(dto.rules.is_none());
    }

    #[test]
    fn test_parse_extract_payload_no_urls() {
        let payload = json!({ "urls": [] });
        let task = make_task(payload);
        assert!(parse_extract_payload(&task).is_err());
    }

    // ---- build_crawl_request ----

    #[test]
    fn test_build_crawl_request_basic() {
        let task = make_task(json!({}));
        let config = CrawlConfigDto {
            max_depth: 3,
            max_concurrency: None,
            include_patterns: None,
            exclude_patterns: None,
            headers: None,
            proxy: None,
            extraction_rules: None,
            strategy: None,
            crawl_delay_ms: None,
        };
        let req = build_crawl_request(&task, &config, 30);
        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.options.timeout, Duration::from_secs(30));
        assert!(!req.options.needs_js);
    }

    #[test]
    fn test_build_crawl_request_with_headers() {
        let task = make_task(json!({}));
        let config = CrawlConfigDto {
            max_depth: 3,
            max_concurrency: None,
            include_patterns: None,
            exclude_patterns: None,
            headers: Some(json!({"X-Custom": "value"})),
            proxy: None,
            extraction_rules: None,
            strategy: None,
            crawl_delay_ms: None,
        };
        let req = build_crawl_request(&task, &config, 30);
        assert_eq!(
            req.options.headers.get("X-Custom").map(|s| s.as_str()),
            Some("value")
        );
    }

    // ---- build_extract_request ----

    #[test]
    fn test_build_extract_request_basic() {
        let req = build_extract_request("https://example.com/page", 60);
        assert_eq!(req.url, "https://example.com/page");
        assert_eq!(req.options.timeout, Duration::from_secs(60));
        assert!(req.options.skip_tls_verification);
        assert!(req.options.proxy.is_none());
    }
}
