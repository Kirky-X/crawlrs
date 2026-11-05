// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 值验证 — 配置值的范围与有效性检查。

use super::settings::Settings;

/// 值验证函数
///
/// 验证配置值的有效性
pub fn validate_values(settings: &Settings) -> Result<(), sdforge::validator::ValidationError> {
    if settings.server.port == 0 {
        return Err(sdforge::validator::ValidationError::new("invalid_port"));
    }

    // 内存水位阈值单调性：tight 必须严格大于 critical，否则三档判定无意义。
    // 无条件校验（即便调度器关闭也提前暴露错误配置，而非运行期静默错档）。
    let ms = &settings.engines.memory_scheduler;
    if ms.tight_available_mib <= ms.critical_available_mib {
        return Err(sdforge::validator::ValidationError::new(
            "invalid_memory_scheduler_thresholds",
        ));
    }
    // 采样周期下界：低于 100ms 的 sysinfo 刷新近乎 busy-loop，纯耗 CPU
    if ms.enabled && ms.refresh_interval_ms < 100 {
        return Err(sdforge::validator::ValidationError::new(
            "invalid_memory_scheduler_refresh_interval",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::Settings;

    fn build_test_settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn test_validate_values_valid_settings() {
        let settings = build_test_settings();
        assert!(validate_values(&settings).is_ok());
    }

    #[test]
    fn test_validate_values_invalid_port_zero() {
        let mut settings = build_test_settings();
        settings.server.port = 0;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "invalid_port");
    }

    /// tight <= critical 的内存水位阈值在配置校验期被拒（含相等）
    #[test]
    fn test_validate_values_memory_scheduler_thresholds_inverted() {
        let mut settings = build_test_settings();
        settings.engines.memory_scheduler.critical_available_mib = 2048;
        settings.engines.memory_scheduler.tight_available_mib = 512;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "invalid_memory_scheduler_thresholds"
        );
    }

    /// refresh_interval_ms < 100 且启用时被拒（busy-loop 防护）
    #[test]
    fn test_validate_values_memory_scheduler_refresh_interval_too_small() {
        let mut settings = build_test_settings();
        settings.engines.memory_scheduler.refresh_interval_ms = 1;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "invalid_memory_scheduler_refresh_interval"
        );
        // 调度器关闭时不校验（值不生效）
        settings.engines.memory_scheduler.refresh_interval_ms = 1;
        settings.engines.memory_scheduler.enabled = false;
        assert!(validate_values(&settings).is_ok());
    }

    /// 相等阈值同样被拒（`<` 语义下 tight == critical 会吞掉 Tight 档）
    #[test]
    fn test_validate_values_memory_scheduler_thresholds_equal_rejected() {
        let mut settings = build_test_settings();
        settings.engines.memory_scheduler.critical_available_mib = 1024;
        settings.engines.memory_scheduler.tight_available_mib = 1024;
        assert!(validate_values(&settings).is_err());
    }
}
