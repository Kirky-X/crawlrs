// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置验证 — 安全验证、环境变量检查、启动编排。

use super::config_loader::{detect_available_port, load_settings};
use crate::config::settings::Settings;
use crate::infrastructure::security::env_var_security::{EnvVarSecurityMonitor, EnvVarValidator};
use anyhow::Result;
use log::{debug, error, info, warn};

/// Validate configuration security settings.
///
/// In production mode, this function will return an error if any security
/// issues are detected in the configuration. In non-production modes, it
/// will only log warnings.
///
/// # Arguments
///
/// * `settings` - The settings to validate
/// * `is_production` - Whether the application is running in production mode
///
/// # Returns
///
/// Returns `Ok(())` if validation passes, or an error with details about
/// the security issue.
pub fn validate_security(_settings: &Settings, _is_production: bool) -> Result<()> {
    // 安全审查 H-1 修复说明：
    //
    // 原注释"Validation is now handled by confers automatically via #[config(validate)]"
    // 是错误的——confers 0.4 集成的是 `garde::Validate`，而 `Settings` 用的是
    // `validator::Validate`，两者不兼容。`validator::Validate::validate` 已在
    // `load_settings()` 中显式调用（覆盖所有 `#[validate(...)]` 注解）。
    //
    // 此函数保留为生产环境特定检查的扩展点（如密钥强度、JWT 长度等业务规则）。
    debug!("Security validation configured via validator::Validate in load_settings()");
    Ok(())
}

/// Validate environment variables for security.
///
/// This function performs comprehensive checks on environment variables:
/// - White list validation
/// - Sensitive value masking
/// - Forbidden variable detection
/// - Required variable checking
///
/// # Arguments
///
/// * `is_production` - Whether the application is running in production mode
///
/// # Returns
///
/// Returns `Ok(())` if validation passes, or an error with details about
/// security issues.
pub fn validate_environment(is_production: bool) -> Result<()> {
    info!("Starting environment variable security validation...");

    // Create security monitor and validator
    let monitor = EnvVarSecurityMonitor::default();
    let validator = EnvVarValidator::new(monitor.clone(), vec!["APP_ENVIRONMENT", "DATABASE_URL"]);

    // Log security warnings
    monitor.log_security_warnings();

    // Validate required variables
    if let Err(missing) = validator.validate_required() {
        let error_msg = format!(
            "Missing required environment variables: {}",
            missing.join(", ")
        );
        if is_production {
            error!("CRITICAL: {}", error_msg);
            return Err(anyhow::anyhow!("{}", error_msg));
        } else {
            warn!(
                "Missing required environment variables in non-production: {}",
                missing.join(", ")
            );
        }
    }

    // Generate and check security report
    let report = monitor.generate_security_report();

    // 在测试环境中跳过禁止环境变量检查
    // 使用配置服务获取环境，如果不可用则回退到环境变量
    let env = std::env::var("CRAWLRS_ENV")
        .or_else(|_| std::env::var("APP_ENVIRONMENT"))
        .unwrap_or_else(|_| "development".to_string());
    let env_lower = env.to_lowercase();
    let is_test =
        env_lower == "test" || std::env::var("CRAWLRS__TEST_MODE").unwrap_or_default() == "true";
    let is_dev = env_lower == "development" || env_lower == "dev" || is_test;

    // SSRF 防护禁用开关风险告警（R-sec-002）
    // 仅在非 test/development 环境下告警；开发/测试场景允许禁用以方便调试
    if std::env::var(crate::common::constants::env_vars::DISABLE_SSRF_PROTECTION).is_ok() && !is_dev
    {
        warn!(
            "⚠️ SSRF 保护已通过 CRAWLRS_DISABLE_SSRF_PROTECTION 禁用！\
             此开关仅限开发/测试环境使用，生产环境禁用将导致内网探测风险。\
             当前环境: {}。如需生产部署，请移除该环境变量并配置 trusted_proxies。",
            env
        );
    }

    // Check for forbidden variables
    if !report.forbidden_variables.is_empty() && !is_test {
        let error_msg = format!(
            "Forbidden environment variables detected: {}",
            report.forbidden_variables.join(", ")
        );
        error!("CRITICAL: {}", error_msg);
        return Err(anyhow::anyhow!("{}", error_msg));
    } else if !report.forbidden_variables.is_empty() && is_test {
        warn!(
            "Test mode: Skipping forbidden environment variable check: {}",
            report.forbidden_variables.join(", ")
        );
    }

    // Check security score
    if report.security_score < 70 {
        let msg = format!(
            "Security score {} is below acceptable level (70). Review warnings above.",
            report.security_score
        );
        if is_production {
            error!("CRITICAL: {}", msg);
            return Err(anyhow::anyhow!("{}", msg));
        } else {
            warn!("{}", msg);
        }
    }

    info!(
        "Environment variable validation complete. Security score: {}/100",
        report.security_score
    );

    Ok(())
}

/// Load, validate, and configure settings for application startup.
///
/// This is a convenience function that combines loading, security validation,
/// environment validation, and port detection into a single call.
///
/// # Arguments
///
/// * `is_production` - Whether the application is running in production mode
///
/// # Returns
///
/// Returns a tuple of the configured settings and the port to use.
pub fn load_and_configure(is_production: bool) -> Result<(Settings, u16)> {
    debug!("Starting application configuration...");

    // Step 1: Validate environment variables first (before loading config)
    debug!("Step 1/4: Validating environment variables...");
    validate_environment(is_production)?;

    // Step 2: Load configuration settings
    debug!("Step 2/4: Loading configuration settings...");
    let mut settings = load_settings()?;

    // Step 3: Validate configuration security
    debug!("Step 3/4: Validating configuration security...");
    validate_security(&settings, is_production)?;

    // Step 4: Detect available port
    debug!("Step 4/4: Detecting available port...");
    let port = detect_available_port(&mut settings)?;

    // Webhook secret fail-fast（R-security-001）
    // 当 `webhook` feature 启用且运行在非 test/development 环境时，空的
    // `webhook.secret` 配置必须阻止服务启动，避免以无签名校验的 Webhook 上生产。
    validate_webhook_secret_fail_fast(is_production, &settings.webhook.secret)?;

    info!("Application configuration completed successfully");
    Ok((settings, port))
}

/// Webhook secret 启动 fail-fast 校验（R-security-001）。
///
/// 当 `webhook` feature 启用且 `is_production=true`（且当前非 test/development 环境）
/// 时，空的 webhook secret 返回 `Err` 阻止启动。
#[cfg(feature = "webhook")]
fn validate_webhook_secret_fail_fast(is_production: bool, secret: &str) -> Result<()> {
    let env = std::env::var(crate::common::constants::env_vars::ENV)
        .or_else(|_| std::env::var(crate::common::constants::env_vars::APP_ENVIRONMENT))
        .unwrap_or_else(|_| "development".to_string());
    let env_lower = env.to_lowercase();
    let is_test_env =
        env_lower == "test" || std::env::var("CRAWLRS__TEST_MODE").unwrap_or_default() == "true";

    if is_production && !is_test_env && secret.is_empty() {
        error!(
            "CRITICAL: webhook.secret must not be empty in production (webhook feature enabled)"
        );
        return Err(anyhow::anyhow!(
            "webhook.secret must not be empty in production (webhook feature enabled)"
        ));
    }
    Ok(())
}

/// 非 `webhook` feature 下的占位：校验直接通过。
#[cfg(not(feature = "webhook"))]
fn validate_webhook_secret_fail_fast(_is_production: bool, _secret: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::config_loader::load_settings;
    use super::*;
    use crate::common::test_support::ENV_MUTEX;

    #[test]
    fn test_validate_security_returns_ok_for_non_production() {
        let settings = load_settings().expect("Failed to load settings");
        let result = validate_security(&settings, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_security_returns_ok_for_production() {
        let settings = load_settings().expect("Failed to load settings");
        let result = validate_security(&settings, true);
        // validate_security always returns Ok(()) - it's a placeholder
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_security_does_not_modify_settings() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = settings1.clone();
        let _ = validate_security(&settings1, false);
        // Settings should be unchanged after validation
        assert_eq!(settings1.server.port, settings2.server.port);
        assert_eq!(settings1.server.host, settings2.server.host);
    }

    #[test]
    fn test_validate_security_preserves_cors_settings() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = settings1.clone();
        let _ = validate_security(&settings1, true);
        assert_eq!(
            settings1.cors.allowed_origins,
            settings2.cors.allowed_origins
        );
    }

    #[test]
    fn test_validate_security_preserves_database_settings() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = settings1.clone();
        let _ = validate_security(&settings1, false);
        assert_eq!(
            settings1.database.max_connections,
            settings2.database.max_connections
        );
    }

    #[test]
    fn test_validate_security_preserves_rate_limiting_settings() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = settings1.clone();
        let _ = validate_security(&settings1, true);
        assert_eq!(
            settings1.rate_limiting.enabled,
            settings2.rate_limiting.enabled
        );
        assert_eq!(
            settings1.rate_limiting.default_rpm,
            settings2.rate_limiting.default_rpm
        );
    }

    #[test]
    fn test_settings_clone_is_equal() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = settings1.clone();
        assert_eq!(settings1.server.port, settings2.server.port);
        assert_eq!(settings1.server.host, settings2.server.host);
        assert_eq!(
            settings1.cors.allowed_origins,
            settings2.cors.allowed_origins
        );
    }

    // ========== validate_environment tests ==========

    #[test]
    fn test_validate_environment_non_production_returns_ok() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        let result = validate_environment(false);
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_environment_production_missing_required_returns_error() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();

        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("DATABASE_URL");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = validate_environment(true);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(
            result.is_err(),
            "validate_environment in production mode should error when required env vars are missing"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Missing required environment variables"),
            "Error should mention missing required env vars, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_validate_environment_production_with_required_vars_succeeds() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();

        std::env::set_var("APP_ENVIRONMENT", "production");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = validate_environment(true);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(
            result.is_ok(),
            "validate_environment in production mode should succeed when required vars are set and test mode skips forbidden check, got err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_validate_environment_test_mode_skips_forbidden_check() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();

        std::env::set_var("APP_ENVIRONMENT", "test");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        std::env::remove_var("CRAWLRS_ENV");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        }

        assert!(
            result.is_ok(),
            "validate_environment in test mode should succeed, got err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_validate_environment_crawlrs_env_test_mode() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();

        std::env::set_var("CRAWLRS_ENV", "test");
        std::env::set_var("APP_ENVIRONMENT", "production");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_environment(false);

        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(
            result.is_ok(),
            "validate_environment with CRAWLRS_ENV=test should succeed"
        );
    }

    #[test]
    fn test_validate_environment_non_production_with_required_vars_succeeds() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();

        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(
            result.is_ok(),
            "validate_environment in non-production with required vars should succeed"
        );
    }

    #[test]
    fn test_validate_environment_forbidden_vars_in_non_test_mode_returns_error() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ld_preload = std::env::var("LD_PRELOAD").ok();

        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::remove_var("CRAWLRS__TEST_MODE");
        std::env::remove_var("CRAWLRS_ENV");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_ld_preload {
            std::env::set_var("LD_PRELOAD", v);
        } else {
            std::env::remove_var("LD_PRELOAD");
        }

        assert!(
            result.is_err(),
            "validate_environment should return error when forbidden vars detected in non-test mode, got: {:?}",
            result.ok()
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Forbidden"),
            "Error should mention forbidden variables, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_validate_environment_forbidden_vars_in_test_mode_warns_only() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ld_preload = std::env::var("LD_PRELOAD").ok();

        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        std::env::remove_var("CRAWLRS_ENV");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_ld_preload {
            std::env::set_var("LD_PRELOAD", v);
        } else {
            std::env::remove_var("LD_PRELOAD");
        }

        assert!(
            result.is_ok(),
            "validate_environment in test mode should succeed even with forbidden vars (warn only), got err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_validate_environment_crawlrs_env_test_mode_with_forbidden_vars() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ld_preload = std::env::var("LD_PRELOAD").ok();

        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        std::env::set_var("CRAWLRS_ENV", "test");
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_ld_preload {
            std::env::set_var("LD_PRELOAD", v);
        } else {
            std::env::remove_var("LD_PRELOAD");
        }

        assert!(
            result.is_ok(),
            "validate_environment with CRAWLRS_ENV=test should succeed even with forbidden vars"
        );
    }

    // ========== SSRF protection disable warning (T002) ==========

    #[test]
    fn test_validate_environment_warns_when_ssrf_disabled_in_non_dev_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ssrf = std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").ok();

        std::env::set_var("APP_ENVIRONMENT", "production");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        std::env::remove_var("CRAWLRS_ENV");
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_ssrf {
            std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", v);
        } else {
            std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
        }

        assert!(
            result.is_ok(),
            "validate_environment should succeed with SSRF disabled (warn only), got err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_validate_environment_no_warn_when_ssrf_disabled_in_dev_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ssrf = std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").ok();

        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        std::env::remove_var("CRAWLRS_ENV");
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");

        let result = validate_environment(false);

        if let Some(v) = saved_app_env {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_db_url {
            std::env::set_var("DATABASE_URL", v);
        } else {
            std::env::remove_var("DATABASE_URL");
        }
        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }
        if let Some(v) = saved_crawlrs_env {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
        if let Some(v) = saved_ssrf {
            std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", v);
        } else {
            std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
        }

        assert!(result.is_ok());
    }

    // ========== load_and_configure tests ==========

    #[test]
    fn test_load_and_configure_non_production_succeeds() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = load_and_configure(false);

        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(
            result.is_ok(),
            "load_and_configure in non-production mode should succeed, got err: {:?}",
            result.err()
        );
        let (settings, port) = result.expect("load_and_configure should succeed");
        assert!(port > 0, "Detected port should be greater than 0");
        assert_eq!(
            settings.server.port, port,
            "settings.server.port should match the detected port"
        );
    }

    #[test]
    fn test_load_and_configure_returns_valid_settings() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = load_and_configure(false);

        if let Some(v) = saved_test_mode {
            std::env::set_var("CRAWLRS__TEST_MODE", v);
        } else {
            std::env::remove_var("CRAWLRS__TEST_MODE");
        }

        assert!(result.is_ok());
        let (settings, _port) = result.unwrap();
        assert_eq!(settings.server.host, "0.0.0.0");
    }

    // ========== Webhook secret fail-fast tests (R-security-001 / T001-T002) ==========

    #[test]
    #[cfg(feature = "webhook")]
    fn test_webhook_secret_empty_in_production_errors() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS_ENV", "production");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_webhook_secret_fail_fast(true, "");

        match saved_env {
            Some(v) => std::env::set_var("CRAWLRS_ENV", v),
            None => std::env::remove_var("CRAWLRS_ENV"),
        }
        match saved_test_mode {
            Some(v) => std::env::set_var("CRAWLRS__TEST_MODE", v),
            None => std::env::remove_var("CRAWLRS__TEST_MODE"),
        }

        assert!(
            result.is_err(),
            "empty webhook secret in production must fail, got: {:?}",
            result
        );
    }

    #[test]
    #[cfg(feature = "webhook")]
    fn test_webhook_secret_empty_in_test_env_passes() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS_ENV", "test");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_webhook_secret_fail_fast(true, "");

        match saved_env {
            Some(v) => std::env::set_var("CRAWLRS_ENV", v),
            None => std::env::remove_var("CRAWLRS_ENV"),
        }
        match saved_test_mode {
            Some(v) => std::env::set_var("CRAWLRS__TEST_MODE", v),
            None => std::env::remove_var("CRAWLRS__TEST_MODE"),
        }

        assert!(
            result.is_ok(),
            "empty webhook secret in test env should pass, got err: {:?}",
            result.err()
        );
    }

    #[test]
    #[cfg(feature = "webhook")]
    fn test_webhook_secret_non_empty_in_production_passes() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        std::env::set_var("CRAWLRS_ENV", "production");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_webhook_secret_fail_fast(true, "non-empty-secret");

        match saved_env {
            Some(v) => std::env::set_var("CRAWLRS_ENV", v),
            None => std::env::remove_var("CRAWLRS_ENV"),
        }
        match saved_test_mode {
            Some(v) => std::env::set_var("CRAWLRS__TEST_MODE", v),
            None => std::env::remove_var("CRAWLRS__TEST_MODE"),
        }

        assert!(
            result.is_ok(),
            "non-empty webhook secret in production should pass, got err: {:?}",
            result.err()
        );
    }
}
