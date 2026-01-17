// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Configuration loading, validation, and port detection.

use crate::config::settings::Settings;
use anyhow::Result;
use tracing::{error, info, warn};

/// Load application configuration from settings file.
///
/// This function reads the configuration from the standard settings location
/// and returns a configured [`Settings`] instance.
pub fn load_settings() -> Result<Settings> {
    let settings = Settings::new()?;
    info!("Configuration loaded");
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
/// and port detection into a single call.
///
/// # Arguments
///
/// * `is_production` - Whether the application is running in production mode
///
/// # Returns
///
/// Returns a tuple of the configured settings and the port to use.
pub fn load_and_configure(is_production: bool) -> Result<(Settings, u16)> {
    let mut settings = load_settings()?;
    validate_security(&settings, is_production)?;
    let port = detect_available_port(&mut settings)?;
    Ok((settings, port))
}
