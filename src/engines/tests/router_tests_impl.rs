use super::*;
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

// A simple test engine that is a controllable implementation
struct TestScraperEngineImpl {
    name: &'static str,
    _supported_domains: Vec<String>,
    _weight: u8,
    response_content: String,
    is_error: bool,
    call_count: AtomicU32,
    max_calls: u32,
}

impl TestScraperEngineImpl {
    fn new(
        name: &'static str,
        supported_domains: Vec<String>,
        weight: u8,
        result: Result<InternalScrapeResponse, EngineError>,
        max_calls: u32,
    ) -> Self {
        match result {
            Ok(resp) => Self {
                name,
                _supported_domains: supported_domains,
                _weight: weight,
                response_content: resp.content,
                is_error: false,
                call_count: AtomicU32::new(0),
                max_calls,
            },
            Err(_) => Self {
                name,
                _supported_domains: supported_domains,
                _weight: weight,
                response_content: String::new(),
                is_error: true,
                call_count: AtomicU32::new(0),
                max_calls,
            },
        }
    }
}

#[async_trait]
impl ScraperEngine for TestScraperEngineImpl {
    async fn scrape(
        &self,
        _request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let call_count = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

        if call_count <= self.max_calls {
            if self.is_error {
                return Err(EngineError::Timeout(Duration::from_secs(30)));
            }
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: self.response_content.clone(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 100,
            })
        } else {
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "Default Result".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 100,
            })
        }
    }

    fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
        100
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

#[tokio::test]
async fn test_aggregate_concurrent_search() {
    let engine1 = TestScraperEngineImpl::new(
        "engine1",
        vec!["example.com".to_string()],
        1,
        Ok(InternalScrapeResponse {
            status_code: 200,
            content: "Result 1".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
        }),
        10, // max_calls
    );

    let engine2 = TestScraperEngineImpl::new(
        "engine2",
        vec!["example.com".to_string()],
        1,
        Ok(InternalScrapeResponse {
            status_code: 200,
            content: "Result 2".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
        }),
        10, // max_calls
    );

    let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

    let request = InternalScrapeRequest {
        url: "http://example.com".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };
    let result = router.aggregate(&request).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.content.contains("Result"));
}

#[tokio::test]
async fn test_aggregate_partial_failure() {
    let engine1 = TestScraperEngineImpl::new(
        "engine1",
        vec!["example.com".to_string()],
        1,
        Err(EngineError::Timeout(Duration::from_secs(30))),
        10, // max_calls
    );

    let engine2 = TestScraperEngineImpl::new(
        "engine2",
        vec!["example.com".to_string()],
        1,
        Ok(InternalScrapeResponse {
            status_code: 200,
            content: "Result 2".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 100,
        }),
        10, // max_calls
    );

    let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

    let request = InternalScrapeRequest {
        url: "http://example.com".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };
    let result = router.aggregate(&request).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.content, "Result 2");
}

// === T013（R-antibot-003）：反爬挑战页改派浏览器引擎 ===
//
// 验证：HTTP 引擎返回 Cloudflare 挑战页 HTML（status=200），被 antibot::classify 判
// needs_browser=true，路由将其视为失败、强制后续 attempt needs_js=true，由浏览器引擎
// 接管并返回正常结果。
//
// 仅在 `antibot` feature 启用时编译——`check_antibot_response` 与 cfg 块都依赖该 feature。
#[cfg(feature = "content")]
#[tokio::test]
async fn test_t013_antibot_cloudflare_forces_needs_js_for_next_attempt() {
    use std::sync::Mutex;

    /// 记录每次调用时的 `needs_js` 值，用于断言改派行为
    struct NeedsJsRecordingEngine {
        name: &'static str,
        /// 用 Mutex 包装 Vec 以满足 ScraperEngine 的 Send+Sync 约束
        recorded_needs_js: Arc<Mutex<Vec<bool>>>,
        response: InternalScrapeResponse,
    }

    #[async_trait]
    impl ScraperEngine for NeedsJsRecordingEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.recorded_needs_js
                .lock()
                .expect("lock recorded_needs_js")
                .push(request.needs_js);
            Ok(self.response.clone())
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    // Cloudflare 挑战页：命中 Tier1 /cdn-cgi/challenge-platform/ 标记
    let cloudflare_body = concat!(
        "<html><head><title>Just a moment...</title></head>",
        "<body>",
        "<script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/jsch/v1\"></script>",
        "</body></html>"
    );

    let http_record = Arc::new(Mutex::new(Vec::new()));
    let http_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
        name: "http-reqwest",
        recorded_needs_js: http_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: cloudflare_body.to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 50,
        },
    });

    // 浏览器引擎应最终返回正常正文（body 需足够长且可见文本 >= 50 字符，
    // 避免被 antibot Tier3 近空页检测误判为 StructuralBlock）
    let browser_record = Arc::new(Mutex::new(Vec::new()));
    let browser_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
        name: "browser-playwright",
        recorded_needs_js: browser_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: "<html><body>This is the real rendered content from the browser \
                           engine after JavaScript execution completed successfully.</body></html>"
                .to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 200,
        },
    });

    let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
    // 关闭特征过滤与竞速，确保走顺序 fallback
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(2);
    router.set_max_retries(2);

    let request = InternalScrapeRequest {
        url: "https://example.com/protected".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(
        result.is_ok(),
        "route should succeed via browser engine after antibot block, got: {:?}",
        result.err()
    );
    let resp = result.unwrap();
    assert!(resp
        .content
        .contains("real rendered content from the browser"));

    // HTTP 引擎被调用 1 次，且 needs_js 与原始请求一致（false）
    let http_calls = http_record.lock().unwrap().clone();
    assert_eq!(
        http_calls.len(),
        1,
        "http engine should be called exactly once"
    );
    assert!(
        !http_calls[0],
        "first attempt must have needs_js=false (original request)"
    );

    // 浏览器引擎被调用 1 次，且 needs_js=true（强制升级）
    let browser_calls = browser_record.lock().unwrap().clone();
    assert_eq!(
        browser_calls.len(),
        1,
        "browser engine should be called exactly once"
    );
    assert!(
        browser_calls[0],
        "second attempt must have needs_js=true (force_needs_js after antibot block)"
    );
}

/// T013 边界：HTTP 引擎返回正常页面（非反爬挑战），不应触发 force_needs_js
#[cfg(feature = "content")]
#[tokio::test]
async fn test_t013_normal_response_does_not_trigger_force_needs_js() {
    use std::sync::Mutex;

    struct SingleCallEngine {
        name: &'static str,
        recorded_needs_js: Arc<Mutex<Vec<bool>>>,
        response: InternalScrapeResponse,
    }

    #[async_trait]
    impl ScraperEngine for SingleCallEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.recorded_needs_js
                .lock()
                .expect("lock recorded_needs_js")
                .push(request.needs_js);
            Ok(self.response.clone())
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    let http_record = Arc::new(Mutex::new(Vec::new()));
    let http_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
        name: "http-reqwest",
        recorded_needs_js: http_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: "<html><body>Normal page with sufficient visible text content \
                           to pass tier3 structural checks.</body></html>"
                .to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 30,
        },
    });

    // 第二引擎不应被调用
    let browser_record = Arc::new(Mutex::new(Vec::new()));
    let browser_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
        name: "browser-playwright",
        recorded_needs_js: browser_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: "browser content".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 0,
        },
    });

    let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(2);
    router.set_max_retries(2);

    let request = InternalScrapeRequest {
        url: "https://example.com/normal".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().content, "<html><body>Normal page with sufficient visible text content to pass tier3 structural checks.</body></html>");

    // HTTP 引擎被调用 1 次，needs_js=false
    let http_calls = http_record.lock().unwrap().clone();
    assert_eq!(http_calls.len(), 1);
    assert!(!http_calls[0]);

    // 浏览器引擎不应被调用
    let browser_calls = browser_record.lock().unwrap().clone();
    assert!(
        browser_calls.is_empty(),
        "browser engine should NOT be called for normal response"
    );
}

// === T015（R-jsrender-001）：SPA 空壳响应触发改派浏览器引擎 ===
//
// 验证：HTTP 引擎（needs_js==false）返回含 `__NEXT_DATA__` 的 SPA 空壳响应，
// JsUpgradeProbe 判定 upgrade=true，路由以 needs_js=true 重新 route_internal
// 改派浏览器引擎，最终返回浏览器引擎渲染后的真实内容。
//
// 防递归：递归调用时 request.needs_js=true，attempt_request.needs_js=true，
// 故 `!attempt_request.needs_js` 为 false，probe 检查自然跳过。
#[tokio::test]
async fn test_t015_spa_shell_triggers_js_upgrade_re_dispatch() {
    use std::sync::Mutex;

    struct NeedsJsRecordingEngine {
        name: &'static str,
        recorded_needs_js: Arc<Mutex<Vec<bool>>>,
        response: InternalScrapeResponse,
    }

    #[async_trait]
    impl ScraperEngine for NeedsJsRecordingEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.recorded_needs_js
                .lock()
                .expect("lock recorded_needs_js")
                .push(request.needs_js);
            Ok(self.response.clone())
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    // SPA 空壳：含 __NEXT_DATA__ 强信号（probe score=10 >= threshold 10）
    // 但可见文本 >= 50 字符，避免被 antibot Tier3 误判为 StructuralBlock
    let spa_shell = concat!(
        r#"<html><head>"#,
        r#"<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>"#,
        r#"</head><body>"#,
        r#"Loading... please wait while we render the content for you. "#,
        r#"This page requires JavaScript to function properly."#,
        r#"</body></html>"#
    );

    let http_record = Arc::new(Mutex::new(Vec::new()));
    let http_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
        name: "http-reqwest",
        recorded_needs_js: http_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: spa_shell.to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 30,
        },
    });

    // 浏览器引擎返回渲染后的真实内容（可见文本 >= 50 避免 antibot 误判）
    let browser_record = Arc::new(Mutex::new(Vec::new()));
    let browser_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
        name: "browser-playwright",
        recorded_needs_js: browser_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: "<html><body>This is the fully rendered content from the browser \
                           engine after JavaScript execution completed successfully.</body></html>"
                .to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 200,
        },
    });

    let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(2);
    router.set_max_retries(2);

    let request = InternalScrapeRequest {
        url: "https://example.com/spa-page".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(
        result.is_ok(),
        "route should succeed via browser engine after SPA shell probe, got: {:?}",
        result.err()
    );
    let resp = result.unwrap();
    assert!(
        resp.content
            .contains("fully rendered content from the browser"),
        "should return browser engine's rendered content, got: {}",
        resp.content
    );

    // HTTP 引擎被调用 1 次，needs_js=false
    let http_calls = http_record.lock().unwrap().clone();
    assert_eq!(
        http_calls.len(),
        1,
        "http engine should be called exactly once"
    );
    assert!(
        !http_calls[0],
        "http engine attempt must have needs_js=false (original request)"
    );

    // 浏览器引擎被调用 1 次，needs_js=true（probe 触发改派）
    let browser_calls = browser_record.lock().unwrap().clone();
    assert_eq!(
        browser_calls.len(),
        1,
        "browser engine should be called exactly once"
    );
    assert!(
        browser_calls[0],
        "browser engine attempt must have needs_js=true (probe-triggered re-route)"
    );
}

/// T015 边界：HTTP 引擎返回非 SPA 页面（无 JS 框架信号），不应触发改派
#[tokio::test]
async fn test_t015_non_spa_response_does_not_trigger_re_dispatch() {
    use std::sync::Mutex;

    struct SingleCallEngine {
        name: &'static str,
        recorded_needs_js: Arc<Mutex<Vec<bool>>>,
        response: InternalScrapeResponse,
    }

    #[async_trait]
    impl ScraperEngine for SingleCallEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.recorded_needs_js
                .lock()
                .expect("lock recorded_needs_js")
                .push(request.needs_js);
            Ok(self.response.clone())
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    let http_record = Arc::new(Mutex::new(Vec::new()));
    let http_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
            name: "http-reqwest",
            recorded_needs_js: http_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>This is a static page with sufficient visible text content \
                           to pass all antibot and probe checks. No SPA framework signals here.</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 30,
            },
        });

    let browser_record = Arc::new(Mutex::new(Vec::new()));
    let browser_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
        name: "browser-playwright",
        recorded_needs_js: browser_record.clone(),
        response: InternalScrapeResponse {
            status_code: 200,
            content: "browser content".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 0,
        },
    });

    let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(2);
    router.set_max_retries(2);

    let request = InternalScrapeRequest {
        url: "https://example.com/static-page".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(result.is_ok());
    assert!(result
        .unwrap()
        .content
        .contains("static page with sufficient visible text"));

    // HTTP 引擎被调用 1 次，needs_js=false
    let http_calls = http_record.lock().unwrap().clone();
    assert_eq!(http_calls.len(), 1);
    assert!(!http_calls[0]);

    // 浏览器引擎不应被调用（非 SPA，不触发 probe）
    let browser_calls = browser_record.lock().unwrap().clone();
    assert!(
        browser_calls.is_empty(),
        "browser engine should NOT be called for non-SPA response"
    );
}

/// T028（R-identity-002）：验证 Transient 错误重试时 UA 按 attempt seed 轮换。
///
/// 场景：3 个失败引擎（Transient）+ 1 个成功引擎，max_retries=4。
/// 预期 directive 序列：
/// - attempt 1 (total=1)：default，无 UA 轮换
/// - attempt 2 (total=2, da=0)：Transient attempt=0 → default，无 UA 轮换
/// - attempt 3 (total=3, da=1)：Transient attempt=1 → rotate_ua=true，seed=2
/// - attempt 4 (total=4, da=2)：Transient attempt=2 → rotate_ua=true，seed=3
#[tokio::test]
async fn test_t028_ua_rotated_across_transient_retries() {
    use std::sync::Mutex;

    /// 记录每次调用的 User-Agent header（None 表示未注入）。
    /// 使用 `error_msg` 标签 + 消息构造错误，避免 `EngineError: Clone` 依赖。
    struct UaRecordingEngine {
        name: &'static str,
        recorded_ua: Arc<Mutex<Vec<Option<String>>>>,
        /// `Some(msg)` → 返回 `EngineError::RequestFailed(msg)`；`None` → 返回成功响应
        error_msg: Option<String>,
        /// 返回 Ok 时的响应
        response: Option<InternalScrapeResponse>,
    }

    #[async_trait]
    impl ScraperEngine for UaRecordingEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            let ua = request.headers.get("User-Agent").map(|v| v.to_string());
            self.recorded_ua.lock().expect("lock recorded_ua").push(ua);
            if let Some(ref msg) = self.error_msg {
                return Err(EngineError::RequestFailed(msg.clone()));
            }
            Ok(self.response.clone().expect("success response must be set"))
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    fn make_failing_engine(
        name: &'static str,
        record: Arc<Mutex<Vec<Option<String>>>>,
        error_msg: &str,
    ) -> Arc<dyn ScraperEngine> {
        Arc::new(UaRecordingEngine {
            name,
            recorded_ua: record,
            error_msg: Some(error_msg.to_string()),
            response: None,
        })
    }

    fn make_success_engine(
        name: &'static str,
        record: Arc<Mutex<Vec<Option<String>>>>,
    ) -> Arc<dyn ScraperEngine> {
        Arc::new(UaRecordingEngine {
            name,
            recorded_ua: record,
            error_msg: None,
            response: Some(InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>success content with enough visible text to pass \
                              any antibot or probe checks along the retry path</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 10,
            }),
        })
    }

    let rec1 = Arc::new(Mutex::new(Vec::new()));
    let rec2 = Arc::new(Mutex::new(Vec::new()));
    let rec3 = Arc::new(Mutex::new(Vec::new()));
    let rec4 = Arc::new(Mutex::new(Vec::new()));

    let engines: Vec<Arc<dyn ScraperEngine>> = vec![
        make_failing_engine("fail-1", rec1.clone(), "transient-1"),
        make_failing_engine("fail-2", rec2.clone(), "transient-2"),
        make_failing_engine("fail-3", rec3.clone(), "transient-3"),
        make_success_engine("success-4", rec4.clone()),
    ];

    let mut router = EngineRouter::new(engines);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(4);
    router.set_max_retries(4);

    let request = InternalScrapeRequest {
        url: "https://example.com/test".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(
        result.is_ok(),
        "route should succeed via 4th engine after 3 transient failures, got: {:?}",
        result.err()
    );

    // attempt 1：default directive，无 UA 轮换
    let r1 = rec1.lock().unwrap().clone();
    assert_eq!(r1.len(), 1, "engine 1 should be called exactly once");
    assert!(
        r1[0].is_none(),
        "attempt 1 must not rotate UA (default directive)"
    );

    // attempt 2：Transient attempt=0 → default，无 UA 轮换
    let r2 = rec2.lock().unwrap().clone();
    assert_eq!(r2.len(), 1, "engine 2 should be called exactly once");
    assert!(
        r2[0].is_none(),
        "attempt 2 must not rotate UA (Transient attempt=0 → default directive)"
    );

    // attempt 3：Transient attempt=1 → rotate_ua=true，seed=2
    let r3 = rec3.lock().unwrap().clone();
    assert_eq!(r3.len(), 1, "engine 3 should be called exactly once");
    assert!(
        r3[0].is_some(),
        "attempt 3 must rotate UA (Transient attempt=1 → rotate_ua=true)"
    );
    let ua3 = r3[0].clone().unwrap();

    // attempt 4：Transient attempt=2 → rotate_ua=true，seed=3
    let r4 = rec4.lock().unwrap().clone();
    assert_eq!(r4.len(), 1, "engine 4 should be called exactly once");
    assert!(
        r4[0].is_some(),
        "attempt 4 must rotate UA (Transient attempt=2 → rotate_ua=true)"
    );
    let ua4 = r4[0].clone().unwrap();

    // 不同 seed 必须返回不同 UA（pick_seeded(2) vs pick_seeded(3)，desktop pool ≥22）
    assert_ne!(
        ua3, ua4,
        "UA must differ across retry attempts (seed=2 vs seed=3)"
    );
}

/// C-1 回归测试：重试轮换 UA 时所有指纹相关 header 必须同步一致。
///
/// 场景：3 个失败引擎（Transient）+ 1 个成功引擎，max_retries=4。
/// 预期：attempt 3/4 触发 `directive.rotate_ua=true` 时，
///   - User-Agent / Accept-Language / sec-ch-ua 三者必须来自同一 profile
///   - 与 `UaPool::pick_seeded(seed, false)` 返回的 profile 字段严格相等
///
/// 修复前：router 只覆盖 User-Agent，Accept-Language 与 sec-ch-ua 仍是首次 profile 的值，
///         导致指纹矛盾（如 Chrome UA + Firefox sec-ch-ua）。
/// 修复后：三者一次性写入，保证指纹一致。
#[tokio::test]
async fn test_c1_fingerprint_headers_rotated_together() {
    use crate::utils::ua_pool::UaPool;
    use std::sync::Mutex;

    /// 记录每次调用全部指纹相关 header（UA / AL / sec-ch-ua）
    struct FingerprintRecordingEngine {
        name: &'static str,
        recorded: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
        error_msg: Option<String>,
        response: Option<InternalScrapeResponse>,
    }

    #[async_trait]
    impl ScraperEngine for FingerprintRecordingEngine {
        async fn scrape(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            let ua = request.headers.get("User-Agent").map(|v| v.to_string());
            let al = request
                .headers
                .get("Accept-Language")
                .map(|v| v.to_string());
            let ch = request.headers.get("sec-ch-ua").map(|v| v.to_string());
            self.recorded
                .lock()
                .expect("lock recorded")
                .push((ua, al, ch));
            if let Some(ref msg) = self.error_msg {
                return Err(EngineError::RequestFailed(msg.clone()));
            }
            Ok(self.response.clone().expect("success response must be set"))
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    fn make_failing(
        name: &'static str,
        rec: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
    ) -> Arc<dyn ScraperEngine> {
        Arc::new(FingerprintRecordingEngine {
            name,
            recorded: rec,
            error_msg: Some("transient".to_string()),
            response: None,
        })
    }

    fn make_success(
        name: &'static str,
        rec: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
    ) -> Arc<dyn ScraperEngine> {
        Arc::new(FingerprintRecordingEngine {
            name,
            recorded: rec,
            error_msg: None,
            response: Some(InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>success content with enough visible text to pass \
                              any antibot or probe checks along the retry path</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 10,
            }),
        })
    }

    let rec1 = Arc::new(Mutex::new(Vec::new()));
    let rec2 = Arc::new(Mutex::new(Vec::new()));
    let rec3 = Arc::new(Mutex::new(Vec::new()));
    let rec4 = Arc::new(Mutex::new(Vec::new()));

    let engines: Vec<Arc<dyn ScraperEngine>> = vec![
        make_failing("fail-1", rec1.clone()),
        make_failing("fail-2", rec2.clone()),
        make_failing("fail-3", rec3.clone()),
        make_success("success-4", rec4.clone()),
    ];

    let mut router = EngineRouter::new(engines);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(4);
    router.set_max_retries(4);

    let request = InternalScrapeRequest {
        url: "https://example.com/test".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(
        result.is_ok(),
        "route should succeed via 4th engine after 3 transient failures, got: {:?}",
        result.err()
    );

    // attempt 3：Transient attempt=1 → rotate_ua=true，seed=2
    let r3 = rec3.lock().unwrap().clone();
    assert_eq!(r3.len(), 1, "engine 3 should be called exactly once");
    let (ua3, al3, ch3) = r3[0].clone();
    assert!(ua3.is_some(), "attempt 3 must rotate User-Agent");
    let ua3 = ua3.expect("ua3 set");

    // attempt 4：Transient attempt=2 → rotate_ua=true，seed=3
    let r4 = rec4.lock().unwrap().clone();
    assert_eq!(r4.len(), 1, "engine 4 should be called exactly once");
    let (ua4, al4, ch4) = r4[0].clone();
    assert!(ua4.is_some(), "attempt 4 must rotate User-Agent");
    let ua4 = ua4.expect("ua4 set");

    // 与 UaPool.pick_seeded 的预期 profile 字段一致
    let pool = UaPool::new();
    let p3 = pool.pick_seeded(2, false);
    let p4 = pool.pick_seeded(3, false);

    // C-1 核心：UA + Accept-Language + sec-ch-ua 三者必须来自同一 profile
    assert_eq!(
        ua3, p3.ua,
        "attempt 3 User-Agent must match pick_seeded(2).ua"
    );
    assert_eq!(
        al3.as_deref(),
        Some(p3.accept_language),
        "attempt 3 Accept-Language must match profile.accept_language (C-1: 同步轮换)"
    );
    assert_eq!(
        ch3.as_deref(),
        if p3.sec_ch_ua.is_empty() {
            None
        } else {
            Some(p3.sec_ch_ua)
        },
        "attempt 3 sec-ch-ua must match profile.sec_ch_ua (C-1: 同步轮换，Firefox/Safari 为 None)"
    );

    assert_eq!(
        ua4, p4.ua,
        "attempt 4 User-Agent must match pick_seeded(3).ua"
    );
    assert_eq!(
        al4.as_deref(),
        Some(p4.accept_language),
        "attempt 4 Accept-Language must match profile.accept_language (C-1: 同步轮换)"
    );
    assert_eq!(
        ch4.as_deref(),
        if p4.sec_ch_ua.is_empty() {
            None
        } else {
            Some(p4.sec_ch_ua)
        },
        "attempt 4 sec-ch-ua must match profile.sec_ch_ua (C-1: 同步轮换)"
    );

    // 不同 seed 必须返回不同 UA
    assert_ne!(
        ua3, ua4,
        "UA must differ across retry attempts (seed=2 vs seed=3)"
    );
}

/// T028（R-identity-002）：验证 RetryTracker 在 FeatureToggle cap=3 时停止重试。
///
/// 场景：5 个引擎全部返回 `EngineError::FeatureToggle`，max_retries=5（高于 cap=3）。
/// 预期：tracker 在第 3 次 record 后 ft=3 → should_retry(FeatureToggle) 返回 false → 停止。
/// 即只调用前 3 个引擎，返回 FeatureToggle 错误。
#[tokio::test]
async fn test_t028_retry_tracker_caps_feature_toggle() {
    use std::sync::Mutex;

    struct FtFailingEngine {
        name: &'static str,
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl ScraperEngine for FtFailingEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            let mut c = self.call_count.lock().unwrap();
            *c += 1;
            Err(EngineError::FeatureToggle(format!("toggle-fail-{}", *c)))
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    let counts: Vec<Arc<Mutex<u32>>> = (0..5).map(|_| Arc::new(Mutex::new(0u32))).collect();

    let engines: Vec<Arc<dyn ScraperEngine>> = (0..5)
        .map(|i| {
            let e: Arc<dyn ScraperEngine> = Arc::new(FtFailingEngine {
                name: Box::leak(format!("ft-fail-{}", i).into_boxed_str()),
                call_count: counts[i].clone(),
            });
            e
        })
        .collect();

    let mut router = EngineRouter::new(engines);
    router.set_feature_filter_enabled(false);
    router.set_race_mode_enabled(false);
    router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
    router.set_max_engine_attempts(5);
    // max_retries=5 > feature_toggle cap=3，验证 tracker 先于 max_retries 触发
    router.set_max_retries(5);

    let request = InternalScrapeRequest {
        url: "https://example.com/ft-test".to_string(),
        method: crate::engines::engine_client::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: false,
        needs_tls_fingerprint: false,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let result = router.route(&request).await;
    assert!(
        result.is_err(),
        "route must fail after RetryTracker caps FeatureToggle"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, EngineError::FeatureToggle(_)),
        "error must be FeatureToggle, got: {:?}",
        err
    );

    // 验证只有前 3 个引擎被调用（cap=3 → 3 次 record 后停止）
    for i in 0..3 {
        let c = counts[i].lock().unwrap();
        assert_eq!(
            *c, 1,
            "engine {} should be called exactly once (within cap)",
            i
        );
    }
    for i in 3..5 {
        let c = counts[i].lock().unwrap();
        assert_eq!(
            *c, 0,
            "engine {} should NOT be called (RetryTracker stopped after cap=3)",
            i
        );
    }
}
