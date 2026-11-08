use super::*;

// ========== SSRF failure path test (line 205) ==========

#[tokio::test]
async fn test_get_robots_content_ssrf_blocked_returns_error() {
    // A localhost URL should be blocked by SSRF validation (line 205),
    // returning an error before any HTTP fetch is attempted.
    let router = Arc::new(MockFetcher::with_response(200, ""));
    let router_ref = router.clone();
    let checker = make_checker_with_mock_fetcher(router);

    let result = checker.is_allowed("http://localhost/page", "MyBot").await;
    assert!(result.is_err(), "localhost URL should be blocked by SSRF");

    // The mock engine should never have been called.
    assert_eq!(
        router_ref.call_count(),
        0,
        "no HTTP fetch should occur when SSRF blocks the URL"
    );
}

// ==== fail_closed：robots.txt 获取失败（重试耗尽）的兜底策略 ====

#[tokio::test]
async fn test_fetch_failure_fail_closed_blocks_and_caches_marker() {
    // fail_closed=true：获取失败判禁；失败标记按短 TTL（60s）写入内存缓存，
    // 后续同 host 请求命中标记，不再发起 HTTP 请求（防打爆）。
    let router = Arc::new(MockFetcher::with_error("connection refused"));
    let router_ref = router.clone();
    let checker = make_fail_closed_checker_with_mock_fetcher(router, true);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("fail-closed 判禁应返回 Ok(false) 而非 Err");
    assert!(!allowed, "fail_closed=true 时获取失败应判禁");
    assert_eq!(router_ref.call_count(), 3, "应重试至 max_retries 耗尽");

    // 失败标记入缓存，TTL 为短 TTL（60s）而非正常 cache_ttl
    {
        let cache = checker.memory_cache.lock().await;
        let entry = cache
            .get("http://8.8.8.8:80/robots.txt")
            .expect("失败标记应写入内存缓存");
        assert!(entry.fetch_failed, "缓存条目应携带失败标记");
        assert_eq!(entry.content, "", "失败标记条目内容为空");
        assert!(
            entry.expires_at <= Instant::now() + ROBOTS_FAILURE_CACHE_TTL,
            "失败标记应使用短 TTL（60s）"
        );
        assert!(entry.expires_at > Instant::now());
    }

    // 第二次调用命中失败标记：不再发起网络请求，仍判禁
    let allowed2 = checker
        .is_allowed("http://8.8.8.8/other", "MyBot")
        .await
        .expect("命中失败标记应返回 Ok(false)");
    assert!(!allowed2, "命中失败标记仍应判禁");
    assert_eq!(
        router_ref.call_count(),
        3,
        "命中失败标记不应再次发起 HTTP 请求（防打爆）"
    );
}

#[tokio::test]
async fn test_fetch_failure_fail_closed_false_keeps_fail_open() {
    // fail_closed=false（默认）：获取失败维持现行 fail-open 放行，
    // 缓存写入的是普通空内容条目（非失败标记）。
    let router = Arc::new(MockFetcher::with_error("connection refused"));
    let router_ref = router.clone();
    let checker = make_fail_closed_checker_with_mock_fetcher(router, false);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("fail-open 路径应成功返回");
    assert!(allowed, "fail_closed=false（默认）时获取失败应放行");
    assert_eq!(router_ref.call_count(), 3);

    let cache = checker.memory_cache.lock().await;
    let entry = cache
        .get("http://8.8.8.8:80/robots.txt")
        .expect("失败后仍应缓存空内容");
    assert!(!entry.fetch_failed, "fail-open 失败缓存的是普通空内容条目");
    assert!(
        entry.expires_at > Instant::now() + ROBOTS_FAILURE_CACHE_TTL,
        "fail-open 失败条目使用正常 cache_ttl 而非短 TTL"
    );
}

#[tokio::test]
async fn test_fetch_500_exhausted_fail_closed_blocks() {
    // 5xx 重试耗尽 + fail_closed=true：判禁（与请求错误同路径）
    let router = Arc::new(MockFetcher::with_response(500, "Internal Server Error"));
    let checker = make_fail_closed_checker_with_mock_fetcher(router, true);

    let allowed = checker
        .is_allowed("http://8.8.8.8/anything", "MyBot")
        .await
        .expect("fail-closed 判禁应返回 Ok(false)");
    assert!(!allowed, "5xx 重试耗尽且 fail_closed=true 应判禁");
}

#[tokio::test]
async fn test_fail_closed_success_path_unaffected() {
    // fail_closed=true 且 robots.txt 获取成功：按规则正常判定，不受失败标记影响
    let router = Arc::new(MockFetcher::with_response(
        200,
        "User-agent: *\nDisallow: /private\n",
    ));
    let checker = make_fail_closed_checker_with_mock_fetcher(router, true);

    let allowed = checker
        .is_allowed("http://8.8.8.8/page", "MyBot")
        .await
        .expect("成功获取后应正常判定");
    assert!(allowed, "未命中 Disallow 的路径应放行");

    let blocked = checker
        .is_allowed("http://8.8.8.8/private", "MyBot")
        .await
        .expect("成功获取后应正常判定");
    assert!(!blocked, "命中 Disallow 的路径应被拦下");
}

// ==== r2 F6：匹配规则携带 query ====

#[test]
fn test_query_disallow_rule_requires_query_in_target() {
    // `Disallow: /*?s=` 以 query 为匹配特征——只传 path 时该规则永不命中
    let content = "User-agent: *\nDisallow: /*?s=";
    let mut matcher = DefaultMatcher::default();
    assert!(
        !matcher.one_agent_allowed_by_robots(content, "crawlrs", "/page?s=a"),
        "query-bearing URL must be disallowed by /*?s= rule"
    );
    assert!(
        matcher.one_agent_allowed_by_robots(content, "crawlrs", "/page"),
        "clean path must be allowed"
    );
}
