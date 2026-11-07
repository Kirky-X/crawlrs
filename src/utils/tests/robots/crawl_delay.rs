use super::*;

#[test]
fn test_checker_new_creates_instance_with_defaults() {
    let checker = make_checker();
    let (hits, misses) = checker.get_cache_stats();
    assert_eq!(hits, 0, "new checker should have 0 cache hits");
    assert_eq!(misses, 0, "new checker should have 0 cache misses");
}

#[test]
fn test_checker_new_with_custom_cache_stats() {
    let http_client = Arc::new(reqwest::Client::new());
    let stats = Arc::new(CacheStats::default());
    stats.record_hit();
    stats.record_miss();

    let checker = RobotsChecker::new(
        http_client,
        &crate::config::settings::RobotsSettings::default(),
        None,
        Some(stats),
    );
    let (hits, misses) = checker.get_cache_stats();
    assert_eq!(hits, 1, "should use the provided cache stats");
    assert_eq!(misses, 1);
}

// parse_crawl_delay tests

#[test]
fn test_parse_crawl_delay_wildcard_agent() {
    let checker = make_checker();
    let content = "User-agent: *\nCrawl-delay: 5\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(5)),
        "wildcard agent should match any user agent"
    );
}

#[test]
fn test_parse_crawl_delay_specific_agent_match() {
    let checker = make_checker();
    let content = "User-agent: crawlrs-bot\nCrawl-delay: 10\n";
    let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(10)),
        "specific agent should match when user_agent contains it"
    );
}

#[test]
fn test_parse_crawl_delay_specific_agent_no_match() {
    let checker = make_checker();
    let content = "User-agent: otherbot\nCrawl-delay: 10\n";
    let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
    assert_eq!(
        delay, None,
        "should return None when the user agent does not match"
    );
}

#[test]
fn test_parse_crawl_delay_no_directive() {
    let checker = make_checker();
    let content = "User-agent: *\nDisallow: /private\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay, None,
        "should return None when no Crawl-delay is present"
    );
}

#[test]
fn test_parse_crawl_delay_empty_content() {
    let checker = make_checker();
    let delay = checker.parse_crawl_delay("", "MyBot/1.0");
    assert_eq!(delay, None, "empty content should return None");
}

#[test]
fn test_parse_crawl_delay_specific_overrides_wildcard() {
    // When a specific agent block appears after the wildcard block,
    // the specific agent's delay should take precedence.
    let checker = make_checker();
    let content = "\
User-agent: *
Crawl-delay: 1
User-agent: crawlrs-bot
Crawl-delay: 30
";
    let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(30)),
        "specific agent delay should override wildcard delay"
    );
}

#[test]
fn test_parse_crawl_delay_wildcard_when_no_specific_match() {
    // When a specific agent block exists but doesn't match,
    // the wildcard block should still apply.
    let checker = make_checker();
    let content = "\
User-agent: *
Crawl-delay: 2
User-agent: otherbot
Crawl-delay: 30
";
    let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(2)),
        "wildcard delay should apply when no specific agent matches"
    );
}

#[test]
fn test_parse_crawl_delay_invalid_value_ignored() {
    let checker = make_checker();
    let content = "User-agent: *\nCrawl-delay: not-a-number\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay, None,
        "invalid Crawl-delay value should be ignored (return None)"
    );
}

#[test]
fn test_parse_crawl_delay_non_finite_or_negative_no_panic() {
    let checker = make_checker();
    for bad in ["inf", "nan", "-5"] {
        let content = format!("User-agent: *\nCrawl-delay: {}\n", bad);
        let delay = checker.parse_crawl_delay(&content, "MyBot/1.0");
        assert_eq!(
            delay, None,
            "Crawl-delay '{}' must be rejected without panic",
            bad
        );
    }
    // 合法小数仍生效
    let ok = checker.parse_crawl_delay("User-agent: *\nCrawl-delay: 1.5\n", "MyBot/1.0");
    assert_eq!(ok, Some(Duration::from_secs_f64(1.5)));
}

#[test]
fn test_parse_crawl_delay_skips_comments_and_empty_lines() {
    let checker = make_checker();
    let content = "\
# This is a comment
User-agent: *

# Another comment
Crawl-delay: 7
";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(7)),
        "comments and empty lines should be skipped"
    );
}

#[test]
fn test_parse_crawl_delay_case_insensitive_user_agent() {
    // The matching logic uses to_lowercase() for comparison.
    let checker = make_checker();
    let content = "User-agent: CrawlRS-Bot\nCrawl-delay: 3\n";
    let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(3)),
        "user agent matching should be case-insensitive"
    );
}

#[test]
fn test_parse_crawl_delay_case_insensitive_directive() {
    // The directive parsing uses to_lowercase() for the directive name.
    let checker = make_checker();
    let content = "user-agent: *\ncrawl-delay: 4\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs(4)),
        "directive names should be case-insensitive"
    );
}

#[test]
fn test_parse_crawl_delay_fractional_seconds() {
    let checker = make_checker();
    let content = "User-agent: *\nCrawl-delay: 0.5\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay,
        Some(Duration::from_secs_f64(0.5)),
        "fractional crawl-delay should be parsed correctly"
    );
}

#[test]
fn test_parse_crawl_delay_multiple_blocks_same_agent() {
    // When the same agent appears in multiple blocks, the last matching
    // Crawl-delay in the current block should be used.
    let checker = make_checker();
    let content = "\
User-agent: *
Crawl-delay: 1
User-agent: *
Crawl-delay: 8
";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    // The second block resets delay to None then sets it to 8.
    assert_eq!(
        delay,
        Some(Duration::from_secs(8)),
        "last matching Crawl-delay should be used"
    );
}

#[test]
fn test_parse_crawl_delay_only_directive_without_agent() {
    // Crawl-delay without a preceding User-agent directive should be ignored
    // because current_agent_matched starts as false.
    let checker = make_checker();
    let content = "Crawl-delay: 5\n";
    let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
    assert_eq!(
        delay, None,
        "Crawl-delay without a matching User-agent block should be ignored"
    );
}

// ========== Cache pre-population tests for is_allowed & get_crawl_delay ==========
