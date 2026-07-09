// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置服务模块
//!
//! 提供统一的配置访问接口，封装环境变量读取。
//! 支持通过 DI 注入，便于测试时模拟不同配置。

use async_trait::async_trait;
use shaku::{Component, Interface};
use std::time::Duration;

/// 配置服务 trait
///
/// 统一管理所有环境变量配置，支持测试时注入 mock 实现。
#[async_trait]
pub trait ConfigServiceTrait: Interface + Send + Sync {
    /// 获取代理 URL
    fn get_proxy_url(&self) -> Option<String>;

    /// 获取远程调试 URL（Chrome）
    fn get_remote_debugging_url(&self) -> Option<String>;

    /// 是否处于测试模式（禁用浏览器复用）
    fn is_test_mode(&self) -> bool;

    /// 获取默认 HTTP 超时时间
    fn get_default_timeout(&self) -> Duration;

    /// 获取浏览器连接超时时间
    fn get_browser_timeout(&self) -> Duration;

    /// 获取浏览器启动超时时间
    fn get_browser_launch_timeout(&self) -> Duration;

    /// 获取应用环境（development/staging/production）
    fn get_app_environment(&self) -> String;

    /// 是否处于生产环境
    fn is_production(&self) -> bool;

    /// 是否处于开发环境
    fn is_development(&self) -> bool;

    /// 获取 Webhook Secret
    fn get_webhook_secret(&self) -> String;

    /// 获取 Redis URL
    fn get_redis_url(&self) -> String;

    /// 获取健康检查 URL
    fn get_health_check_url(&self) -> Option<String>;

    /// 是否禁用 SSRF 保护
    fn is_ssrf_protection_disabled(&self) -> bool;

    /// 是否启用网络测试
    fn is_network_tests_enabled(&self) -> bool;

    /// 是否启用调试 HTML 保存
    fn is_debug_save_html_enabled(&self) -> bool;

    /// 获取 FlareSolverr URL
    fn get_flaresolverr_url(&self) -> Option<String>;
}

/// 浏览器配置 trait
///
/// 专门用于浏览器相关的配置。
pub trait BrowserConfigTrait: Interface + Send + Sync {
    /// 获取代理 URL
    fn get_proxy_url(&self) -> Option<String>;

    /// 获取远程调试 URL
    fn get_remote_debugging_url(&self) -> Option<String>;

    /// 是否测试模式
    fn is_test_mode(&self) -> bool;
}

/// 浏览器配置组件
///
/// 通过 DI 注入的浏览器配置实现。
#[derive(Component)]
#[shaku(interface = BrowserConfigTrait)]
pub struct BrowserConfigComponent {
    /// 代理 URL（优先使用环境变量）
    proxy_url: Option<String>,
    /// 远程调试 URL
    remote_debugging_url: Option<String>,
    /// 测试模式标志
    test_mode: bool,
}

impl BrowserConfigComponent {
    pub fn new() -> Self {
        Self {
            proxy_url: std::env::var("CRAWLRS_PROXY_URL")
                .ok()
                .filter(|url| !url.is_empty()),
            remote_debugging_url: std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")
                .ok()
                .filter(|url| !url.is_empty()),
            test_mode: std::env::var("CRAWLRS_TEST_NO_BROWSER_REUSE").is_ok(),
        }
    }
}

impl Default for BrowserConfigComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserConfigTrait for BrowserConfigComponent {
    fn get_proxy_url(&self) -> Option<String> {
        self.proxy_url.clone()
    }

    fn get_remote_debugging_url(&self) -> Option<String> {
        self.remote_debugging_url.clone()
    }

    fn is_test_mode(&self) -> bool {
        self.test_mode
    }
}

/// 配置服务组件
///
/// 从 Settings 中读取配置，支持环境变量覆盖。
#[derive(Component)]
#[shaku(interface = ConfigServiceTrait)]
pub struct ConfigServiceComponent {
    /// 代理 URL
    proxy_url: Option<String>,
    /// 远程调试 URL
    remote_debugging_url: Option<String>,
    /// 测试模式标志
    test_mode: bool,
    /// 默认超时（秒）
    default_timeout: u64,
    /// 浏览器超时（秒）
    browser_timeout: u64,
    /// 浏览器启动超时（秒）
    browser_launch_timeout: u64,
    /// 应用环境
    app_environment: String,
    /// Webhook Secret
    webhook_secret: String,
    /// Redis URL
    redis_url: String,
    /// 健康检查 URL
    health_check_url: Option<String>,
    /// 是否禁用 SSRF 保护
    ssrf_protection_disabled: bool,
    /// 是否启用网络测试
    network_tests_enabled: bool,
    /// 是否启用调试 HTML 保存
    debug_save_html_enabled: bool,
    /// FlareSolverr URL
    flaresolverr_url: Option<String>,
}

impl ConfigServiceComponent {
    /// 从 Settings 创建配置服务
    pub fn from_settings(
        proxy_enabled: bool,
        proxy_url: &str,
        default_timeout: u64,
        browser_timeout: u64,
    ) -> Self {
        // 环境变量优先于配置文件
        let proxy_url = std::env::var("CRAWLRS_PROXY_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .or_else(|| {
                if proxy_enabled {
                    Some(proxy_url.to_string())
                } else {
                    None
                }
            });

        Self {
            proxy_url,
            remote_debugging_url: std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")
                .ok()
                .filter(|url| !url.is_empty()),
            test_mode: std::env::var("CRAWLRS_TEST_NO_BROWSER_REUSE").is_ok(),
            default_timeout,
            browser_timeout,
            browser_launch_timeout: 30,
            app_environment: std::env::var("CRAWLRS_ENV")
                .or_else(|_| std::env::var("APP_ENVIRONMENT"))
                .unwrap_or_else(|_| "development".to_string()),
            webhook_secret: std::env::var("WEBHOOK_SECRET")
                .unwrap_or_else(|_| "default-webhook-secret".to_string()),
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            health_check_url: std::env::var("CRAWLRS_HEALTH_CHECK_URL").ok(),
            ssrf_protection_disabled: std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").is_ok(),
            network_tests_enabled: std::env::var("CRAWLRS_ENABLE_NETWORK_TESTS").is_ok(),
            debug_save_html_enabled: std::env::var("DEBUG_SAVE_HTML").is_ok(),
            flaresolverr_url: std::env::var("CRAWLRS_FLARESOLVERR_URL")
                .ok()
                .filter(|url| !url.is_empty()),
        }
    }
}

impl ConfigServiceTrait for ConfigServiceComponent {
    fn get_proxy_url(&self) -> Option<String> {
        self.proxy_url.clone()
    }

    fn get_remote_debugging_url(&self) -> Option<String> {
        self.remote_debugging_url.clone()
    }

    fn is_test_mode(&self) -> bool {
        self.test_mode
    }

    fn get_default_timeout(&self) -> Duration {
        Duration::from_secs(self.default_timeout)
    }

    fn get_browser_timeout(&self) -> Duration {
        Duration::from_secs(self.browser_timeout)
    }

    fn get_browser_launch_timeout(&self) -> Duration {
        Duration::from_secs(self.browser_launch_timeout)
    }

    fn get_app_environment(&self) -> String {
        self.app_environment.clone()
    }

    fn is_production(&self) -> bool {
        self.app_environment == "production"
    }

    fn is_development(&self) -> bool {
        self.app_environment == "development"
    }

    fn get_webhook_secret(&self) -> String {
        self.webhook_secret.clone()
    }

    fn get_redis_url(&self) -> String {
        self.redis_url.clone()
    }

    fn get_health_check_url(&self) -> Option<String> {
        self.health_check_url.clone()
    }

    fn is_ssrf_protection_disabled(&self) -> bool {
        self.ssrf_protection_disabled
    }

    fn is_network_tests_enabled(&self) -> bool {
        self.network_tests_enabled
    }

    fn is_debug_save_html_enabled(&self) -> bool {
        self.debug_save_html_enabled
    }

    fn get_flaresolverr_url(&self) -> Option<String> {
        self.flaresolverr_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_service_proxy_url_from_env() {
        // 设置环境变量
        std::env::set_var("CRAWLRS_PROXY_URL", "http://test.proxy:8080");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(
            config.get_proxy_url(),
            Some("http://test.proxy:8080".to_string())
        );

        // 清理
        std::env::remove_var("CRAWLRS_PROXY_URL");
    }

    #[test]
    fn test_config_service_proxy_from_settings() {
        std::env::remove_var("CRAWLRS_PROXY_URL");

        let config =
            ConfigServiceComponent::from_settings(true, "http://settings.proxy:8080", 30, 30);
        assert_eq!(
            config.get_proxy_url(),
            Some("http://settings.proxy:8080".to_string())
        );
    }

    #[test]
    fn test_config_service_test_mode() {
        std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(config.is_test_mode());

        std::env::remove_var("CRAWLRS_TEST_NO_BROWSER_REUSE");
    }

    #[test]
    fn test_browser_config_component() {
        std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "localhost:9222");
        std::env::set_var("CRAWLRS_PROXY_URL", "http://localhost:1080");

        let config = BrowserConfigComponent::new();
        assert_eq!(
            config.get_remote_debugging_url(),
            Some("localhost:9222".to_string())
        );
        assert_eq!(
            config.get_proxy_url(),
            Some("http://localhost:1080".to_string())
        );

        std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
        std::env::remove_var("CRAWLRS_PROXY_URL");
    }

    #[test]
    fn test_browser_config_component_default_impl() {
        std::env::remove_var("CRAWLRS_PROXY_URL");
        std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
        std::env::remove_var("CRAWLRS_TEST_NO_BROWSER_REUSE");

        let config = BrowserConfigComponent::default();
        assert_eq!(config.get_proxy_url(), None);
        assert_eq!(config.get_remote_debugging_url(), None);
        assert!(!config.is_test_mode());
    }

    #[test]
    fn test_browser_config_component_empty_env_vars_filtered() {
        std::env::set_var("CRAWLRS_PROXY_URL", "");
        std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "");

        let config = BrowserConfigComponent::new();
        assert_eq!(config.get_proxy_url(), None);
        assert_eq!(config.get_remote_debugging_url(), None);

        std::env::remove_var("CRAWLRS_PROXY_URL");
        std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
    }

    #[test]
    fn test_browser_config_component_test_mode() {
        std::env::set_var("CRAWLRS_TEST_NO_BROWSER_REUSE", "1");

        let config = BrowserConfigComponent::new();
        assert!(config.is_test_mode());

        std::env::remove_var("CRAWLRS_TEST_NO_BROWSER_REUSE");
    }

    #[test]
    fn test_config_service_proxy_disabled_no_env() {
        std::env::remove_var("CRAWLRS_PROXY_URL");

        let config = ConfigServiceComponent::from_settings(false, "http://ignored.proxy:8080", 30, 30);
        assert_eq!(config.get_proxy_url(), None);
    }

    #[test]
    fn test_config_service_default_timeout_values() {
        std::env::remove_var("CRAWLRS_PROXY_URL");

        let config = ConfigServiceComponent::from_settings(false, "", 45, 60);
        assert_eq!(config.get_default_timeout(), Duration::from_secs(45));
        assert_eq!(config.get_browser_timeout(), Duration::from_secs(60));
        assert_eq!(config.get_browser_launch_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_config_service_app_environment_default() {
        std::env::remove_var("CRAWLRS_ENV");
        std::env::remove_var("APP_ENVIRONMENT");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_app_environment(), "development");
        assert!(config.is_development());
        assert!(!config.is_production());
    }

    #[test]
    fn test_config_service_app_environment_from_crawlrs_env() {
        std::env::set_var("CRAWLRS_ENV", "production");
        std::env::remove_var("APP_ENVIRONMENT");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_app_environment(), "production");
        assert!(config.is_production());
        assert!(!config.is_development());

        std::env::remove_var("CRAWLRS_ENV");
    }

    #[test]
    fn test_config_service_app_environment_from_app_env_fallback() {
        std::env::remove_var("CRAWLRS_ENV");
        std::env::set_var("APP_ENVIRONMENT", "staging");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_app_environment(), "staging");
        assert!(!config.is_development());
        assert!(!config.is_production());

        std::env::remove_var("APP_ENVIRONMENT");
    }

    #[test]
    fn test_config_service_webhook_secret_default() {
        std::env::remove_var("WEBHOOK_SECRET");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_webhook_secret(), "default-webhook-secret");
    }

    #[test]
    fn test_config_service_webhook_secret_from_env() {
        std::env::set_var("WEBHOOK_SECRET", "my-secret-key");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_webhook_secret(), "my-secret-key");

        std::env::remove_var("WEBHOOK_SECRET");
    }

    #[test]
    fn test_config_service_redis_url_default() {
        std::env::remove_var("REDIS_URL");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_redis_url(), "redis://localhost:6379");
    }

    #[test]
    fn test_config_service_redis_url_from_env() {
        std::env::set_var("REDIS_URL", "redis://custom:6380");

        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_redis_url(), "redis://custom:6380");

        std::env::remove_var("REDIS_URL");
    }

    #[test]
    fn test_config_service_health_check_url() {
        std::env::remove_var("CRAWLRS_HEALTH_CHECK_URL");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_health_check_url(), None);

        std::env::set_var("CRAWLRS_HEALTH_CHECK_URL", "http://health:8080/check");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(
            config.get_health_check_url(),
            Some("http://health:8080/check".to_string())
        );

        std::env::remove_var("CRAWLRS_HEALTH_CHECK_URL");
    }

    #[test]
    fn test_config_service_ssrf_protection_disabled() {
        std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(!config.is_ssrf_protection_disabled());

        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "1");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(config.is_ssrf_protection_disabled());

        std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
    }

    #[test]
    fn test_config_service_network_tests_enabled() {
        std::env::remove_var("CRAWLRS_ENABLE_NETWORK_TESTS");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(!config.is_network_tests_enabled());

        std::env::set_var("CRAWLRS_ENABLE_NETWORK_TESTS", "1");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(config.is_network_tests_enabled());

        std::env::remove_var("CRAWLRS_ENABLE_NETWORK_TESTS");
    }

    #[test]
    fn test_config_service_debug_save_html_enabled() {
        std::env::remove_var("DEBUG_SAVE_HTML");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(!config.is_debug_save_html_enabled());

        std::env::set_var("DEBUG_SAVE_HTML", "1");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert!(config.is_debug_save_html_enabled());

        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    #[test]
    fn test_config_service_flaresolverr_url() {
        std::env::remove_var("CRAWLRS_FLARESOLVERR_URL");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_flaresolverr_url(), None);

        std::env::set_var("CRAWLRS_FLARESOLVERR_URL", "");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_flaresolverr_url(), None);

        std::env::set_var("CRAWLRS_FLARESOLVERR_URL", "http://flaresolverr:8191");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(
            config.get_flaresolverr_url(),
            Some("http://flaresolverr:8191".to_string())
        );

        std::env::remove_var("CRAWLRS_FLARESOLVERR_URL");
    }

    #[test]
    fn test_config_service_remote_debugging_url() {
        std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(config.get_remote_debugging_url(), None);

        std::env::set_var("CHROMIUM_REMOTE_DEBUGGING_URL", "localhost:9222");
        let config = ConfigServiceComponent::from_settings(false, "", 30, 30);
        assert_eq!(
            config.get_remote_debugging_url(),
            Some("localhost:9222".to_string())
        );

        std::env::remove_var("CHROMIUM_REMOTE_DEBUGGING_URL");
    }

    #[test]
    fn test_config_service_proxy_url_env_overrides_settings() {
        std::env::set_var("CRAWLRS_PROXY_URL", "http://env.proxy:9090");

        let config = ConfigServiceComponent::from_settings(true, "http://settings.proxy:8080", 30, 30);
        assert_eq!(
            config.get_proxy_url(),
            Some("http://env.proxy:9090".to_string())
        );

        std::env::remove_var("CRAWLRS_PROXY_URL");
    }
}
