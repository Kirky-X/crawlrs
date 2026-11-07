use super::*;

#[tokio::test]
async fn test_is_allowed_with_cached_robots_allows_non_disallowed_path() {
    let checker = make_checker();
    let content = "User-agent: *\nDisallow: /private\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    let allowed = checker
        .is_allowed("https://example.com/page", "MyBot/1.0")
        .await
        .expect("should succeed with cached content");
    assert!(allowed, "non-disallowed path should be allowed");
}

#[tokio::test]
async fn test_is_allowed_with_cached_robots_blocks_disallowed_path() {
    let checker = make_checker();
    let content = "User-agent: *\nDisallow: /private\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    let allowed = checker
        .is_allowed("https://example.com/private", "MyBot/1.0")
        .await
        .expect("should succeed with cached content");
    assert!(!allowed, "disallowed path should be blocked");
}

#[tokio::test]
async fn test_is_allowed_with_cached_robots_blocks_disallowed_subpath() {
    let checker = make_checker();
    let content = "User-agent: *\nDisallow: /private\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    let allowed = checker
        .is_allowed("https://example.com/private/secret", "MyBot/1.0")
        .await
        .expect("should succeed with cached content");
    assert!(
        !allowed,
        "subpath of disallowed path should also be blocked"
    );
}

#[tokio::test]
async fn test_is_allowed_with_cached_empty_robots_allows_all() {
    let checker = make_checker();
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", "").await;

    let allowed = checker
        .is_allowed("https://example.com/anything", "MyBot/1.0")
        .await
        .expect("should succeed with empty robots.txt");
    assert!(allowed, "empty robots.txt should allow all paths");
}

#[tokio::test]
async fn test_is_allowed_with_cached_robots_specific_agent_block() {
    let checker = make_checker();
    // Use a specific user agent (without version suffix) that matches
    // the robots.txt User-agent directive. The robotstxt crate's
    // extract_user_agent extracts [a-zA-Z_-] characters from the
    // robots.txt directive, then compares with the caller's user_agent
    // using eq_ignore_ascii_case. So the caller must pass the product
    // name only (e.g. "BadBot", not "BadBot/1.0").
    let content = "User-agent: BadBot\nDisallow: /\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    // BadBot should be blocked everywhere by Disallow: /
    let bad_allowed = checker
        .is_allowed("https://example.com/page", "BadBot")
        .await
        .expect("should succeed");
    assert!(!bad_allowed, "BadBot should be blocked on all paths");

    // With only a BadBot-specific group and no * group, other bots
    // should be allowed (no applicable rules).
    let other_content = "User-agent: BadBot\nDisallow: /\nUser-agent: *\nAllow: /\n";
    populate_robots_cache(
        &checker,
        "https://example.com:443/robots.txt",
        other_content,
    )
    .await;
    let good_allowed = checker
        .is_allowed("https://example.com/page", "GoodBot")
        .await
        .expect("should succeed");
    assert!(good_allowed, "GoodBot should be allowed via * group");
}

#[tokio::test]
async fn test_get_crawl_delay_with_cached_robots() {
    let checker = make_checker();
    let content = "User-agent: *\nCrawl-delay: 5\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    let delay = checker
        .get_crawl_delay("https://example.com/page", "MyBot/1.0")
        .await
        .expect("should succeed with cached content");
    assert_eq!(
        delay,
        Some(Duration::from_secs(5)),
        "should return the cached crawl delay"
    );
}

#[tokio::test]
async fn test_get_crawl_delay_with_cached_no_delay_directive() {
    let checker = make_checker();
    let content = "User-agent: *\nDisallow: /private\n";
    populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

    let delay = checker
        .get_crawl_delay("https://example.com/page", "MyBot/1.0")
        .await
        .expect("should succeed");
    assert_eq!(
        delay, None,
        "should return None when no Crawl-delay directive"
    );
}

// ========== Cache hit statistics tests ==========

#[tokio::test]
async fn test_is_allowed_cache_hit_increments_stats() {
    let http_client = Arc::new(reqwest::Client::new());
    let stats = Arc::new(CacheStats::default());
    let checker = RobotsChecker::new(
        http_client,
        &crate::config::settings::RobotsSettings::default(),
        None,
        Some(stats.clone()),
    );

    populate_robots_cache(
        &checker,
        "https://example.com:443/robots.txt",
        "User-agent: *\nDisallow: /private\n",
    )
    .await;

    let (hits_before, misses_before) = checker.get_cache_stats();
    assert_eq!(hits_before, 0);
    assert_eq!(misses_before, 0);

    checker
        .is_allowed("https://example.com/page", "MyBot")
        .await
        .expect("should succeed");

    let (hits_after, misses_after) = checker.get_cache_stats();
    assert_eq!(hits_after, 1, "cache hit should increment hits");
    assert_eq!(misses_after, 0, "no misses should occur for cache hit");
}

#[tokio::test]
async fn test_get_crawl_delay_cache_hit_increments_stats() {
    let http_client = Arc::new(reqwest::Client::new());
    let stats = Arc::new(CacheStats::default());
    let checker = RobotsChecker::new(
        http_client,
        &crate::config::settings::RobotsSettings::default(),
        None,
        Some(stats.clone()),
    );

    populate_robots_cache(
        &checker,
        "https://example.com:443/robots.txt",
        "User-agent: *\nCrawl-delay: 3\n",
    )
    .await;

    checker
        .get_crawl_delay("https://example.com/page", "MyBot")
        .await
        .expect("should succeed");

    let (hits, misses) = checker.get_cache_stats();
    assert_eq!(hits, 1, "cache hit should be recorded");
    assert_eq!(misses, 0);
}

// ========== Error path tests ==========

#[tokio::test]
async fn test_is_allowed_invalid_url_returns_error() {
    let checker = make_checker();
    let result = checker.is_allowed("not-a-valid-url", "MyBot").await;
    assert!(result.is_err(), "invalid URL should return an error");
}

#[tokio::test]
async fn test_is_allowed_url_without_host_returns_error() {
    let checker = make_checker();
    let result = checker.is_allowed("file:///etc/passwd", "MyBot").await;
    assert!(result.is_err(), "URL without host should return an error");
}

#[tokio::test]
async fn test_get_crawl_delay_invalid_url_returns_error() {
    let checker = make_checker();
    let result = checker.get_crawl_delay("not-a-valid-url", "MyBot").await;
    assert!(result.is_err(), "invalid URL should return an error");
}

#[tokio::test]
async fn test_get_crawl_delay_url_without_host_returns_error() {
    let checker = make_checker();
    let result = checker.get_crawl_delay("file:///etc/passwd", "MyBot").await;
    assert!(result.is_err(), "URL without host should return an error");
}

// ========== create_engine_client test ==========

#[test]
fn test_create_engine_client_creates_valid_client() {
    let http_client = Arc::new(reqwest::Client::new());
    let engine_client = RobotsChecker::create_engine_client(http_client);
    assert_eq!(
        engine_client.engine_count(),
        1,
        "exactly one engine (reqwest) should be registered"
    );
    let names = engine_client.registered_engines();
    assert!(
        names.iter().any(|n| n == "reqwest"),
        "registered engines should contain 'reqwest', got {:?}",
        names
    );
}
