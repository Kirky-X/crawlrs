// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置加载 — 从文件和环境变量加载配置，端口检测。

use crate::config::settings::Settings;
use anyhow::Result;
use confers::{ConfigBuilder, EnvSource};
use log::{error, info};
use validator::Validate;

/// Load application configuration from the standard settings file and environment.
///
/// Uses confers 0.4 `ConfigBuilder` to merge `config/default.toml` with
/// environment variables prefixed by `CRAWLRS__` (nested via `__`).
/// Note: confers 0.4's `load_sync()` only applies field-level defaults and env
/// vars; it no longer auto-discovers config files (breaking change from 0.2.2).
///
/// # 安全验证（T062 安全审查 CRITICAL-1 修复）
///
/// confers 0.4 `#[config(validate)]` 集成的是 `garde::Validate`，而 `Settings` 用的是
/// `validator::Validate`，两者不兼容——`#[validate(range(...))]` 等注解不会被
/// confers 自动触发。本函数在 `build()` 后显式调用 `Settings::validate()`，
/// 覆盖所有 `#[validate(...)]` 注解（EngineTimeoutSettings 的 range、TaskQueryRequestDto
/// 的 range 等），防止环境变量注入 `CRAWLRS__TIMEOUTS__ENGINES__DEFAULT_TIMEOUT_SECONDS=0`
/// 绕过校验触发 DoS（CWE-400 + CWE-20 + 配置注入）。
pub fn load_settings() -> Result<Settings> {
    let settings = ConfigBuilder::<Settings>::new()
        .file("config/default.toml")
        .source(Box::new(
            EnvSource::with_prefix("CRAWLRS__").separator("__"),
        ))
        .build()
        .map_err(|e| anyhow::anyhow!("Configuration load failed: {}", e))?;

    // T062 安全审查 CRITICAL-1 修复：显式调用 validator::Validate::validate()
    // 防止环境变量绕过 #[validate(range(min = 1, max = 600))] 等约束
    settings
        .validate()
        .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;

    // T005: Validate cross-field invariants (e.g. mem_pressure < mem_critical)
    settings.concurrency.validate();

    info!("Configuration loaded and validated successfully from config sources");
    Ok(settings)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_load_settings_has_tls_fingerprint_engine_config() {
        // T020：`[engines.tls_fingerprint]` 配置段应从 default.toml 正确解析
        let settings = load_settings().expect("Failed to load settings");
        assert_eq!(settings.engines.tls_fingerprint.enabled, false);
        assert_eq!(settings.engines.tls_fingerprint.timeout_seconds, 15);
    }

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

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    #[test]
    fn test_detect_available_port_switches_when_configured_port_occupied() {
        ensure_debug_logger();
        let listener = std::net::TcpListener::bind("0.0.0.0:0").expect("Failed to bind listener");
        let occupied_port = listener.local_addr().unwrap().port();

        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = occupied_port;
        settings.server.enable_port_detection = true;

        let result = detect_available_port(&mut settings);
        assert!(
            result.is_ok(),
            "detect_available_port should succeed by switching ports"
        );
        let detected_port = result.unwrap();
        assert_ne!(
            detected_port, occupied_port,
            "detected port should differ from occupied configured port"
        );
        assert_eq!(
            settings.server.port, detected_port,
            "settings.server.port should be updated to the detected port"
        );
        drop(listener);
    }

    #[test]
    fn test_detect_available_port_error_at_65535_boundary() {
        ensure_debug_logger();
        let _listener = std::net::TcpListener::bind("0.0.0.0:65535").ok();

        let mut settings = load_settings().expect("Failed to load settings");
        settings.server.port = 65535;
        settings.server.enable_port_detection = true;

        let result = detect_available_port(&mut settings);
        assert!(
            result.is_err(),
            "detect_available_port should return error when 65535 is occupied"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to find available port"),
            "error should mention port detection failure, got: {}",
            err_msg
        );
    }
}
