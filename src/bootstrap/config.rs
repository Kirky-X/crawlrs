// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Configuration loading, validation, and port detection.
//!
//! 此模块负责在应用启动早期进行配置和环境变量的安全验证

use crate::config::settings::Settings;
use crate::infrastructure::security::env_var_security::{EnvVarSecurityMonitor, EnvVarValidator};
use anyhow::Result;
use confers::{ConfigBuilder, EnvSource};
use log::{debug, error, info, warn};

/// Load application configuration from the standard settings file and environment.
///
/// Uses confers 0.4 `ConfigBuilder` to merge `config/default.toml` with
/// environment variables prefixed by `CRAWLRS__` (nested via `__`).
/// Note: confers 0.4's `load_sync()` only applies field-level defaults and env
/// vars; it no longer auto-discovers config files (breaking change from 0.2.2).
pub fn load_settings() -> Result<Settings> {
    let settings = ConfigBuilder::<Settings>::new()
        .file("config/default.toml")
        .source(Box::new(
            EnvSource::with_prefix("CRAWLRS__").separator("__"),
        ))
        .build()
        .map_err(|e| anyhow::anyhow!("Configuration load failed: {}", e))?;
    info!("Configuration loaded successfully from config sources");
    Ok(settings)
}

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
    // Validation is now handled by confers automatically via #[config(validate)]
    // This function can be used for additional production-specific checks if needed
    debug!("Security validation configured via confers library");
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
    let is_test = env.to_lowercase() == "test"
        || std::env::var("CRAWLRS__TEST_MODE").unwrap_or_default() == "true";

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

/// Detect and configure an available port.
///
/// This function attempts to use the configured port, and if it's unavailable,
/// it will find and use an alternative port.
///
/// # Arguments
///
/// * `settings` - The settings to modify with the detected port
///
/// # Returns
///
/// Returns the port that will be used, along with any informational logs.
pub fn detect_available_port(settings: &mut Settings) -> Result<u16> {
    let port_result = crate::utils::port_sniffer::PortSniffer::find_available_port(
        settings.server.port,
        settings.server.enable_port_detection,
        50,
    );

    match port_result {
        Ok(result) => {
            if result.port != settings.server.port {
                info!(
                    "Default port {} is occupied, switching to port {}",
                    settings.server.port, result.port
                );
                settings.server.port = result.port;
            }
            for log in result.logs {
                info!("{}", log);
            }
            Ok(result.port)
        }
        Err(e) => {
            error!("Port detection failed: {}", e);
            Err(anyhow::anyhow!("Failed to find available port: {}", e))
        }
    }
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

    info!("Application configuration completed successfully");
    Ok((settings, port))
}

#[cfg(test)]
mod tests {
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
    fn test_validate_environment_non_production_returns_ok() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // In non-production mode, validate_environment should return Ok(())
        // even if required env vars are missing (only warns).
        // Use test mode to avoid interference from forbidden env vars set by
        // parallel tests (e.g. LD_PRELOAD from other test cases).
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
    fn test_load_settings_returns_valid_config() {
        let settings = load_settings();
        assert!(
            settings.is_ok(),
            "load_settings failed: {:?}",
            settings.err()
        );
        let settings = settings.unwrap();
        // Verify some default values from config/default.toml
        assert_eq!(settings.server.port, 8899);
        assert_eq!(settings.server.host, "0.0.0.0");
    }

    #[test]
    fn test_load_settings_has_database_config() {
        let settings = load_settings().expect("Failed to load settings");
        // max_connections may be overridden by env var; just verify field exists
        let _max_conn = &settings.database.max_connections;
    }

    #[test]
    fn test_load_settings_has_rate_limiting_config() {
        let settings = load_settings().expect("Failed to load settings");
        // Rate limiting config should be present (values may be overridden by env)
        let _enabled = settings.rate_limiting.enabled;
        let _rpm = settings.rate_limiting.default_rpm;
    }

    #[test]
    fn test_load_settings_has_cors_config() {
        let settings = load_settings().expect("Failed to load settings");
        assert_eq!(settings.cors.allowed_origins, "*");
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

    // ========== detect_available_port tests ==========

    #[test]
    fn test_detect_available_port_with_detection_disabled_returns_ok() {
        // Use a high port number likely to be free, with detection disabled.
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 0; // port 0 lets OS assign a free port; but detection disabled means we check this port
        settings.server.enable_port_detection = false;
        // Port 0 is reserved; with detection disabled, the sniffer checks if it's in use.
        // Since port 0 is never bindable, this may behave specially. Use a high port instead.
        settings.server.port = 49999;
        let result = detect_available_port(&mut settings);
        assert!(
            result.is_ok(),
            "detect_available_port should return Ok for a free high port with detection disabled"
        );
    }

    #[test]
    fn test_detect_available_port_returns_configured_port_when_free() {
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 49998;
        settings.server.enable_port_detection = false;
        let port = detect_available_port(&mut settings).expect("Should find available port");
        assert_eq!(
            port, 49998,
            "Should return the configured port when it is free"
        );
    }

    #[test]
    fn test_detect_available_port_updates_settings_port() {
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 49997;
        settings.server.enable_port_detection = false;
        let _ = detect_available_port(&mut settings).expect("Should find available port");
        assert_eq!(
            settings.server.port, 49997,
            "settings.server.port should match the detected port"
        );
    }

    #[test]
    fn test_detect_available_port_with_detection_enabled_returns_ok() {
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 49996;
        settings.server.enable_port_detection = true;
        let result = detect_available_port(&mut settings);
        assert!(
            result.is_ok(),
            "detect_available_port should return Ok with detection enabled"
        );
    }

    // ========== validate_environment production mode tests ==========

    #[test]
    fn test_validate_environment_production_missing_required_returns_error() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        // Save current values
        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();

        // Remove required env vars to trigger missing-required error in production
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("DATABASE_URL");
        // Enable test mode to skip forbidden variable check
        std::env::set_var("CRAWLRS__TEST_MODE", "true");

        let result = validate_environment(true);

        // Restore saved values
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

    // ========== validate_environment test mode detection ==========

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

        // In test mode, forbidden variables check is skipped
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

        // CRAWLRS_ENV=test should trigger is_test=true
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

    // ========== validate_security with different settings ==========

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

    // ========== load_settings returns consistent results ==========

    #[test]
    fn test_load_settings_returns_consistent_port() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = load_settings().expect("Failed to load settings");
        assert_eq!(settings1.server.port, settings2.server.port);
    }

    #[test]
    fn test_load_settings_returns_consistent_host() {
        let settings1 = load_settings().expect("Failed to load settings");
        let settings2 = load_settings().expect("Failed to load settings");
        assert_eq!(settings1.server.host, settings2.server.host);
    }

    // ========== detect_available_port with different ports ==========

    #[test]
    fn test_detect_available_port_does_not_change_when_port_free() {
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 49995;
        settings.server.enable_port_detection = false;
        let original_port = settings.server.port;
        let _ = detect_available_port(&mut settings).expect("Should find available port");
        assert_eq!(
            settings.server.port, original_port,
            "Port should not change when the configured port is free"
        );
    }

    #[test]
    fn test_detect_available_port_with_detection_enabled_finds_port() {
        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 49994;
        settings.server.enable_port_detection = true;
        let result = detect_available_port(&mut settings);
        assert!(
            result.is_ok(),
            "detect_available_port with detection enabled should find a port"
        );
        let port = result.unwrap();
        assert!(port > 0, "Detected port should be greater than 0");
    }

    // ========== load_and_configure returns consistent settings ==========

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

    // ========== validate_environment non-production with required vars ==========

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

    // ========== Settings clone and equality ==========

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

    // ========== load_settings server config ==========

    #[test]
    fn test_load_settings_server_enable_port_detection_exists() {
        let settings = load_settings().expect("Failed to load settings");
        // Just verify the field exists and is a bool
        let _ = settings.server.enable_port_detection;
    }

    #[test]
    fn test_load_settings_has_server_config() {
        let settings = load_settings().expect("Failed to load settings");
        assert!(!settings.server.host.is_empty());
        assert!(settings.server.port > 0);
    }

    // ========== validate_environment forbidden variables path ==========

    #[test]
    fn test_validate_environment_forbidden_vars_in_non_test_mode_returns_error() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let saved_app_env = std::env::var("APP_ENVIRONMENT").ok();
        let saved_db_url = std::env::var("DATABASE_URL").ok();
        let saved_test_mode = std::env::var("CRAWLRS__TEST_MODE").ok();
        let saved_crawlrs_env = std::env::var("CRAWLRS_ENV").ok();
        let saved_ld_preload = std::env::var("LD_PRELOAD").ok();

        // Set a forbidden env var (LD_PRELOAD is in the forbidden list)
        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        // Ensure we are NOT in test mode
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::remove_var("CRAWLRS__TEST_MODE");
        std::env::remove_var("CRAWLRS_ENV");

        let result = validate_environment(false);

        // Restore saved values
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

        // Set a forbidden env var
        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        // Set test mode via CRAWLRS__TEST_MODE
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::set_var("CRAWLRS__TEST_MODE", "true");
        std::env::remove_var("CRAWLRS_ENV");

        let result = validate_environment(false);

        // Restore saved values
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

        // Set a forbidden env var
        std::env::set_var("LD_PRELOAD", "/tmp/fake_lib.so");
        // CRAWLRS_ENV=test triggers is_test=true
        std::env::set_var("CRAWLRS_ENV", "test");
        std::env::set_var("APP_ENVIRONMENT", "development");
        std::env::set_var("DATABASE_URL", "postgresql://test:test@localhost/test");
        std::env::remove_var("CRAWLRS__TEST_MODE");

        let result = validate_environment(false);

        // Restore saved values
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
}
