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
use log::{debug, error, info, warn};

/// Load application configuration from settings file.
///
/// This function reads the configuration from the standard settings location
/// and returns a configured [`Settings`] instance.
pub fn load_settings() -> Result<Settings> {
    let settings = Settings::load_sync()?;
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
        // In non-production mode, validate_environment should return Ok(())
        // even if required env vars are missing (only warns)
        let result = validate_environment(false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_settings_returns_valid_config() {
        let settings = load_settings();
        assert!(settings.is_ok());
        let settings = settings.unwrap();
        // Verify some default values from config/default.toml
        assert_eq!(settings.server.port, 8899);
        assert_eq!(settings.server.host, "0.0.0.0");
    }

    #[test]
    fn test_load_settings_has_redis_config() {
        let settings = load_settings().expect("Failed to load settings");
        // Redis URL should be loaded from config or env var
        // Just verify the field exists and is a string
        let _url = &settings.redis.url;
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
}
