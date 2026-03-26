// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置默认值模块
//!
//! 提供内联默认值，用于 ConfigBuilder 加载配置
//! 这些值在配置文件和环境变量都未提供时生效

use confers::ConfigValue;
use std::collections::HashMap;

/// 获取完整的内联默认值映射
///
/// 用于 ConfigBuilder 的 defaults() 方法
/// 当配置文件和环境变量都未提供时使用这些默认值
///
/// # 层级优先级（从高到低）
/// 1. 环境变量 (CRAWLRS__*)
/// 2. 配置文件 (config/default.toml)
/// 3. 代码默认值 (本函数返回值)
pub fn get_inline_defaults() -> HashMap<String, ConfigValue> {
    let mut defaults = HashMap::new();

    // =====================================================================
    // Server defaults
    // =====================================================================
    defaults.insert("server.host".into(), ConfigValue::String("0.0.0.0".into()));
    defaults.insert("server.port".into(), ConfigValue::U64(8899));
    defaults.insert("server.enable_port_detection".into(), ConfigValue::Bool(true));

    // =====================================================================
    // CORS defaults
    // =====================================================================
    defaults.insert("cors.allowed_origins".into(), ConfigValue::String("*".into()));

    // =====================================================================
    // Rate limiting defaults
    // =====================================================================
    defaults.insert("rate_limiting.enabled".into(), ConfigValue::Bool(true));
    defaults.insert("rate_limiting.default_rpm".into(), ConfigValue::U64(60));
    defaults.insert("rate_limiting.default_limit".into(), ConfigValue::U64(60));
    defaults.insert("rate_limiting.burst_size".into(), ConfigValue::U64(20));

    // =====================================================================
    // Concurrency defaults
    // =====================================================================
    defaults.insert("concurrency.default_team_limit".into(), ConfigValue::I64(10));
    defaults.insert("concurrency.task_lock_duration_seconds".into(), ConfigValue::I64(300));

    // =====================================================================
    // Cache defaults
    // =====================================================================
    defaults.insert("cache.enabled".into(), ConfigValue::Bool(true));
    defaults.insert("cache.memory.capacity".into(), ConfigValue::U64(10000));
    defaults.insert("cache.memory.ttl_seconds".into(), ConfigValue::U64(300));
    defaults.insert("cache.redis.enabled".into(), ConfigValue::Bool(false));
    defaults.insert("cache.redis.url".into(), ConfigValue::String("redis://localhost:6379".into()));
    defaults.insert("cache.redis.pool_size".into(), ConfigValue::U64(10));
    defaults.insert("cache.redis.ttl_seconds".into(), ConfigValue::U64(3600));
    defaults.insert("cache.types.search.ttl_seconds".into(), ConfigValue::U64(300));
    defaults.insert("cache.types.search.max_size".into(), ConfigValue::U64(10000));
    defaults.insert("cache.types.dns.ttl_seconds".into(), ConfigValue::U64(3600));
    defaults.insert("cache.types.dns.max_size".into(), ConfigValue::U64(1000));
    defaults.insert("cache.types.regex.ttl_seconds".into(), ConfigValue::U64(86400));
    defaults.insert("cache.types.regex.max_size".into(), ConfigValue::U64(5000));

    // =====================================================================
    // Storage defaults
    // =====================================================================
    defaults.insert("storage.storage_type".into(), ConfigValue::String("local".into()));
    defaults.insert("storage.local_path".into(), ConfigValue::String("./storage".into()));

    // =====================================================================
    // Webhook defaults
    // =====================================================================
    defaults.insert("webhook.max_retries".into(), ConfigValue::U64(5));
    defaults.insert("webhook.batch_size".into(), ConfigValue::U64(1000));

    // =====================================================================
    // Search defaults
    // =====================================================================
    defaults.insert("search.ab_test_enabled".into(), ConfigValue::Bool(false));
    defaults.insert("search.variant_b_weight".into(), ConfigValue::F64(0.1));
    defaults.insert("search.timeout_seconds".into(), ConfigValue::U64(30));
    defaults.insert("search.rate_limiting_enabled".into(), ConfigValue::Bool(true));
    defaults.insert("search.test_data_enabled".into(), ConfigValue::Bool(false));
    defaults.insert("search.max_retries".into(), ConfigValue::U64(3));
    defaults.insert("search.retry_delay_ms".into(), ConfigValue::U64(1000));
    defaults.insert("search.default_engine".into(), ConfigValue::String("baidu".into()));
    defaults.insert("search.engines.google_enabled".into(), ConfigValue::Bool(true));
    defaults.insert("search.engines.bing_enabled".into(), ConfigValue::Bool(true));
    defaults.insert("search.engines.baidu_enabled".into(), ConfigValue::Bool(true));
    defaults.insert("search.engines.sogou_enabled".into(), ConfigValue::Bool(true));

    // =====================================================================
    // Bing Search defaults
    // =====================================================================
    defaults.insert("bing_search.enabled".into(), ConfigValue::Bool(true));
    defaults.insert("bing_search.api_key".into(), ConfigValue::String(String::new()));
    defaults.insert("bing_search.endpoint".into(), ConfigValue::String("https://api.bing.microsoft.com/v7.0/search".into()));
    defaults.insert("bing_search.rate_limit_rpm".into(), ConfigValue::U64(30));

    // =====================================================================
    // Engine defaults
    // =====================================================================
    defaults.insert("engines.flaresolverr.enabled".into(), ConfigValue::Bool(true));
    defaults.insert("engines.flaresolverr.url".into(), ConfigValue::String("http://localhost:8191/v1".into()));
    defaults.insert("engines.flaresolverr.timeout_seconds".into(), ConfigValue::U64(30));
    defaults.insert("engines.flaresolverr.max_retries".into(), ConfigValue::U64(3));
    defaults.insert("engines.fire_cdp.enabled".into(), ConfigValue::Bool(false));
    defaults.insert("engines.fire_cdp.url".into(), ConfigValue::String("http://localhost:8191/v1".into()));
    defaults.insert("engines.fire_tls.enabled".into(), ConfigValue::Bool(false));
    defaults.insert("engines.fire_tls.url".into(), ConfigValue::String("http://localhost:8191/v1".into()));

    // =====================================================================
    // Proxy defaults
    // =====================================================================
    defaults.insert("proxy.url".into(), ConfigValue::String("http://localhost:10808".into()));
    defaults.insert("proxy.enabled".into(), ConfigValue::Bool(false));

    // =====================================================================
    // LLM defaults
    // =====================================================================
    defaults.insert("llm.provider".into(), ConfigValue::Null);
    defaults.insert("llm.model".into(), ConfigValue::String("qwen3:1.7b".into()));
    defaults.insert("llm.api_base_url".into(), ConfigValue::String("http://localhost:11434/v1".into()));

    // =====================================================================
    // Worker defaults
    // =====================================================================
    defaults.insert("workers.count".into(), ConfigValue::String("auto".into()));

    // =====================================================================
    // Timeout defaults
    // =====================================================================
    defaults.insert("timeouts.workers.webhook_interval_seconds".into(), ConfigValue::U64(5));
    defaults.insert("timeouts.workers.backlog_interval_seconds".into(), ConfigValue::U64(30));
    defaults.insert("timeouts.engines.default_timeout_seconds".into(), ConfigValue::U64(30));
    defaults.insert("timeouts.engines.playwright_timeout_seconds".into(), ConfigValue::U64(30));
    defaults.insert("timeouts.engines.flaresolverr_timeout_seconds".into(), ConfigValue::U64(30));
    defaults.insert("timeouts.retry.initial_backoff_seconds".into(), ConfigValue::U64(1));
    defaults.insert("timeouts.retry.max_backoff_seconds".into(), ConfigValue::U64(60));
    defaults.insert("timeouts.cache.default_ttl_seconds".into(), ConfigValue::U64(300));
    defaults.insert("timeouts.cache.memory_ttl_seconds".into(), ConfigValue::U64(300));
    defaults.insert("timeouts.cache.redis_ttl_seconds".into(), ConfigValue::U64(300));

    // =====================================================================
    // Logging defaults
    // =====================================================================
    defaults.insert("logging.console.enabled".into(), ConfigValue::Bool(true));
    defaults.insert("logging.file.enabled".into(), ConfigValue::Bool(false));
    defaults.insert("logging.file.path".into(), ConfigValue::String("logs/crawlrs.log".into()));
    defaults.insert("logging.file.max_file_size_mb".into(), ConfigValue::U64(100));
    defaults.insert("logging.file.file_count".into(), ConfigValue::U64(10));

    // =====================================================================
    // Trusted proxies defaults
    // =====================================================================
    defaults.insert("trusted_proxies.enabled".into(), ConfigValue::Bool(true));
    // Note: proxies array is defined in config/default.toml

    defaults
}
