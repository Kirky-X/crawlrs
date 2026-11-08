use super::*;

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

/// 测试抓取器：按状态码语义产生 [`RobotsFetchAttempt`]，并计数调用次数。
struct MockFetcher {
    content: Option<String>,
    error: Option<String>,
    call_count: AtomicU64,
}

impl MockFetcher {
    /// 沿用 HTTP 状态码语义：2xx→内容；404/其余非 5xx→空内容；
    /// 5xx→可重试错误。生产实现的同一语义在 `HttpRobotsFetcher`。
    fn with_response(status_code: u16, content: &str) -> Self {
        let content = if (200..300).contains(&status_code) {
            content.to_string()
        } else if status_code >= 500 {
            return Self::with_error(&format!("Server error: {status_code}"));
        } else {
            String::new()
        };
        Self {
            content: Some(content),
            error: None,
            call_count: AtomicU64::new(0),
        }
    }

    fn with_error(msg: &str) -> Self {
        Self {
            content: None,
            error: Some(msg.to_string()),
            call_count: AtomicU64::new(0),
        }
    }

    fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl RobotsFetcher for MockFetcher {
    async fn fetch(&self, _robots_url: &str, _user_agent: &str) -> RobotsFetchAttempt {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        if let Some(content) = &self.content {
            RobotsFetchAttempt::Content(content.clone())
        } else {
            RobotsFetchAttempt::Retryable(anyhow::anyhow!(self.error.clone().unwrap_or_default()))
        }
    }
}

/// Build a RobotsChecker backed by a mock fetcher, with short
/// retry delays so tests run fast.
fn make_checker_with_mock_fetcher(fetcher: Arc<MockFetcher>) -> RobotsChecker {
    make_fail_closed_checker_with_mock_fetcher(fetcher, false)
}

/// Same as [`make_checker_with_mock_fetcher`], but with an explicit
/// `fail_closed` flag for fail-closed behavior tests.
fn make_fail_closed_checker_with_mock_fetcher(
    fetcher: Arc<MockFetcher>,
    fail_closed: bool,
) -> RobotsChecker {
    RobotsChecker {
        fetcher,
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
