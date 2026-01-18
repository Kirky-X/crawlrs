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
use tracing::{debug, error, info, warn};

/// Load application configuration from settings file.
///
/// This function reads the configuration from the standard settings location
/// and returns a configured [`Settings`] instance.
pub fn load_settings() -> Result<Settings> {
    let settings = Settings::new()?;
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
pub fn validate_security(settings: &Settings, is_production: bool) -> Result<()> {
    if let Err(e) = settings.validate_security() {
        if is_production {
            error!(
                "Configuration security validation failed in production: {}",
                e
            );
            error!("Server will NOT start due to security concerns. Please fix the configuration issues above.");
            return Err(anyhow::anyhow!("Security validation failed: {}", e));
        } else {
            warn!("Security warning in non-production environment: {}", e);
        }
    }
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

    // Check for forbidden variables
    if !report.forbidden_variables.is_empty() {
        let error_msg = format!(
            "Forbidden environment variables detected: {}",
            report.forbidden_variables.join(", ")
        );
        error!("CRITICAL: {}", error_msg);
        return Err(anyhow::anyhow!("{}", error_msg));
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
