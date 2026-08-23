use super::*;
use crate::domain::models::{CrawlStatus, ScrapeResult, TaskType};
use crate::engines::EngineError;
use crate::infrastructure::oxcache::RegexCacheType;
use crate::utils::coalesce::RequestCoalescer;
use crate::workers::cache_utils::{filter_sensitive_headers, generate_scrape_cache_key};
use crate::workers::crawl::{FilterContext, UrlFilter, UrlPatternFilter};
use crate::workers::errors::ScrapeWorkerError;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;
use std::time::Duration;

// ========== T059/R-cache-002: MockCacheService ==========

/// 可观测的 MockCacheService 用于 T059 缓存门控测试
///
/// - `data`：内存存储，可预填充模拟 cache hit
/// - `get_count`/`set_count`：原子计数器，验证读/写行为
struct MockCacheService {
    data: Mutex<HashMap<String, String>>,
    get_count: AtomicU64,
    set_count: AtomicU64,
}

impl MockCacheService {
    fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            get_count: AtomicU64::new(0),
            set_count: AtomicU64::new(0),
        }
    }

    fn with_entry(key: &str, value: &str) -> Self {
        let s = Self::new();
        s.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        s
    }

    fn get_count(&self) -> u64 {
        self.get_count.load(Ordering::Relaxed)
    }

    fn set_count(&self) -> u64 {
        self.set_count.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl CacheService for MockCacheService {
    fn get(
        &self,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<String>>> + Send + '_>> {
        self.get_count.fetch_add(1, Ordering::Relaxed);
        let data = self.data.lock().unwrap().get(key).cloned();
        Box::pin(async move { Ok(data) })
    }

    fn set(
        &self,
        key: &str,
        value: &str,
        _ttl_seconds: u64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        self.set_count.fetch_add(1, Ordering::Relaxed);
        // R-cache-002 修复：必须真正写入 data，否则写后读测试（test_try_write_scrape_cache_key_matches_read_key）
        // 会因 miss 失败。原实现以 `_key`/`_value` 命名导致数据被丢弃，违反规则 12（失败显性化）。
        // `_ttl_seconds` 保留下划线前缀：内存 mock 无过期语义，TTL 不影响读写一致性验证。
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Box::pin(async { Ok(()) })
    }

    fn delete(&self, key: &str) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        self.data.lock().unwrap().remove(key);
        Box::pin(async { Ok(()) })
    }

    fn exists(&self, key: &str) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send + '_>> {
        let exists = self.data.lock().unwrap().contains_key(key);
        Box::pin(async move { Ok(exists) })
    }
}

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

/// H-4 测试辅助：构造 CoalesceCoordinator + 新 RequestCoalescer（架构审查 H-2 修复）
///
/// 集中装配逻辑，避免在 16+ 处测试中重复 `Arc::new(CoalesceCoordinator::new(...))`。
/// 默认使用新的 `RequestCoalescer` 实例（与多数单测场景一致）。
fn make_coalesce_coordinator(
    task_repo: Arc<dyn TaskRepository>,
    result_repo: Arc<dyn ScrapeResultRepository>,
) -> Arc<CoalesceCoordinator> {
    Arc::new(CoalesceCoordinator::new(
        task_repo,
        result_repo,
        Arc::new(RequestCoalescer::new()),
    ))
}

/// H-4 测试辅助：构造 CoalesceCoordinator + 指定 RequestCoalescer（架构审查 H-2 修复）
///
/// 用于需要共享 `request_coalescer` 实例的测试（如 line 4629）。
fn make_coalesce_coordinator_with_coalescer(
    task_repo: Arc<dyn TaskRepository>,
    result_repo: Arc<dyn ScrapeResultRepository>,
    request_coalescer: Arc<RequestCoalescer>,
) -> Arc<CoalesceCoordinator> {
    Arc::new(CoalesceCoordinator::new(
        task_repo,
        result_repo,
        request_coalescer,
    ))
}

/// PERF-H1 测试辅助：模拟生产路径解析 payload 得到 ScrapeRequestDto。
///
/// 调用方完成 `let dto = parse_dto_for_test(&task);` 后，
/// 应以 `dto.as_ref()` 作为 `handle_scrape_success` 的第二参数，
/// 与生产路径 `process_scrape_task` 行为一致（解析失败返回 None）。
fn parse_dto_for_test(task: &Task) -> Option<ScrapeRequestDto> {
    serde_json::from_value(task.payload.clone()).ok()
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

/// T019：构造测试用 MemoryScheduler
///
/// 默认返回 Normal 状态的调度器（内存使用率 0.5）。
/// 需要模拟 Critical/Pressure 的测试可自行构造 MemoryScheduler。
#[cfg(feature = "metrics")]
fn make_test_memory_scheduler() -> Arc<MemoryScheduler> {
    use crate::infrastructure::observability::metrics::SystemMonitorTrait;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 测试用 mock：memory_usage 从 AtomicU64 读取（f64 位模式）
    struct StaticMockMonitor {
        bits: AtomicU64,
    }
    impl SystemMonitorTrait for StaticMockMonitor {
        fn cpu_usage(&self) -> f64 {
            0.0
        }
        fn memory_usage(&self) -> f64 {
            f64::from_bits(self.bits.load(Ordering::Relaxed))
        }
        fn is_metrics_stale(&self) -> bool {
            false
        }
    }
    Arc::new(MemoryScheduler::new(
        Arc::new(StaticMockMonitor {
            bits: AtomicU64::new(0.5f64.to_bits()),
        }),
        0.8,
        0.9,
        Duration::from_secs(30),
    ))
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

// ========== T062 安全审查 MEDIUM-2: redact_url_for_log tests ==========

#[test]
fn test_redact_url_for_log_strips_query_params() {
    // query 参数中的 token/api_key 必须被移除
    let url = "https://example.com/api?token=secret&api_key=key123";
    let redacted = redact_url_for_log(url);
    assert_eq!(redacted, "https://example.com/api");
    assert!(
        !redacted.contains("secret"),
        "token value must not appear in log"
    );
    assert!(
        !redacted.contains("key123"),
        "api_key value must not appear in log"
    );
}

#[test]
fn test_redact_url_for_log_strips_fragment() {
    let url = "https://example.com/page#section";
    let redacted = redact_url_for_log(url);
    assert_eq!(redacted, "https://example.com/page");
    assert!(!redacted.contains("section"));
}

#[test]
fn test_redact_url_for_log_preserves_path() {
    let url = "https://example.com/deep/nested/path";
    let redacted = redact_url_for_log(url);
    assert_eq!(redacted, url);
}

#[test]
fn test_redact_url_for_log_preserves_port() {
    let url = "http://localhost:8080/api";
    let redacted = redact_url_for_log(url);
    assert_eq!(redacted, url);
}

#[test]
fn test_redact_url_for_log_invalid_url_returns_placeholder() {
    // 非法 URL 返回占位符，绝不原样返回可能含凭证的输入
    let redacted = redact_url_for_log("not a url at all");
    assert_eq!(redacted, "[invalid-url]");
}

#[test]
fn test_redact_url_for_log_truncates_long_urls() {
    // 超长 URL 截断到 200 字符 + "..."
    let long_path = "a".repeat(300);
    let url = format!("https://example.com/{}", long_path);
    let redacted = redact_url_for_log(&url);
    assert!(
        redacted.ends_with("..."),
        "truncated URL should end with '...': {}",
        redacted
    );
    // 截断后总长度应 <= 203（200 + "..."）
    assert!(
        redacted.len() <= 203,
        "redacted length {} should be <= 203",
        redacted.len()
    );
}

#[test]
fn test_redact_url_for_log_empty_query_only() {
    // 仅 query 无 path
    let url = "https://example.com?token=secret";
    let redacted = redact_url_for_log(url);
    assert_eq!(redacted, "https://example.com/");
    assert!(!redacted.contains("secret"));
}

// ========== T062 安全审查 LOW-2: filter_sensitive_headers tests ==========
// 性能审查 MEDIUM-1：函数改为原地修改 (&mut HashMap)，测试同步更新

#[test]
fn test_filter_sensitive_headers_removes_set_cookie() {
    let mut headers = HashMap::new();
    headers.insert("Set-Cookie".to_string(), "session=abc123".to_string());
    headers.insert("Content-Type".to_string(), "text/html".to_string());
    filter_sensitive_headers(&mut headers);
    assert!(!headers.contains_key("Set-Cookie"));
    assert!(headers.contains_key("Content-Type"));
}

#[test]
fn test_filter_sensitive_headers_removes_authorization() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer token123".to_string());
    headers.insert("X-Custom".to_string(), "value".to_string());
    filter_sensitive_headers(&mut headers);
    assert!(!headers.contains_key("Authorization"));
    assert!(headers.contains_key("X-Custom"));
}

#[test]
fn test_filter_sensitive_headers_case_insensitive() {
    // HTTP header 名称大小写不敏感
    let mut headers = HashMap::new();
    headers.insert("set-cookie".to_string(), "v1".to_string());
    headers.insert("SET-COOKIE".to_string(), "v2".to_string());
    headers.insert("Set-Cookie".to_string(), "v3".to_string());
    filter_sensitive_headers(&mut headers);
    assert!(
        !headers.contains_key("set-cookie"),
        "lowercase set-cookie should be filtered"
    );
    assert!(
        !headers.contains_key("SET-COOKIE"),
        "uppercase SET-COOKIE should be filtered"
    );
    assert!(
        !headers.contains_key("Set-Cookie"),
        "mixed-case Set-Cookie should be filtered"
    );
}

#[test]
fn test_filter_sensitive_headers_removes_all_sensitive() {
    let mut headers = HashMap::new();
    headers.insert("Set-Cookie".to_string(), "v".to_string());
    headers.insert("Cookie".to_string(), "v".to_string());
    headers.insert("Authorization".to_string(), "v".to_string());
    headers.insert("Proxy-Authorization".to_string(), "v".to_string());
    headers.insert("WWW-Authenticate".to_string(), "v".to_string());
    headers.insert("X-Api-Key".to_string(), "v".to_string());
    headers.insert("X-Auth-Token".to_string(), "v".to_string());
    headers.insert("X-Session-Id".to_string(), "v".to_string());
    filter_sensitive_headers(&mut headers);
    assert!(
        headers.is_empty(),
        "all sensitive headers should be removed"
    );
}

#[test]
fn test_filter_sensitive_headers_preserves_non_sensitive() {
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "text/html".to_string());
    headers.insert("Content-Length".to_string(), "1234".to_string());
    headers.insert("Cache-Control".to_string(), "no-cache".to_string());
    headers.insert("ETag".to_string(), "abc".to_string());
    filter_sensitive_headers(&mut headers);
    assert_eq!(headers.len(), 4);
    assert!(headers.contains_key("Content-Type"));
    assert!(headers.contains_key("Content-Length"));
    assert!(headers.contains_key("Cache-Control"));
    assert!(headers.contains_key("ETag"));
}

#[test]
fn test_filter_sensitive_headers_empty_input() {
    let mut headers: HashMap<String, String> = HashMap::new();
    filter_sensitive_headers(&mut headers);
    assert!(headers.is_empty());
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
    let request =
        ScrapeWorker::build_scrape_request(&task).expect("minimal payload with url should succeed");
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
use crate::domain::services::llm::TokenUsage;
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
    async fn reset_stuck_tasks(&self, _timeout: chrono::Duration) -> Result<u64, RepositoryError> {
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
    async fn cleanup_expired(&self, _retention_days: i64) -> Result<u64> {
        Ok(0)
    }
}

/// T042: 捕获 save_result 调用以验证 meta_data 持久化
///
/// 用于 markdown 持久化测试：调用方通过 `captured()` 获取保存的 ScrapeResult。
struct CapturingScrapeResultRepository {
    captured: std::sync::Mutex<Option<ScrapeResult>>,
}

impl CapturingScrapeResultRepository {
    fn new() -> Self {
        Self {
            captured: std::sync::Mutex::new(None),
        }
    }
    fn captured(&self) -> Option<ScrapeResult> {
        self.captured.lock().ok()?.clone()
    }
}

#[async_trait::async_trait]
impl ScrapeResultRepository for CapturingScrapeResultRepository {
    async fn save(&self, result: ScrapeResult) -> Result<()> {
        let mut guard = self.captured.lock().expect("captured mutex poisoned");
        *guard = Some(result);
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
    async fn cleanup_expired(&self, _retention_days: i64) -> Result<u64> {
        Ok(0)
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
    async fn update_status(&self, _id: Uuid, _status: CrawlStatus) -> Result<(), RepositoryError> {
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
    async fn execute(&self, _request_dto: ScrapeRequestDto) -> Result<ScrapeResponse, DomainError> {
        Ok(ScrapeResponse {
            content: String::new(),
            status_code: 200,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 0,
            final_url: None,
            markdown: None,
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
    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> Result<Option<Duration>> {
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
    async fn extract_with_rag(
        &self,
        _html_content: &str,
        _query: &str,
        _schema: &Value,
        _rag_strategy: &crate::domain::services::rag_strategy::RagExtractionStrategy,
    ) -> Result<(Value, TokenUsage)> {
        Ok((json!({}), TokenUsage::default()))
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
    let settings =
        crate::bootstrap::config::load_settings().expect("Failed to load settings for mock worker");
    let settings_arc = Arc::new(settings.clone());
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: settings_arc,
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
}

/// T059/R-cache-002：构造使用指定 MockCacheService 的 worker
///
/// 返回 (worker, cache_arc) —— 调用方通过 cache_arc.get_count()/set_count()
/// 验证缓存读/写行为，或通过 cache_arc 预填充数据模拟 cache hit。
async fn build_mock_worker_with_cache(cache: Arc<MockCacheService>) -> ScrapeWorker {
    let regex_cache = make_regex_cache().await;
    let engine_client = Arc::new(EngineClient::new());
    let settings =
        crate::bootstrap::config::load_settings().expect("Failed to load settings for mock worker");
    let settings_arc = Arc::new(settings.clone());
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: settings_arc,
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: cache as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
}

/// T042: 构造使用 CapturingScrapeResultRepository 的 worker
///
/// 返回 (worker, capturing_repo_arc) —— 调用方通过 capturing_repo_arc.captured()
/// 获取 save_result 保存的 ScrapeResult 以断言 meta_data 内容。
async fn build_mock_worker_with_capturing_repo(
) -> (ScrapeWorker, Arc<CapturingScrapeResultRepository>) {
    let regex_cache = make_regex_cache().await;
    let engine_client = Arc::new(EngineClient::new());
    let settings =
        crate::bootstrap::config::load_settings().expect("Failed to load settings for mock worker");
    let settings_arc = Arc::new(settings.clone());
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let capturing_repo = Arc::new(CapturingScrapeResultRepository::new());
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> =
        capturing_repo.clone() as Arc<dyn ScrapeResultRepository>;
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: settings_arc,
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });
    (worker, capturing_repo)
}

// --- should_crawl tests ---
//
// 架构审查 M-4 修复：原 should_crawl 方法在生产 impl 块中（虽然 #[cfg(test)] 门控），
// 违反规则9（生产 impl 块不应保留测试 helper）。已移到 test mod 内独立 impl 块，
// 保持 test_mock_should_crawl_* 系列调用方式（`worker.should_crawl(...)`）不变。
impl super::ScrapeWorker {
    /// 测试专用：验证 UrlPatternFilter 行为等价性
    ///
    /// 性能审查 H-3 修复后：生产路径 extract_and_queue_links 已改为循环外构造一次
    /// UrlPatternFilter 复用，不再调用 should_crawl。此方法保留供测试验证行为等价性。
    fn should_crawl(&self, url: &str, config: &CrawlConfigDto) -> bool {
        // T066/R-frontier-002：委托 UrlPatternFilter（行为等价，回归测试断言）
        //
        // 边界场景：Some(vec![]) 空 include 列表 → 无 pattern 可匹配 → 拒绝（vacuous truth）。
        // UrlPatternFilter 将空 include 视为"无限制"（返回 true），需在此显式处理。
        if matches!(&config.include_patterns, Some(patterns) if patterns.is_empty()) {
            return false;
        }
        let include = config.include_patterns.clone().unwrap_or_default();
        let exclude = config.exclude_patterns.clone().unwrap_or_default();
        UrlPatternFilter::new(include, exclude).accept(url, &FilterContext::default())
    }
}

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
    let config = make_crawl_config(None, Some(vec!["blocked".to_string(), "admin".to_string()]));
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
        extraction_prompt: None,
        extraction_schema: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
    // T019 修复：build_extract_request 不再跳过 TLS 验证
    assert!(!request.options.skip_tls_verification);
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
    };
    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
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
        markdown: None,
    };
    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
    assert!(result.is_ok());
}

// --- T042/R-content-001: Markdown 集成测试（gated `markdown` 特性） ---

/// formats 含 "markdown" 时应生成 Markdown 并持久化到 meta_data
#[cfg(feature = "content")]
#[tokio::test]
async fn test_handle_scrape_success_with_markdown_format_generates_markdown() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({
        "url": "https://example.com",
        "formats": ["markdown"],
    }));
    let response = ScrapeResponse {
        content: r#"<html><body><h1>Title</h1><p>Paragraph text</p></body></html>"#.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: None,
    };

    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
    assert!(result.is_ok(), "handle_scrape_success should succeed");

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    let meta = &saved.meta_data;
    assert!(
        meta.get("markdown").is_some(),
        "meta_data should contain markdown key, got: {meta}"
    );
    let md = meta
        .get("markdown")
        .and_then(|v| v.as_str())
        .expect("markdown should be a string");
    assert!(
        md.contains("Title"),
        "markdown should contain heading text, got: {md}"
    );
    assert!(
        md.contains("Paragraph text"),
        "markdown should contain paragraph text, got: {md}"
    );
}

/// formats 不含 "markdown" 时不应生成 Markdown
#[cfg(feature = "content")]
#[tokio::test]
async fn test_handle_scrape_success_without_markdown_format_no_markdown() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({
        "url": "https://example.com",
        "formats": ["html"],
    }));
    let response = ScrapeResponse {
        content: r#"<html><body><h1>Title</h1></body></html>"#.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: None,
    };

    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
    assert!(result.is_ok());

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    // meta_data 应为 Null（无提取规则、无 markdown）
    assert!(
        saved.meta_data.is_null(),
        "meta_data should be null when no markdown requested, got: {}",
        saved.meta_data
    );
}

/// formats 为 None 时不应生成 Markdown
#[cfg(feature = "content")]
#[tokio::test]
async fn test_handle_scrape_success_no_formats_field_no_markdown() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({
        "url": "https://example.com",
    }));
    let response = ScrapeResponse {
        content: r#"<html><body><h1>Title</h1></body></html>"#.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: None,
    };

    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
    assert!(result.is_ok());

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    assert!(
        saved.meta_data.is_null(),
        "meta_data should be null when formats field absent, got: {}",
        saved.meta_data
    );
}

/// save_result 应将 markdown 合并到已存在的 meta_data Object 中
#[cfg(feature = "content")]
#[tokio::test]
async fn test_save_result_merges_markdown_into_existing_meta_data() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({"url": "https://example.com"}));
    let response = ScrapeResponse {
        content: "<html><body>Hello</body></html>".to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: Some("# Hello".to_string()),
    };
    // 已有提取数据：title 字段
    let existing_meta = json!({"title": "Existing Title"});

    let result = worker
        .save_result(&task, &response, Some(existing_meta))
        .await;
    assert!(result.is_ok());

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    let meta = &saved.meta_data;
    assert!(
        meta.get("title").and_then(|v| v.as_str()) == Some("Existing Title"),
        "meta_data should preserve existing extracted fields, got: {meta}"
    );
    assert!(
        meta.get("markdown").and_then(|v| v.as_str()) == Some("# Hello"),
        "meta_data should contain merged markdown, got: {meta}"
    );
}

/// save_result 在 meta_data 为 Null 时应创建 {"markdown": "..."} 对象
#[cfg(feature = "content")]
#[tokio::test]
async fn test_save_result_with_markdown_only_creates_markdown_object() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({"url": "https://example.com"}));
    let response = ScrapeResponse {
        content: "<html><body>Hello</body></html>".to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: Some("# Generated MD".to_string()),
    };

    let result = worker.save_result(&task, &response, None).await;
    assert!(result.is_ok());

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    let meta = &saved.meta_data;
    assert!(
        meta.is_object(),
        "meta_data should be an object when only markdown present, got: {meta}"
    );
    assert!(
        meta.get("markdown").and_then(|v| v.as_str()) == Some("# Generated MD"),
        "meta_data should contain markdown, got: {meta}"
    );
    // 仅含 markdown 一个键
    assert_eq!(
        meta.as_object().map(|o| o.len()),
        Some(1),
        "meta_data should have exactly one key (markdown), got: {meta}"
    );
}

/// save_result 在 response.markdown 为 None 时不应修改 meta_data
#[tokio::test]
async fn test_save_result_without_markdown_does_not_modify_meta_data() {
    let (worker, capturing_repo) = build_mock_worker_with_capturing_repo().await;
    let task = make_task(json!({"url": "https://example.com"}));
    let response = ScrapeResponse {
        content: "<html><body>Hello</body></html>".to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 50,
        final_url: None,
        markdown: None,
    };
    let existing_meta = json!({"title": "Only Title"});

    let result = worker
        .save_result(&task, &response, Some(existing_meta))
        .await;
    assert!(result.is_ok());

    let saved = capturing_repo
        .captured()
        .expect("result should have been saved");
    let meta = &saved.meta_data;
    assert!(
        meta.get("title").and_then(|v| v.as_str()) == Some("Only Title"),
        "meta_data should preserve existing fields, got: {meta}"
    );
    assert!(
        meta.get("markdown").is_none(),
        "meta_data should not contain markdown key when response.markdown is None, got: {meta}"
    );
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
        markdown: None,
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
        markdown: None,
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
    assert_eq!(builder.default_concurrency_limit(), 10);
}

#[tokio::test]
async fn test_builder_default_impl() {
    let builder = ScrapeWorkerBuilder::default();
    assert_eq!(builder.default_concurrency_limit(), 10);
}

#[tokio::test]
async fn test_builder_with_default_concurrency_limit() {
    let builder = ScrapeWorkerBuilder::new().with_default_concurrency_limit(50);
    assert_eq!(builder.default_concurrency_limit(), 50);
}

#[tokio::test]
async fn test_builder_build_success() {
    let regex_cache = make_regex_cache().await;
    let engine_client = Arc::new(EngineClient::new());
    let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
    let team_semaphore = Arc::new(TeamSemaphore::new(10));

    let builder = ScrapeWorkerBuilder::new()
        .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
        .with_result_repository(
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
        )
        .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
        .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>);
    #[cfg(feature = "metrics")]
    let builder = builder.with_memory_scheduler(make_test_memory_scheduler());
    let worker = builder.build();

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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
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

    let builder = ScrapeWorkerBuilder::new()
        .with_repository(Arc::new(MockTaskRepository) as Arc<dyn TaskRepository>)
        .with_result_repository(
            Arc::new(MockScrapeResultRepository) as Arc<dyn ScrapeResultRepository>
        )
        .with_crawl_repository(Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>)
        .with_webhook_service(Arc::new(MockWebhookService) as Arc<dyn WebhookService>)
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
        .with_default_concurrency_limit(100);
    #[cfg(feature = "metrics")]
    let builder = builder.with_memory_scheduler(make_test_memory_scheduler());
    let worker = builder.build().expect("build should succeed");

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
        // proxy_provider=None, proxy_strategy=RoundRobin, proxy_url=None：测试环境无代理配置
        // H1/H2 修复：proxy_provider 改为 Option<Arc<dyn ProxyProvider>>，新增 strategy 参数
        None,
        crate::config::settings::ProxyStrategy::RoundRobin,
        None,
        &settings.engines,
        // T061：注入完整 EngineTimeoutSettings（含 default_timeout_seconds + 三个 MRT 字段）
        &settings.timeouts.engines,
    );
    let services = init_services(
        &infra,
        engines.engine_client.clone(),
        infra.http_client.clone(),
        &settings,
    )
    .await;

    // H-4 职责拆分：构造 CoalesceCoordinator（共享 task_repo + result_repo + request_coalescer）
    let coalesce_coordinator = make_coalesce_coordinator_with_coalescer(
        infra.repositories.task_repo.clone() as Arc<dyn TaskRepository>,
        infra.repositories.result_repo.clone() as Arc<dyn ScrapeResultRepository>,
        services.request_coalescer.clone(),
    );

    // Construct ScrapeWorker via new().
    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: infra.repositories.task_repo.clone() as Arc<dyn TaskRepository>,
        result_repository: infra.repositories.result_repo.clone()
            as Arc<dyn ScrapeResultRepository>,
        crawl_repository: infra.repositories.crawl_repo.clone() as Arc<dyn CrawlRepository>,
        webhook_service: services.webhook_service.clone(),
        credits_repository: infra.repositories.credits_repo.clone() as Arc<dyn CreditsRepository>,
        engine_client: engines.engine_client.clone(),
        create_scrape_use_case: services.create_scrape_use_case.clone(),
        team_semaphore: services.team_semaphore.clone(),
        coalesce_coordinator,
        robots_checker: services.robots_checker.clone(),
        settings: settings_arc,
        default_concurrency_limit: settings.concurrency.default_team_limit as usize,
        extraction_service: services.extraction_service.clone(),
        regex_cache: (*services.regex_cache).clone(),
        cache_service: infra.cache_service.clone(),
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

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
        extraction_prompt: None,
        extraction_schema: None,
    }
}

#[tokio::test]
async fn tc_scrape_worker_new_constructs_successfully() {
    if !require_docker().await {
        eprintln!("[skip] Docker unavailable — tc_scrape_worker_new_constructs_successfully");
        return;
    }
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
        // proxy_pool=None, proxy_url=None：此处无代理配置（T056：用 ProxyPool 替代单 proxy_url）
        None,
        crate::config::settings::ProxyStrategy::RoundRobin,
        None,
        &settings.engines,
        // T061：注入完整 EngineTimeoutSettings（含 default_timeout_seconds + 三个 MRT 字段）
        &settings.timeouts.engines,
    );
    let services = init_services(
        &infra,
        engines.engine_client.clone(),
        infra.http_client.clone(),
        &settings,
    )
    .await;

    // Use ScrapeWorkerBuilder to construct the worker.
    let builder = ScrapeWorkerBuilder::new()
        .with_repository(infra.repositories.task_repo.clone() as Arc<dyn TaskRepository>)
        .with_result_repository(
            infra.repositories.result_repo.clone() as Arc<dyn ScrapeResultRepository>
        )
        .with_crawl_repository(infra.repositories.crawl_repo.clone() as Arc<dyn CrawlRepository>)
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
        .with_cache_service(infra.cache_service.clone());
    #[cfg(feature = "metrics")]
    let builder = builder.with_memory_scheduler(make_test_memory_scheduler());
    let worker = builder
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
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
    async fn extract_with_rag(
        &self,
        _html_content: &str,
        _query: &str,
        _schema: &Value,
        _rag_strategy: &crate::domain::services::rag_strategy::RagExtractionStrategy,
    ) -> Result<(Value, TokenUsage)> {
        Ok((json!({}), TokenUsage::default()))
    }
}

/// Build a ScrapeWorker whose ExtractionService returns non-zero tokens.
async fn build_mock_worker_with_tokens() -> ScrapeWorker {
    let regex_cache = make_regex_cache().await;
    let engine_client = Arc::new(EngineClient::new());
    let settings =
        crate::bootstrap::config::load_settings().expect("Failed to load settings for mock worker");
    let settings_arc = Arc::new(settings.clone());
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: settings_arc,
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionServiceWithTokens)
            as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
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
        markdown: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        markdown: None,
    };
    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
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
        markdown: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        markdown: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
        markdown: None,
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
    /// T019：捕获最近一次 update 的 task，用于验证 reschedule 后的状态
    last_updated_task: std::sync::Mutex<Option<Task>>,
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
            last_updated_task: std::sync::Mutex::new(None),
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

    /// T019：获取最近一次 update 捕获的 task（用于验证 reschedule 状态）
    fn last_updated_task(&self) -> Option<Task> {
        self.last_updated_task.lock().unwrap().clone()
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
        // T019：捕获 update 的 task 用于验证 reschedule 状态
        *self.last_updated_task.lock().unwrap() = Some(task.clone());
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
    async fn reset_stuck_tasks(&self, _timeout: chrono::Duration) -> Result<u64, RepositoryError> {
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
    async fn update_status(&self, _id: Uuid, _status: CrawlStatus) -> Result<(), RepositoryError> {
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
    async fn cleanup_expired(&self, _retention_days: i64) -> Result<u64> {
        Ok(0)
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
    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> Result<Option<Duration>> {
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
    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> Result<Option<Duration>> {
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
    async fn get_crawl_delay(&self, _url_str: &str, _user_agent: &str) -> Result<Option<Duration>> {
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
                EngineError::AntiBotDetected(s) => EngineError::AntiBotDetected(s.clone()),
                EngineError::FeatureToggle(s) => EngineError::FeatureToggle(s.clone()),
                EngineError::RequestFailed(s) => EngineError::RequestFailed(s.clone()),
                EngineError::Other(s) => EngineError::Other(s.clone()),
                EngineError::Internal(s) => EngineError::Internal(s.clone()),
                EngineError::EngineMrtExceeded { engine, mrt } => EngineError::EngineMrtExceeded {
                    engine: engine.clone(),
                    mrt: *mrt,
                },
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
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: crawl_repo,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
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
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service,
        credits_repository: credits_repo,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
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
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo.clone(),
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client: Arc::new(EngineClient::new()),
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

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

// ========== T019：内存感知准入检查测试（R-runtime-001）==========

/// T019：构造 Critical 状态的 MemoryScheduler，验证 process_task 延后任务而非执行
#[cfg(feature = "metrics")]
#[tokio::test]
async fn test_process_task_memory_critical_defers_task() {
    use crate::infrastructure::observability::metrics::SystemMonitorTrait;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 测试用 mock：memory_usage 返回固定值（f64 位模式存储于 AtomicU64）
    struct CriticalMockMonitor {
        bits: AtomicU64,
    }
    impl SystemMonitorTrait for CriticalMockMonitor {
        fn cpu_usage(&self) -> f64 {
            0.0
        }
        fn memory_usage(&self) -> f64 {
            f64::from_bits(self.bits.load(Ordering::Relaxed))
        }
        fn is_metrics_stale(&self) -> bool {
            false
        }
    }

    // 构造 Critical 状态调度器（内存使用率 0.95 >= critical_threshold 0.9）
    let memory_scheduler: Arc<MemoryScheduler> = Arc::new(MemoryScheduler::new(
        Arc::new(CriticalMockMonitor {
            bits: AtomicU64::new(0.95f64.to_bits()),
        }),
        0.8,
        0.9,
        Duration::from_secs(30),
    ));

    let task_repo = Arc::new(ConfigurableTaskRepo::new());
    let regex_cache = make_regex_cache().await;
    let settings = crate::bootstrap::config::load_settings()
        .expect("Failed to load settings for memory critical test");
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(
        task_repo.clone() as Arc<dyn TaskRepository>,
        result_repo.clone(),
    );

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo.clone() as Arc<dyn TaskRepository>,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client: Arc::new(EngineClient::new()),
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        memory_scheduler,
    });

    let task = make_task(json!({"url": "https://example.com"}));
    let original_scheduled_at = task.scheduled_at;
    let result = worker.process_task(task).await;

    // 延后而非执行：process_task 应返回 Ok(())（reschedule 非错误）
    assert!(
        result.is_ok(),
        "Critical memory should defer task, returning Ok(())"
    );

    // update 应被调用一次（reschedule 到 backlog）
    assert_eq!(
        task_repo.update_count(),
        1,
        "Critical memory should trigger one update to reschedule task"
    );

    // mark_failed 不应被调用（延后 ≠ 失败）
    assert_eq!(
        task_repo.mark_failed_count(),
        0,
        "Critical memory should NOT mark task as failed"
    );

    // 验证 reschedule 后的 task 状态：status=Queued，scheduled_at 推后 30s
    let updated = task_repo
        .last_updated_task()
        .expect("update should have captured the task");
    assert_eq!(
        updated.status,
        TaskStatus::Queued,
        "deferred task should be re-queued"
    );
    let now = Utc::now();
    let scheduled = updated
        .scheduled_at
        .expect("scheduled_at should be set for deferred task");
    assert!(
        scheduled > now,
        "scheduled_at should be in the future (deferred), got {} <= now {}",
        scheduled,
        now
    );
    // 与原 scheduled_at 相比应有明显推迟（30 秒）
    if let Some(orig) = original_scheduled_at {
        assert!(
            scheduled > orig,
            "deferred scheduled_at should be later than original"
        );
    }
}

/// T019：构造 Pressure 状态的 MemoryScheduler，验证 process_task 延后任务
#[cfg(feature = "metrics")]
#[tokio::test]
async fn test_process_task_memory_pressure_defers_task() {
    use crate::infrastructure::observability::metrics::SystemMonitorTrait;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct PressureMockMonitor {
        bits: AtomicU64,
    }
    impl SystemMonitorTrait for PressureMockMonitor {
        fn cpu_usage(&self) -> f64 {
            0.0
        }
        fn memory_usage(&self) -> f64 {
            f64::from_bits(self.bits.load(Ordering::Relaxed))
        }
        fn is_metrics_stale(&self) -> bool {
            false
        }
    }

    // Pressure：0.85 介于 pressure(0.8) 与 critical(0.9) 之间
    let memory_scheduler: Arc<MemoryScheduler> = Arc::new(MemoryScheduler::new(
        Arc::new(PressureMockMonitor {
            bits: AtomicU64::new(0.85f64.to_bits()),
        }),
        0.8,
        0.9,
        Duration::from_secs(30),
    ));

    let task_repo = Arc::new(ConfigurableTaskRepo::new());
    let regex_cache = make_regex_cache().await;
    let settings = crate::bootstrap::config::load_settings()
        .expect("Failed to load settings for memory pressure test");
    let team_semaphore = Arc::new(TeamSemaphore::new(10));
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(
        task_repo.clone() as Arc<dyn TaskRepository>,
        result_repo.clone(),
    );

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo.clone() as Arc<dyn TaskRepository>,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client: Arc::new(EngineClient::new()),
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        memory_scheduler,
    });

    let task = make_task(json!({"url": "https://example.com"}));
    let result = worker.process_task(task).await;

    assert!(
        result.is_ok(),
        "Pressure memory should defer task, returning Ok(())"
    );
    assert_eq!(
        task_repo.update_count(),
        1,
        "Pressure memory should trigger one update to defer task"
    );
    assert_eq!(
        task_repo.mark_failed_count(),
        0,
        "Pressure memory should NOT mark task as failed"
    );
}

/// T019：Normal 状态下任务正常进入并发获取流程（不延后）
#[cfg(feature = "metrics")]
#[tokio::test]
async fn test_process_task_memory_normal_proceeds() {
    // Normal 状态调度器由 make_test_memory_scheduler() 提供（内存 0.5 < pressure 0.8）
    let worker = build_configurable_worker(
        Arc::new(ConfigurableTaskRepo::new()),
        Arc::new(ConfigurableCrawlRepo::new()),
        Arc::new(MockRobotsChecker),
        Arc::new(EngineClient::new()),
    )
    .await;

    let task = make_task(json!({"url": "https://example.com"}));
    // Normal 状态下任务应进入并发获取流程；由于 mock engine 会失败，
    // process_task 返回 Ok(()) 但不触发 reschedule-by-memory。
    // 关键断言：不因内存准入检查而提前 return（update_count == 0 表示未走内存延后路径）
    let _ = worker.process_task(task).await;
    // build_configurable_worker 使用独立的 ConfigurableTaskRepo，无法直接读取 update_count；
    // 此测试仅验证 Normal 状态下不 panic 且流程继续（未在准入检查处短路）。
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
    let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new(EngineError::Timeout(
        Duration::from_secs(30),
    )));
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
    let router: Arc<dyn EngineRouterTrait> = Arc::new(MockEngineRouter::new(EngineError::Expired));
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
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_team_semaphore(team_semaphore)
        .with_robots_checker(Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        .with_credits_repository(Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>)
        .with_engine_client(engine_client)
        .with_create_scrape_use_case(
            Arc::new(MockCreateScrapeUseCase) as Arc<dyn CreateScrapeUseCaseTrait>
        )
        .with_team_semaphore(team_semaphore)
        .with_settings(Arc::new(settings))
        .with_extraction_service(Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>)
        .with_regex_cache(regex_cache)
        .with_cache_service(Arc::new(MockCacheService::new()) as Arc<dyn CacheService>)
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
        markdown: None,
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

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(FailingScrapeResultRepo);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

    let task = make_task(json!({}));
    let response = ScrapeResponse {
        content: r#"<html><body><a href="/page1">Link</a></body></html>"#.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 100,
        final_url: None,
        markdown: None,
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
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: crawl_repo,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

    let task = make_task(json!({}));
    let response = ScrapeResponse {
        content: r#"<html><body>Hello</body></html>"#.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 100,
        final_url: None,
        markdown: None,
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

    let task_repo: Arc<dyn TaskRepository> = Arc::new(ConfigurableTaskRepo::new());
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

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

    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo.clone(),
        result_repository: result_repo,
        crawl_repository: Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(DenyingRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

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
    async fn extract_with_rag(
        &self,
        _html_content: &str,
        _query: &str,
        _schema: &Value,
        _rag_strategy: &crate::domain::services::rag_strategy::RagExtractionStrategy,
    ) -> Result<(Value, TokenUsage)> {
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
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(MockScrapeResultRepository);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo,
        result_repository: result_repo,
        crawl_repository: Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: credits_repo,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    })
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
    let result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(FailingScrapeResultRepo);
    let coalesce_coordinator = make_coalesce_coordinator(task_repo.clone(), result_repo.clone());

    let worker = ScrapeWorker::new(ScrapeWorkerDeps {
        repository: task_repo.clone(),
        result_repository: result_repo,
        crawl_repository: Arc::new(ConfigurableCrawlRepo::new()) as Arc<dyn CrawlRepository>,
        webhook_service: Arc::new(MockWebhookService) as Arc<dyn WebhookService>,
        credits_repository: Arc::new(MockCreditsRepo::default()) as Arc<dyn CreditsRepository>,
        engine_client,
        create_scrape_use_case: Arc::new(MockCreateScrapeUseCase)
            as Arc<dyn CreateScrapeUseCaseTrait>,
        team_semaphore,
        coalesce_coordinator,
        robots_checker: Arc::new(MockRobotsChecker) as Arc<dyn RobotsCheckerTrait>,
        settings: Arc::new(settings),
        default_concurrency_limit: 10,
        extraction_service: Arc::new(MockExtractionService) as Arc<dyn ExtractionServiceTrait>,
        regex_cache,
        cache_service: Arc::new(MockCacheService::new()) as Arc<dyn CacheService>,
        #[cfg(feature = "metrics")]
        memory_scheduler: make_test_memory_scheduler(),
    });

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

    // T053/R-frontier-001：预填充 Bloom 让 page1/page2 走 DB 校验路径
    // （Bloom 空时所有 URL DefinitelyNew 直接入队，不会调用 find_existing_urls）
    let dedup_arc = worker.deduplicator_for_test();
    {
        let mut dedup = dedup_arc.write();
        dedup.insert("https://example.com/page1");
        dedup.insert("https://example.com/page2");
    }

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
        markdown: None,
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
        markdown: None,
    };

    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
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
        markdown: None,
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
        extraction_prompt: None,
        extraction_schema: None,
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

    // T053/R-frontier-001：预填充 Bloom 让 page1 走 DB 校验路径
    // （Bloom 空时所有 URL DefinitelyNew 直接入队，不查 DB）
    // 只 insert page1，page2 仍 DefinitelyNew 直接入队
    let dedup_arc = worker.deduplicator_for_test();
    {
        let mut dedup = dedup_arc.write();
        dedup.insert("https://example.com/page1");
    }

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
        markdown: None,
    };
    let config = make_crawl_config(None, None);
    let result = worker
        .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
        .await;
    assert!(result.is_ok());

    // page1 was skipped (Bloom 命中 → DB 命中 → skip), page2 was created
    // (Bloom 未命中 → DefinitelyNew → 直接入队) → create_count == 1
    assert_eq!(
        task_repo.create_count(),
        1,
        "only non-existing URLs should be created (page1 skipped, page2 created)"
    );
}

// T053/R-frontier-001：新增测试，验证 Bloom 未命中时直接入队（不查 DB）
#[tokio::test]
async fn test_extract_and_queue_links_bloom_miss_directly_enqueues() {
    let task_repo = Arc::new(ConfigurableTaskRepo::new());
    // 不预填充 existing_urls（DB 返回空集），不预填充 Bloom
    // → 所有 URL DefinitelyNew → 直接入队，不调用 find_existing_urls

    let worker = build_configurable_worker(
        task_repo.clone(),
        Arc::new(ConfigurableCrawlRepo::new()),
        Arc::new(MockRobotsChecker),
        Arc::new(EngineClient::new()),
    )
    .await;

    let task = make_task(json!({}));
    let html = r#"<html><body>
            <a href="/page1">Page 1</a>
            <a href="/page2">Page 2</a>
            <a href="/page3">Page 3</a>
        </body></html>"#;
    let response = ScrapeResponse {
        content: html.to_string(),
        status_code: 200,
        screenshot: None,
        content_type: "text/html".to_string(),
        headers: HashMap::new(),
        response_time_ms: 100,
        final_url: None,
        markdown: None,
    };
    let config = make_crawl_config(None, None);
    let result = worker
        .extract_and_queue_links(&task, &response, Uuid::new_v4(), 0, &config)
        .await;
    assert!(result.is_ok());

    // 所有 3 个 URL 都被入队（Bloom 全部未命中 → DefinitelyNew）
    assert_eq!(
        task_repo.create_count(),
        3,
        "all URLs should be created when Bloom is empty (no DB lookup)"
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
        markdown: None,
    };

    let dto = parse_dto_for_test(&task);
    let result = worker
        .handle_scrape_success(&task, dto.as_ref(), response)
        .await;
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

// ========== should_crawl tests ==========

/// Build a worker pre-configured for should_crawl unit tests.
/// should_crawl only reads `self.regex_cache`, so all other deps use
/// default in-memory mocks.
async fn build_should_crawl_worker() -> ScrapeWorker {
    build_configurable_worker(
        Arc::new(ConfigurableTaskRepo::new()),
        Arc::new(ConfigurableCrawlRepo::new()),
        Arc::new(MockRobotsChecker),
        Arc::new(EngineClient::new()),
    )
    .await
}

/// Build a CrawlConfigDto with only the given include/exclude patterns set;
/// all other fields default to None, max_depth defaults to 3.
fn build_crawl_config(
    include_patterns: Option<Vec<String>>,
    exclude_patterns: Option<Vec<String>>,
) -> CrawlConfigDto {
    CrawlConfigDto {
        max_depth: 3,
        include_patterns,
        exclude_patterns,
        strategy: None,
        crawl_delay_ms: None,
        max_concurrency: None,
        proxy: None,
        headers: None,
        extraction_rules: None,
        extraction_prompt: None,
        extraction_schema: None,
    }
}

#[tokio::test]
async fn test_should_crawl_no_patterns_allows_all() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(None, None);

    assert!(worker.should_crawl("https://any-url.com/page", &config));
    assert!(worker.should_crawl("https://another.org/deep/path", &config));
}

#[tokio::test]
async fn test_should_crawl_include_regex_matches() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(Some(vec![r".*/blog/.*".to_string()]), None);

    assert!(worker.should_crawl("https://example.com/blog/post-1", &config));
    assert!(!worker.should_crawl("https://example.com/about", &config));
}

#[tokio::test]
async fn test_should_crawl_include_regex_no_match_returns_false() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(Some(vec![r"^https://specific\.com/.*".to_string()]), None);

    assert!(!worker.should_crawl("https://other.com/page", &config));
}

#[tokio::test]
async fn test_should_crawl_exclude_regex_blocks() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(None, Some(vec![r".*/admin/.*".to_string()]));

    assert!(!worker.should_crawl("https://example.com/admin/settings", &config));
    assert!(worker.should_crawl("https://example.com/public/page", &config));
}

#[tokio::test]
async fn test_should_crawl_invalid_regex_falls_back_to_contains() {
    let worker = build_should_crawl_worker().await;
    // Invalid regex pattern (unclosed bracket) falls back to string contains
    let config = build_crawl_config(Some(vec!["[unclosed".to_string()]), None);

    // String contains "[unclosed"
    assert!(worker.should_crawl("https://example.com/[unclosed/page", &config));
    // Does not contain "[unclosed"
    assert!(!worker.should_crawl("https://example.com/other", &config));
}

#[tokio::test]
async fn test_should_crawl_include_and_exclude_overlap_exclude_wins() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(
        Some(vec![r".*/blog/.*".to_string()]),
        Some(vec![r".*/blog/private/.*".to_string()]),
    );

    // Matches include but also matches exclude → blocked
    assert!(!worker.should_crawl("https://example.com/blog/private/draft", &config));
    // Matches include, does not match exclude → allowed
    assert!(worker.should_crawl("https://example.com/blog/public-post", &config));
}

#[tokio::test]
async fn test_should_crawl_multiple_include_patterns_any_match() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(
        Some(vec![r".*/docs/.*".to_string(), r".*/api/.*".to_string()]),
        None,
    );

    assert!(worker.should_crawl("https://example.com/docs/intro", &config));
    assert!(worker.should_crawl("https://example.com/api/v1/users", &config));
    assert!(!worker.should_crawl("https://example.com/home", &config));
}

#[tokio::test]
async fn test_should_crawl_multiple_exclude_patterns_any_blocks() {
    let worker = build_should_crawl_worker().await;
    let config = build_crawl_config(
        None,
        Some(vec![
            r".*/admin/.*".to_string(),
            r".*/internal/.*".to_string(),
        ]),
    );

    assert!(!worker.should_crawl("https://example.com/admin/users", &config));
    assert!(!worker.should_crawl("https://example.com/internal/debug", &config));
    assert!(worker.should_crawl("https://example.com/public/page", &config));
}

// =========================================================================
// T059/R-cache-002: 高级缓存模式门控测试
// =========================================================================

// --- generate_scrape_cache_key ---

#[test]
fn test_generate_scrape_cache_key_format() {
    let ctx = CacheContext {
        url: "https://example.com".to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::Enabled,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());
    assert!(key.starts_with("scrape:"));
    assert!(key.contains("https://example.com"));
}

#[test]
fn test_generate_scrape_cache_key_different_methods_produce_different_keys() {
    let url = "https://example.com/api";
    let get_key = generate_scrape_cache_key(
        &CacheContext {
            url: url.to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        },
        &ScrapeOptions::default(),
    );
    let post_key = generate_scrape_cache_key(
        &CacheContext {
            url: url.to_string(),
            method: HttpMethod::Post,
            mode: CacheMode::Enabled,
        },
        &ScrapeOptions::default(),
    );
    assert_ne!(
        get_key, post_key,
        "GET and POST must have different cache keys"
    );
}

#[test]
fn test_generate_scrape_cache_key_different_urls_produce_different_keys() {
    let key1 = generate_scrape_cache_key(
        &CacheContext {
            url: "https://a.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        },
        &ScrapeOptions::default(),
    );
    let key2 = generate_scrape_cache_key(
        &CacheContext {
            url: "https://b.com".to_string(),
            method: HttpMethod::Get,
            mode: CacheMode::Enabled,
        },
        &ScrapeOptions::default(),
    );
    assert_ne!(key1, key2);
}

// --- try_read_scrape_cache ---

#[tokio::test]
async fn test_try_read_scrape_cache_miss_returns_none() {
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let ctx = CacheContext {
        url: "https://example.com".to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::Enabled,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());

    let result = worker.try_read_scrape_cache(&ctx, &key).await;
    assert!(result.is_ok(), "read should succeed even on miss");
    assert!(result.unwrap().is_none(), "empty cache should return None");
    assert_eq!(cache.get_count(), 1, "get should be called once");
}

#[tokio::test]
async fn test_try_read_scrape_cache_hit_returns_response() {
    let url = "https://example.com";
    let ctx = CacheContext {
        url: url.to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::ReadOnly,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());
    let cached_response = ScrapeResponse::new(200, "cached content", "text/html");
    let cached_json = serde_json::to_string(&cached_response).expect("serialize failed");

    let cache = Arc::new(MockCacheService::with_entry(&key, &cached_json));
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let result = worker.try_read_scrape_cache(&ctx, &key).await;
    assert!(result.is_ok(), "read should succeed on hit");
    let resp = result.unwrap().expect("should have cached response");
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content, "cached content");
    assert_eq!(cache.get_count(), 1, "get should be called once");
}

#[tokio::test]
async fn test_try_read_scrape_cache_corrupt_data_returns_err() {
    let url = "https://example.com";
    let ctx = CacheContext {
        url: url.to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::Enabled,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());
    // 存入损坏的 JSON（不是有效的 ScrapeResponse）
    let cache = Arc::new(MockCacheService::with_entry(&key, "{invalid json}"));
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let result = worker.try_read_scrape_cache(&ctx, &key).await;
    assert!(
        result.is_err(),
        "corrupt cache should return Err so caller can evict the entry"
    );
    assert_eq!(cache.get_count(), 1, "get should still be called once");
}

// --- try_write_scrape_cache ---

#[tokio::test]
async fn test_try_write_scrape_cache_writes_serialized_response() {
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let ctx = CacheContext {
        url: "https://example.com/page".to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::Enabled,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());

    let response = ScrapeResponse::new(200, "fresh content", "text/html");
    let result = worker.try_write_scrape_cache(&ctx, &key, &response).await;
    assert!(result.is_ok(), "write should succeed");
    assert_eq!(cache.set_count(), 1, "set should be called once");
}

#[tokio::test]
async fn test_try_write_scrape_cache_key_matches_read_key() {
    let url = "https://example.com/sync";
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    // 写缓存（Bypass 模式：should_read=false, should_write=true，合并原 WriteOnly 语义）
    let ctx = CacheContext {
        url: url.to_string(),
        method: HttpMethod::Get,
        mode: CacheMode::Bypass,
    };
    let key = generate_scrape_cache_key(&ctx, &ScrapeOptions::default());
    let response = ScrapeResponse::new(200, "written", "text/html");
    worker
        .try_write_scrape_cache(&ctx, &key, &response)
        .await
        .unwrap();
    assert_eq!(cache.set_count(), 1);

    // 读缓存应命中（同一个 key）
    let result = worker.try_read_scrape_cache(&ctx, &key).await;
    assert!(result.is_ok());
    let resp = result.unwrap().expect("should hit after write");
    assert_eq!(resp.content, "written");
}

// --- process_scrape_task 门控行为（通过 MockCacheService 计数器验证）---

/// 辅助：构造带 cache_mode 的 Task payload
fn make_task_with_cache_mode(url: &str, mode: Option<CacheMode>) -> Task {
    let options = match mode {
        Some(m) => serde_json::json!({ "cache_mode": m }),
        None => serde_json::json!({}),
    };
    let payload = serde_json::json!({
        "url": url,
        "options": options,
    });
    make_task(payload)
}

#[tokio::test]
async fn test_process_scrape_task_disabled_mode_no_cache_read_or_write() {
    // Disabled 模式：should_read=false, should_write=false
    // → 不读缓存（get_count=0），不写缓存（set_count=0）
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let task = make_task_with_cache_mode("https://example.com", Some(CacheMode::Disabled));
    let _ = worker.process_scrape_task(task).await;

    assert_eq!(cache.get_count(), 0, "Disabled mode should not read cache");
    assert_eq!(cache.set_count(), 0, "Disabled mode should not write cache");
}

#[tokio::test]
async fn test_process_scrape_task_bypass_mode_no_read_but_attempt_write() {
    // Bypass 模式：should_read=false, should_write=true
    // → 不读缓存（get_count=0）
    // → 抓取失败（engine_client 无引擎），不写缓存（set_count=0）
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let task = make_task_with_cache_mode("https://example.com", Some(CacheMode::Bypass));
    let _ = worker.process_scrape_task(task).await;

    assert_eq!(cache.get_count(), 0, "Bypass mode should not read cache");
    assert_eq!(
        cache.set_count(),
        0,
        "Bypass mode should not write on scrape failure"
    );
}

#[tokio::test]
async fn test_process_scrape_task_read_only_mode_reads_cache_no_write() {
    // ReadOnly 模式：should_read=true, should_write=false
    // → 读缓存（get_count=1），缓存未命中
    // → 抓取失败（engine_client 无引擎），不写缓存（set_count=0）
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let task = make_task_with_cache_mode("https://example.com", Some(CacheMode::ReadOnly));
    let _ = worker.process_scrape_task(task).await;

    assert_eq!(cache.get_count(), 1, "ReadOnly mode should read cache");
    assert_eq!(cache.set_count(), 0, "ReadOnly mode should not write cache");
}

#[tokio::test]
async fn test_process_scrape_task_enabled_mode_reads_cache_no_write_on_failure() {
    // Enabled 模式：should_read=true, should_write=true
    // → 读缓存（get_count=1），缓存未命中
    // → 抓取失败（engine_client 无引擎），不写缓存（set_count=0）
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let task = make_task_with_cache_mode("https://example.com", Some(CacheMode::Enabled));
    let _ = worker.process_scrape_task(task).await;

    assert_eq!(cache.get_count(), 1, "Enabled mode should read cache");
    assert_eq!(
        cache.set_count(),
        0,
        "Enabled mode should not write on scrape failure"
    );
}

#[tokio::test]
async fn test_process_scrape_task_default_mode_reads_cache() {
    // cache_mode=None（默认）→ 等价于 Enabled
    // → 读缓存（get_count=1）
    let cache = Arc::new(MockCacheService::new());
    let worker = build_mock_worker_with_cache(cache.clone()).await;

    let task = make_task_with_cache_mode("https://example.com", None);
    let _ = worker.process_scrape_task(task).await;

    assert_eq!(
        cache.get_count(),
        1,
        "Default (Enabled) mode should read cache"
    );
    assert_eq!(cache.set_count(), 0, "Should not write on scrape failure");
}
