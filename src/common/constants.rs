// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 应用程序常量定义
//!
//! 将魔法数字定义为有意义的常量，提高代码可读性和可维护性。
//!
//! **注意**：可配置的值应优先通过 `config/default.toml` + confers 配置系统管理，
//! 此处仅保留不适合外部配置的内部常量（如分页限制、Credits 成本、测试超时等）。

/// 缓存配置常量
///
/// 注意：部分值与 `config/default.toml` 中的 `[cache]` 段对应，
/// 修改时需同步更新配置文件。
pub mod cache_config {
    pub const DEFAULT_TTL_SECS: u64 = 300; // 5分钟
    pub const ROBOTS_TTL_SECS: u64 = 3600; // 1小时
    pub const MAX_CACHE_ENTRIES: usize = 10000;
    pub const MEMORY_CACHE_MAX_SIZE: usize = 1000;
    pub const EVICTION_BUFFER_PERCENT: usize = 10;
}

/// 默认身份常量 - 用于 `auth` feature 关闭时的单租户降级
///
/// **阶段说明**：Stage 0 仅定义常量；Stage 1 (T007-T009) 实现 `default_identity_middleware`
/// 并加 `#[cfg(not(feature = "auth"))]` 门控；Stage 3 (T017-T022) 对 limiteron 路径加 cfg 门控。
/// 在所有门控就位前，`--no-default-features` 构建会失败（预期行为，Stage 5 统一验证）。
///
/// 命名使用 `default_identity` 而非 `auth`，避免与 `domain::auth`（ApiKeyScope 等领域模型）同名歧义。
pub mod default_identity {
    use uuid::Uuid;

    /// 单租户模式下的默认团队 ID（非 nil，固定值 1）
    pub const DEFAULT_TEAM_ID: Uuid = Uuid::from_u128(1);

    /// 单租户模式下的默认 API Key ID（非 nil，固定值 2，与 DEFAULT_TEAM_ID 区分）
    pub const DEFAULT_API_KEY_ID: Uuid = Uuid::from_u128(2);

    /// 单租户降级模式下注入的 `token_hash` 占位字符串。
    ///
    /// 真实 `auth_middleware` 注入 `"sha256:<hex>"` 格式的 `token_hash`；
    /// 单租户模式下没有 token，使用固定字符串 `"sha256:default_identity"` 作为占位。
    ///
    /// 格式与真实 `token_hash` 对齐（`sha256:` 前缀），确保下游消费者
    /// 不会因格式差异产生异常行为（如 rate-limit bucket key 分裂）。
    ///
    /// 迁移路径：若未来需要区分单租户与多租户的 token hash，
    /// 可改用 `sha256:single_tenant_<team_id>` 格式。
    pub const DEFAULT_IDENTITY_TOKEN_HASH: &str = "sha256:default_identity";
}

/// 服务器配置常量
///
/// 注意：`DEFAULT_HOST`/`DEFAULT_PORT` 等值与 `config/default.toml` 对应，
/// 修改时需同步更新配置文件。
pub mod server_config {
    pub const DEFAULT_HOST: &str = "0.0.0.0";
    pub const DEFAULT_PORT: u16 = 8899;
    pub const DEFAULT_RATE_LIMIT_RPM: u32 = 100;
    pub const DEFAULT_TEAM_LIMIT: u32 = 10;
    pub const DEFAULT_TASK_LOCK_DURATION_SECS: u64 = 300;

    /// 默认分页限制
    pub const DEFAULT_PAGE_LIMIT: u32 = 100;
    /// 最大分页限制
    pub const MAX_PAGE_LIMIT: u32 = 1000;

    /// CORS 缓存时间（秒）
    pub const CORS_MAX_AGE_SECS: u64 = 86400; // 24小时
}

/// 爬虫任务常量 - 避免handler中的硬编码值
pub mod crawl_task {
    pub const CRAWL_TASK_CREDITS_COST: i64 = 10;
    pub const SCRAPE_TASK_CREDITS_COST: i64 = 5;
    pub const EXTRACT_TASK_CREDITS_COST: i64 = 8;
    pub const MAX_CONCURRENT_CRAWLS: u32 = 10;
    pub const DEFAULT_MAX_RETRIES: u32 = 3;
    pub const BASE_POLL_INTERVAL_MS: u64 = 1000;
    pub const DEFAULT_TIMEOUT_MS: u64 = 5000;
    pub const MAX_SYNC_WAIT_MS: u32 = 30000;

    /// 最大轮询次数（防止过多数据库查询）
    pub const MAX_POLL_COUNT: u32 = 60;
}

/// 测试相关常量
pub mod testing {
    use std::time::Duration;

    /// API 请求超时时间（10秒）
    pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
    /// 快速测试超时时间（10秒）
    pub const QUICK_TEST_TIMEOUT: Duration = Duration::from_secs(10);
    /// E2E 测试超时时间（90秒）
    pub const E2E_TEST_TIMEOUT: Duration = Duration::from_secs(90);
    /// 爬虫任务超时时间（90秒）
    pub const CRAWL_TASK_TIMEOUT: Duration = Duration::from_secs(90);
}

/// 环境变量名称常量
/// 使用常量定义环境变量名称，避免拼写错误，提高可维护性
pub mod env_vars {
    /// 应用程序环境
    pub const ENV: &str = "CRAWLRS_ENV";
    /// 应用程序环境（备用名称）
    pub const APP_ENVIRONMENT: &str = "APP_ENVIRONMENT";

    // === 速率限制相关 ===
    /// 禁用速率限制
    pub const RATE_LIMITING_ENABLED: &str = "CRAWLRS_RATE_LIMITING_ENABLED";

    // === SSRF 保护相关 ===
    /// 禁用 SSRF 保护
    pub const DISABLE_SSRF_PROTECTION: &str = "CRAWLRS_DISABLE_SSRF_PROTECTION";
    /// 启用网络测试
    pub const ENABLE_NETWORK_TESTS: &str = "CRAWLRS_ENABLE_NETWORK_TESTS";

    // === 代理相关 ===
    /// 代理 URL
    pub const PROXY_URL: &str = "CRAWLRS_PROXY_URL";

    // === 测试相关 ===
    /// 测试模式：不复用浏览器
    pub const TEST_NO_BROWSER_REUSE: &str = "CRAWLRS_TEST_NO_BROWSER_REUSE";

    // === 调试相关 ===
    /// 调试：保存 HTML
    pub const DEBUG_SAVE_HTML: &str = "DEBUG_SAVE_HTML";

    // === 健康检查相关 ===
    /// 健康检查 URL
    pub const HEALTH_CHECK_URL: &str = "CRAWLRS_HEALTH_CHECK_URL";

    // === 搜索引擎测试结果 ===
    /// 百度测试结果
    pub const BAIDU_TEST_RESULTS: &str = "BAIDU_TEST_RESULTS";
    /// 必应测试结果
    pub const BING_TEST_RESULTS: &str = "BING_TEST_RESULTS";
    /// 谷歌 HTTP 回退测试结果
    pub const GOOGLE_HTTP_FALLBACK_TEST_RESULTS: &str = "GOOGLE_HTTP_FALLBACK_TEST_RESULTS";
    /// 搜狗测试结果
    pub const SOGOU_TEST_RESULTS: &str = "SOGOU_TEST_RESULTS";
    /// 使用测试数据
    pub const USE_TEST_DATA: &str = "USE_TEST_DATA";

    // === 跳过测试 ===
    /// 跳过搜索测试
    pub const SKIP_SEARCH_TESTS: &str = "SKIP_SEARCH_TESTS";
    /// 跳过浏览器测试
    pub const SKIP_BROWSER_TESTS: &str = "SKIP_BROWSER_TESTS";

    // === 浏览器远程调试 ===
    /// Chromium 远程调试 URL
    pub const CHROMIUM_REMOTE_DEBUGGING_URL: &str = "CHROMIUM_REMOTE_DEBUGGING_URL";

    // === Fire 引擎相关 ===
    /// Fire 引擎 CDP URL
    pub const FIRE_ENGINE_CDP_URL: &str = "FIRE_ENGINE_CDP_URL";
    /// Fire 引擎 TLS URL
    pub const FIRE_ENGINE_TLS_URL: &str = "FIRE_ENGINE_TLS_URL";
    /// Fire 引擎基础 URL
    pub const FIRE_ENGINE_URL: &str = "FIRE_ENGINE_URL";

    // === FlareSolverr ===
    /// FlareSolverr URL
    pub const FLARESOLVERR_URL: &str = "FLARESOLVERR_URL";

    // === 测试用环境变量 ===
    /// 测试数据库 URL
    pub const TEST_DATABASE_URL: &str = "TEST_DATABASE_URL";
    /// 测试数据库密码
    pub const TEST_DATABASE_PASSWORD: &str = "TEST_DATABASE_PASSWORD";
    /// 测试 Webhook 密钥
    pub const TEST_WEBHOOK_SECRET: &str = "TEST_WEBHOOK_SECRET";
    /// 测试 S3 访问密钥
    pub const TEST_S3_ACCESS_KEY: &str = "TEST_S3_ACCESS_KEY";
    /// 测试 S3 密钥
    pub const TEST_S3_SECRET_KEY: &str = "TEST_S3_SECRET_KEY";
    /// 测试 S3 端点
    pub const TEST_S3_ENDPOINT: &str = "TEST_S3_ENDPOINT";
    /// 跳过 S3 测试
    pub const SKIP_S3_TESTS: &str = "SKIP_S3_TESTS";
    /// 测试 Fire 引擎 CDP URL
    pub const TEST_FIRE_ENGINE_CDP_URL: &str = "TEST_FIRE_ENGINE_CDP_URL";
}

/// 导出测试常量
pub use testing::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_constants() {
        assert_eq!(cache_config::DEFAULT_TTL_SECS, 300);
        assert_eq!(cache_config::MAX_CACHE_ENTRIES, 10000);
    }

    #[test]
    fn test_auth_default_identity_constants() {
        // R-flags-004: DEFAULT_TEAM_ID 与 DEFAULT_API_KEY_ID 必须非 nil 且互不相等
        assert_ne!(
            default_identity::DEFAULT_TEAM_ID,
            uuid::Uuid::nil(),
            "DEFAULT_TEAM_ID must not be nil UUID"
        );
        assert_ne!(
            default_identity::DEFAULT_API_KEY_ID,
            uuid::Uuid::nil(),
            "DEFAULT_API_KEY_ID must not be nil UUID"
        );
        assert_ne!(
            default_identity::DEFAULT_TEAM_ID,
            default_identity::DEFAULT_API_KEY_ID,
            "DEFAULT_TEAM_ID and DEFAULT_API_KEY_ID must be distinct"
        );
    }
}
