// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 值验证 — 配置值的范围与有效性检查。

use super::settings::Settings;

/// 值验证函数
///
/// 验证配置值的有效性
pub fn validate_values(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 验证端口范围
    if settings.server.port == 0 {
        return Err(validator::ValidationError::new("invalid_port"));
    }

    // 验证 A/B 测试权重范围
    if settings.search.variant_b_weight < 0.0 || settings.search.variant_b_weight > 1.0 {
        return Err(validator::ValidationError::new("invalid_variant_b_weight"));
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

    #[test]
    fn test_validate_values_invalid_variant_b_weight_negative() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = -0.1;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "invalid_variant_b_weight"
        );
    }

    #[test]
    fn test_validate_values_invalid_variant_b_weight_above_one() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = 1.5;
        let result = validate_values(&settings);
        assert!(result.is_err());
    }
}
