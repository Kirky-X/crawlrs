use super::*;

// ========== Cache service path tests (lines 184, 187-200, 179) ==========

#[tokio::test]
async fn test_cache_service_hit_returns_content_without_http() {
    // When memory cache misses but cache_service has the content,
    // the content should be returned without making an HTTP request.
    let robots_url = "http://8.8.8.8:80/robots.txt";
    let cache_key = format!("robots_cache:{}", robots_url);
    let mock_cache = Arc::new(MockCacheService::with_entry(
        &cache_key,
        "User-agent: *\nDisallow: /private\n",
    ));
    let checker = RobotsChecker {
        engine_client: Arc::new(EngineClient::new()),
        user_agent: "crawlrs-bot/1.0".to_string(),
        memory_cache: Arc::new(Mutex::new(HashMap::new())),
        cache_service: Some(mock_cache as Arc<dyn CacheService>),

        cache_ttl: Duration::from_secs(3600),
        retry_policy: RetryPolicy::default(),
        fail_closed: false,
        cache_stats: Arc::new(CacheStats::default()),
    };

    let allowed = checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("should succeed via cache_service");
    assert!(allowed, "non-disallowed path should be allowed");

    // cache_service hit should be recorded
    let (hits, misses) = checker.get_cache_stats();
    assert_eq!(hits, 1, "cache_service hit should increment hits");
    assert_eq!(misses, 1, "memory cache miss should increment misses");

    // Memory cache should be updated from cache_service content
    let mem_cache = checker.memory_cache.lock().await;
    let entry = mem_cache
        .get(robots_url)
        .expect("memory cache should be populated from cache_service");
    assert!(entry.content.contains("Disallow: /private"));
    assert!(entry.expires_at > Instant::now());
}

#[tokio::test]
async fn test_expired_memory_cache_removed_then_cache_service_hit() {
    // An expired memory cache entry should be removed (line 179),
    // then cache_service is consulted (lines 184, 187-200).
    let robots_url = "http://8.8.8.8:80/robots.txt";
    let cache_key = format!("robots_cache:{}", robots_url);
    let mock_cache = Arc::new(MockCacheService::with_entry(
        &cache_key,
        "User-agent: *\nDisallow: /\n",
    ));
    let checker = RobotsChecker {
        engine_client: Arc::new(EngineClient::new()),
        user_agent: "crawlrs-bot/1.0".to_string(),
        memory_cache: Arc::new(Mutex::new(HashMap::new())),
        cache_service: Some(mock_cache as Arc<dyn CacheService>),

        cache_ttl: Duration::from_secs(3600),
        retry_policy: RetryPolicy::default(),
        fail_closed: false,
        cache_stats: Arc::new(CacheStats::default()),
    };

    // Pre-populate memory cache with an EXPIRED entry.
    {
        let mut cache = checker.memory_cache.lock().await;
        cache.insert(
            robots_url.to_string(),
            CachedRobots {
                content: "old-expired-content".to_string(),
                expires_at: Instant::now() - Duration::from_secs(1),
                fetch_failed: false,
            },
        );
    }

    let allowed = checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("should succeed via cache_service after expired memory cache");
    assert!(!allowed, "Disallow: / should block all paths");

    // Memory cache should now hold the fresh content from cache_service.
    let mem_cache = checker.memory_cache.lock().await;
    let entry = mem_cache
        .get(robots_url)
        .expect("memory cache should have fresh entry");
    assert_eq!(entry.content, "User-agent: *\nDisallow: /\n");
    assert!(
        entry.expires_at > Instant::now(),
        "new entry should not be expired"
    );
}

// ========== HTTP fetch path tests (lines 205-279) ==========
//
// These tests use a mock EngineRouter so no real HTTP request is made.
// The target host is 8.8.8.8 (a public IP) so SSRF validation passes
// without DNS resolution.

#[tokio::test]
async fn test_http_fetch_success_returns_content() {
    // 200 response → content stored and returned.
    let router = Arc::new(MockEngineRouter::with_response(
        200,
        "User-agent: *\nDisallow: /private\n",
    ));
    let checker = make_checker_with_mock_router(router);

    let allowed = checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("should succeed via HTTP fetch");
    assert!(allowed, "non-disallowed path should be allowed");

    // Memory cache should be updated after fetch.
    let mem_cache = checker.memory_cache.lock().await;
    let entry = mem_cache
        .get("http://8.8.8.8:80/robots.txt")
        .expect("memory cache should be updated after HTTP fetch");
    assert!(entry.content.contains("Disallow: /private"));
}

#[tokio::test]
async fn test_http_fetch_404_returns_empty_content() {
    // 404 → empty content → allow all.
    let router = Arc::new(MockEngineRouter::with_response(404, "Not Found"));
    let checker = make_checker_with_mock_router(router);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("should succeed with empty content on 404");
    assert!(allowed, "404 should default to allowing all paths");

    let mem_cache = checker.memory_cache.lock().await;
    let entry = mem_cache
        .get("http://8.8.8.8:80/robots.txt")
        .expect("memory cache should have empty content");
    assert_eq!(entry.content, "", "404 should store empty content");
}

#[tokio::test]
async fn test_http_fetch_403_returns_empty_content() {
    // Other non-500 status (e.g. 403) → empty content → allow all.
    let router = Arc::new(MockEngineRouter::with_response(403, "Forbidden"));
    let checker = make_checker_with_mock_router(router);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("should succeed with empty content on 403");
    assert!(allowed, "403 should default to allowing all paths");
}

#[tokio::test]
async fn test_http_fetch_500_retries_then_defaults_empty() {
    // 500 (server error) triggers retries; after all retries the
    // content defaults to empty.
    let router = Arc::new(MockEngineRouter::with_response(
        500,
        "Internal Server Error",
    ));
    let router_ref = router.clone();
    let checker = make_checker_with_mock_router(router);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("should succeed with empty content after retries");
    assert!(
        allowed,
        "server errors should default to allowing all paths"
    );

    // Should have retried max_retries (3) times.
    assert_eq!(
        router_ref.call_count(),
        3,
        "should retry 3 times on 500 errors"
    );

    let mem_cache = checker.memory_cache.lock().await;
    let entry = mem_cache
        .get("http://8.8.8.8:80/robots.txt")
        .expect("memory cache should have entry");
    assert_eq!(
        entry.content, "",
        "content should be empty after failed retries"
    );
}

#[tokio::test]
async fn test_http_fetch_request_error_defaults_empty() {
    // Request failure (e.g. connection refused) triggers retries;
    // after all retries the content defaults to empty.
    let router = Arc::new(MockEngineRouter::with_error("connection refused"));
    let router_ref = router.clone();
    let checker = make_checker_with_mock_router(router);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("should succeed with empty content after request errors");
    assert!(
        allowed,
        "request errors should default to allowing all paths"
    );

    assert_eq!(
        router_ref.call_count(),
        3,
        "should retry 3 times on request errors"
    );
}

#[tokio::test]
async fn test_http_fetch_updates_cache_service() {
    // After a successful HTTP fetch, cache_service.set should be called
    // to persist the content.
    let router = Arc::new(MockEngineRouter::with_response(
        200,
        "User-agent: *\nDisallow: /private\n",
    ));
    let mock_cache = Arc::new(MockCacheService::new());
    let checker = RobotsChecker {
        engine_client: Arc::new(EngineClient::with_router(router)),
        user_agent: "crawlrs-bot/1.0".to_string(),
        memory_cache: Arc::new(Mutex::new(HashMap::new())),
        cache_service: Some(mock_cache.clone() as Arc<dyn CacheService>),
        cache_ttl: Duration::from_secs(3600),
        retry_policy: RetryPolicy {
            max_retries: 3,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(10),
            ..Default::default()
        },
        fail_closed: false,
        cache_stats: Arc::new(CacheStats::default()),
    };

    checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("should succeed");

    assert_eq!(
        mock_cache.set_count(),
        1,
        "cache_service.set should be called once after HTTP fetch"
    );
}

#[tokio::test]
async fn test_full_flow_first_fetch_then_cache_hit() {
    // First call: cache miss → HTTP fetch → memory cache populated.
    // Second call: memory cache hit → no HTTP fetch needed.
    let router = Arc::new(MockEngineRouter::with_response(
        200,
        "User-agent: *\nDisallow: /private\n",
    ));
    let router_ref = router.clone();
    let checker = make_checker_with_mock_router(router);

    // First call — should fetch via HTTP.
    let allowed1 = checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("first call should succeed");
    assert!(allowed1, "first call: non-disallowed path allowed");
    assert_eq!(
        router_ref.call_count(),
        1,
        "first call should trigger HTTP fetch"
    );

    // Second call — should hit memory cache (no HTTP fetch).
    let allowed2 = checker
        .is_allowed("http://8.8.8.8/private", "MyBot")
        .await
        .expect("second call should succeed");
    assert!(!allowed2, "second call: disallowed path blocked");
    assert_eq!(
        router_ref.call_count(),
        1,
        "second call should NOT trigger HTTP fetch (cache hit)"
    );

    let (hits, misses) = checker.get_cache_stats();
    assert_eq!(hits, 1, "one cache hit (second call)");
    assert_eq!(misses, 1, "one cache miss (first call)");
}
