use super::*;

use crate::engines::engine_client::{EngineError, InternalScrapeRequest, InternalScrapeResponse};
use crate::engines::router::EngineStats;
use std::future::Future;
use std::pin::Pin;

mod cache_service;
mod cache_stats;
mod crawl_delay;
mod errors;
mod fail_closed;
mod is_allowed;

// RobotsChecker construction & parse_crawl_delay tests

fn make_checker() -> RobotsChecker {
    let http_client = Arc::new(reqwest::Client::new());
    RobotsChecker::new(
        http_client,
        &crate::config::settings::RobotsSettings::default(),
        None,
        None,
    )
}

// These tests pre-populate the memory cache to exercise the cache-hit path
// and the robots.txt matching logic without making real HTTP requests.

async fn populate_robots_cache(checker: &RobotsChecker, robots_url: &str, content: &str) {
    let mut cache = checker.memory_cache.lock().await;
    cache.insert(
        robots_url.to_string(),
        CachedRobots {
            content: content.to_string(),
            expires_at: Instant::now() + checker.cache_ttl,
            fetch_failed: false,
        },
    );
}

// ========== Mock infrastructure for HTTP fetch & cache_service path tests ==========
//
// The following mocks allow testing get_robots_content() paths that
// normally require real HTTP requests or a real cache service backend.
// A public IP (8.8.8.8) is used as the target host so SSRF validation
// passes without requiring DNS resolution (IP literals are parsed
// directly by lookup_host).

/// In-memory mock CacheService for testing the cache_service hit/miss paths.
struct MockCacheService {
    data: std::sync::Mutex<HashMap<String, String>>,
    set_count: AtomicU64,
}

impl MockCacheService {
    fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(HashMap::new()),
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

    fn set_count(&self) -> u64 {
        self.set_count.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl CacheService for MockCacheService {
    fn get(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + '_>> {
        let data = self.data.lock().unwrap().get(key).cloned();
        Box::pin(async move { Ok(data) })
    }

    fn set(
        &self,
        _key: &str,
        _value: &str,
        _ttl_seconds: u64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.set_count.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move { Ok(()) })
    }

    fn delete(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.data.lock().unwrap().remove(key);
        Box::pin(async move { Ok(()) })
    }

    fn exists(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + '_>> {
        let exists = self.data.lock().unwrap().contains_key(key);
        Box::pin(async move { Ok(exists) })
    }
}

struct MockEngineRouter {
    response: Option<InternalScrapeResponse>,
    error: Option<String>,
    call_count: AtomicU64,
}

impl MockEngineRouter {
    fn with_response(status_code: u16, content: &str) -> Self {
        Self {
            response: Some(InternalScrapeResponse {
                js_degraded: false,
                captured_responses: Vec::new(),
                status_code,
                content: content.to_string(),
                screenshot: None,
                content_type: "text/plain".to_string(),
                headers: HashMap::new(),
                response_time_ms: 0,
                final_url: None,
            }),
            error: None,
            call_count: AtomicU64::new(0),
        }
    }

    fn with_error(msg: &str) -> Self {
        Self {
            response: None,
            error: Some(msg.to_string()),
            call_count: AtomicU64::new(0),
        }
    }

    fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl EngineRouterTrait for MockEngineRouter {
    async fn route(
        &self,
        _request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        if let Some(resp) = &self.response {
            Ok(resp.clone())
        } else {
            Err(EngineError::RequestFailed(
                self.error.clone().unwrap_or_default(),
            ))
        }
    }

    async fn aggregate(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self.route(request).await
    }

    fn get_engine_stats(&self) -> HashMap<String, EngineStats> {
        HashMap::new()
    }

    fn reset_engine_stats(&self, _engine_name: &str) {}

    fn registered_engines(&self) -> Vec<String> {
        vec!["mock".to_string()]
    }
}

/// Build a RobotsChecker backed by a mock engine router, with short
/// retry delays so tests run fast.
fn make_checker_with_mock_router(router: Arc<dyn EngineRouterTrait>) -> RobotsChecker {
    make_fail_closed_checker_with_mock_router(router, false)
}

/// Same as [`make_checker_with_mock_router`], but with an explicit
/// `fail_closed` flag for fail-closed behavior tests.
fn make_fail_closed_checker_with_mock_router(
    router: Arc<dyn EngineRouterTrait>,
    fail_closed: bool,
) -> RobotsChecker {
    let engine_client = Arc::new(EngineClient::with_router(router));
    RobotsChecker {
        engine_client,
        user_agent: "crawlrs-bot/1.0".to_string(),
        memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
        cache_service: None,

        cache_ttl: Duration::from_secs(3600),
        retry_policy: RetryPolicy {
            max_retries: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(10),
            ..Default::default()
        },
        fail_closed,
        cache_stats: Arc::new(CacheStats::default()),
    }
}
