// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 安全验证 — 配置的安全性检查（webhook secret、数据库密码、JWT 等）。

use super::settings::Settings;

/// 安全验证函数
///
/// 验证配置的安全性要求，包括 webhook secret、数据库密码、S3 凭据等
pub fn validate_security(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 检查 webhook secret 是否为空
    if settings.webhook.secret().is_empty() {
        return Err(validator::ValidationError::new("webhook_secret_empty"));
    }

    // 检查 webhook secret 是否使用默认值
    let weak_secrets = [
        "your-webhook-secret",
        "your-secret-key",
        "secret",
        "webhook-secret",
        "change-me",
        "password",
    ];
    if weak_secrets.contains(&settings.webhook.secret()) {
        return Err(validator::ValidationError::new("webhook_secret_weak"));
    }

    // 检查 webhook secret 长度
    if settings.webhook.secret().len() < 32 {
        return Err(validator::ValidationError::new("webhook_secret_short"));
    }

    // 检查速率限制是否禁用
    if !settings.rate_limiting.enabled {
        return Err(validator::ValidationError::new("rate_limiting_disabled"));
    }

    // 检查数据库密码
    let weak_patterns = ["password=password", "password=postgres", "password=admin"];
    if weak_patterns
        .iter()
        .any(|p| settings.database.url().contains(p))
    {
        return Err(validator::ValidationError::new("database_password_weak"));
    }

    // 生产环境密码长度验证
    let env = std::env::var("APP_ENVIRONMENT")
        .or_else(|_| std::env::var("CRAWLRS_ENV"))
        .unwrap_or_else(|_| "development".to_string());
    let is_production = env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");

    if is_production {
        let password_length = extract_password_length(settings.database.url());
        if password_length > 0 && password_length < 16 {
            return Err(validator::ValidationError::new(
                "database_password_short_production",
            ));
        }
    }

    // JWT secret 校验（auth-on 时强制，MEDIUM-3 修复：早期失败反馈）
    //
    // 仅在 `auth` feature 启用时检查——auth-off 走 default_identity_middleware，
    // 不读取 jwt_secret。空 / 弱密钥（< 32 字节）会被 `build_garrison_config` 二次拒绝，
    // 此处提前检查让运维在启动时即收到反馈而非等到 garrison 初始化。
    #[cfg(feature = "auth")]
    {
        let jwt_secret = settings.auth.jwt_secret();
        if jwt_secret.is_empty() {
            return Err(validator::ValidationError::new("auth_jwt_secret_empty"));
        }
        if jwt_secret.len() < 32 {
            return Err(validator::ValidationError::new("auth_jwt_secret_weak"));
        }
    }

    Ok(())
}

fn extract_password_length(url: &str) -> usize {
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].find(':') {
            return at_pos - colon_pos - 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::ENV_MUTEX;
    use crate::config::settings::Settings;

    // MEDIUM-3 修复后 validate_security 在 auth-on 时校验 jwt_secret，
    // 测试用强密钥（32+ 字节）确保 validate_security 通过
    fn build_test_settings() -> Settings {
        let mut settings = Settings::default();
        settings.webhook.secret = "a-very-strong-webhook-secret-that-is-32-chars!!".to_string();
        #[cfg(feature = "auth")]
        {
            settings.auth.jwt_secret = "a-very-strong-jwt-secret-that-is-32-chars!!".to_string();
        }
        settings
    }

    #[test]
    fn test_validate_security_valid_settings() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::var("APP_ENVIRONMENT").ok();
        std::env::set_var("APP_ENVIRONMENT", "development");

        let settings = build_test_settings();
        assert!(validate_security(&settings).is_ok());

        if let Some(v) = saved {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
    }

    #[test]
    fn test_validate_security_empty_webhook_secret() {
        let mut settings = build_test_settings();
        settings.webhook.secret = String::new();
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_empty");
    }

    #[test]
    fn test_validate_security_weak_webhook_secret() {
        let mut settings = build_test_settings();
        settings.webhook.secret = "your-webhook-secret".to_string();
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_weak");
    }

    #[test]
    fn test_validate_security_short_webhook_secret() {
        let mut settings = build_test_settings();
        settings.webhook.secret = "short-secret".to_string();
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_short");
    }

    #[test]
    fn test_validate_security_rate_limiting_disabled() {
        let mut settings = build_test_settings();
        settings.rate_limiting.enabled = false;
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "rate_limiting_disabled"
        );
    }

    #[test]
    fn test_validate_security_weak_database_password() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut settings = build_test_settings();
        settings.database.url = "postgres://user:password=password@localhost:5432/db".to_string();
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "database_password_weak"
        );
    }

    #[cfg(feature = "auth")]
    #[test]
    fn test_validate_security_empty_jwt_secret_in_auth_mode() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut settings = build_test_settings();
        settings.auth.jwt_secret = String::new();
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "auth_jwt_secret_empty"
        );
    }
}
