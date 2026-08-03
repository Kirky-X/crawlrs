// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 环境变量注入检测模块
//!
//! 提供环境变量白名单验证、注入检测、敏感信息脱敏和安全报告生成。
//! 值验证逻辑（弱密钥/测试值/日志安全）拆分至 [`super::env_validation`]。

use log::{debug, error, info, warn};
use std::collections::HashSet;
use std::sync::Arc;

use super::env_validation::{
    LoggingSecurityWarning, LoggingWarningType, SecurityValidationResult, SensitiveVarWarning,
    SensitiveVarWarningType, WarningSeverity,
};

/// 环境变量白名单配置
#[derive(Debug, Clone)]
pub struct EnvVarWhitelist {
    /// 允许的环境变量前缀
    pub allowed_prefixes: Vec<&'static str>,
    /// 允许的精确环境变量名
    pub allowed_names: HashSet<&'static str>,
    /// 敏感环境变量（需要特殊处理）
    pub sensitive_vars: HashSet<&'static str>,
    /// 禁止的环境变量（危险变量）
    pub forbidden_vars: HashSet<&'static str>,
}

impl Default for EnvVarWhitelist {
    fn default() -> Self {
        Self {
            allowed_prefixes: vec!["CRAWLRS_", "APP_", "DATABASE_", "RUST_", "HTTP_", "HTTPS_"],
            allowed_names: HashSet::from([
                // 应用配置
                "APP_ENVIRONMENT",
                "APP_PORT",
                // 数据库配置
                "DB_HOST",
                "DB_PORT",
                "DB_USER",
                "DB_PASSWORD",
                "DB_NAME",
                "DATABASE_URL",
                "DATABASE_MAX_CONNECTIONS",
                // 服务器配置
                "SERVER_HOST",
                "SERVER_PORT",
                // 搜索引擎配置
                "SEARCH_ENGINE_GOOGLE_ENABLED",
                "SEARCH_ENGINE_BING_ENABLED",
                "SEARCH_ENGINE_BAIDU_ENABLED",
                "SEARCH_ENGINE_SOGOU_ENABLED",
                "SEARCH_ENGINE_DEFAULT",
                // FlareSolverr配置
                "FLARESOLVERR_HOST",
                "FLARESOLVERR_PORT",
                "FLARESOLVERR_AUTO_START",
                "FLARESOLVERR_LOG_LEVEL",
                "FLARESOLVERR_CAPTCHA_SOLVER",
                // Chrome配置
                "CHROME_HOST",
                "CHROME_PORT",
                // 速率限制配置
                "RATE_LIMITING_ENABLED",
                "RATE_LIMITING_DEFAULT_RPM",
                // 并发配置
                "CONCURRENCY_DEFAULT_TEAM_LIMIT",
                "CONCURRENCY_TASK_LOCK_DURATION",
                // 监控配置
                "METRICS_ENABLED",
                "METRICS_PORT",
                "PROMETHEUS_PORT",
                // 监控工具密码
                "GRAFANA_PASSWORD",
                "PGADMIN_PASSWORD",
                // 数据卷路径配置
                "DATA_VOLUME_PATH",
                // LLM配置
                "LLM_API_KEY",
                "LLM_MODEL",
                "LLM_API_BASE_URL",
                // 搜索引擎API密钥
                "GOOGLE_SEARCH_API_KEY",
                "GOOGLE_SEARCH_CX",
                "BING_SEARCH_API_KEY",
                "BAIDU_SEARCH_API_KEY",
                "SOGOU_SEARCH_API_KEY",
            ]),
            sensitive_vars: HashSet::from([
                // 数据库敏感变量
                "DB_PASSWORD",
                "DATABASE_URL",
                "DATABASE_PASSWORD",
                // LLM API密钥
                "LLM_API_KEY",
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
                // 搜索引擎API密钥
                "GOOGLE_SEARCH_API_KEY",
                "GOOGLE_SEARCH_CX",
                "BING_SEARCH_API_KEY",
                "BAIDU_SEARCH_API_KEY",
                "SOGOU_SEARCH_API_KEY",
                // 监控工具密码
                "GRAFANA_PASSWORD",
                "PGADMIN_PASSWORD",
                // AWS相关敏感变量
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN",
                "AWS_SECURITY_TOKEN",
                "AWS_SECRET_KEY",
                // S3存储敏感变量
                "S3_ACCESS_KEY_ID",
                "S3_SECRET_ACCESS_KEY",
                "S3_SECRET_KEY",
                // SMTP邮件敏感变量
                "SMTP_PASSWORD",
                "SMTP_USERNAME",
                "MAIL_PASSWORD",
                "MAIL_USERNAME",
                "SENDGRID_API_KEY",
                "MAILGUN_API_KEY",
                // JWT认证敏感变量
                "JWT_SECRET",
                "JWT_SIGNING_KEY",
                "JWT_PRIVATE_KEY",
                "JWT_PUBLIC_KEY",
                // 加密密钥
                "ENCRYPTION_KEY",
                "SECRET_KEY",
                "MASTER_KEY",
                "PRIVATE_KEY",
                // API密钥和秘密
                "API_SECRET",
                "API_KEY",
                "SECRET_TOKEN",
                "ACCESS_TOKEN",
                "REFRESH_TOKEN",
                // 会话相关
                "SESSION_SECRET",
                "SESSION_KEY",
                "COOKIE_SECRET",
                // OAuth相关
                "OAUTH_CLIENT_SECRET",
                "OAUTH_ACCESS_TOKEN",
                "GITHUB_TOKEN",
                "GITLAB_TOKEN",
                // 第三方服务密钥
                "STRIPE_SECRET_KEY",
                "STRIPE_API_KEY",
                "TWILIO_AUTH_TOKEN",
                "TWILIO_ACCOUNT_SID",
                // 代理认证
                "PROXY_PASSWORD",
                "PROXY_USERNAME",
                // 其他敏感配置
                "ADMIN_PASSWORD",
                "ROOT_PASSWORD",
                "SUPERUSER_PASSWORD",
            ]),
            forbidden_vars: HashSet::from([
                // 可能导致安全问题的环境变量
                "CARGO_INCREMENTAL",
                "RUSTFLAGS",
                "LD_PRELOAD",
                "DYLD_INSERT_LIBRARIES",
                // 注意: PATH, HOME, USER, LD_LIBRARY_PATH 已从禁止列表移除
                // 因为这些是标准的系统环境变量，在生产环境中通常需要
                // 如果需要严格限制，可以在特定部署场景中重新添加
            ]),
        }
    }
}

/// 环境变量检查结果
#[derive(Debug, Clone)]
pub enum EnvVarCheckResult {
    /// 允许的环境变量
    Allowed(String),
    /// 敏感环境变量（已脱敏）
    Sensitive { name: String, masked_value: String },
    /// 未知环境变量
    Unknown(String),
    /// 禁止的环境变量
    Forbidden { name: String, reason: String },
}

/// 环境变量安全报告
#[derive(Debug, Clone)]
pub struct EnvVarSecurityReport {
    /// 所有检测到的环境变量
    pub detected_variables: Vec<String>,
    /// 白名单变量
    pub allowed_variables: Vec<String>,
    /// 未知变量（可能需要添加白名单）
    pub unknown_variables: Vec<String>,
    /// 敏感变量（已脱敏处理）
    pub sensitive_variables: Vec<String>,
    /// 危险变量（被阻止使用）
    pub forbidden_variables: Vec<String>,
    /// 安全评分 (0-100)
    pub security_score: u8,
    /// 警告列表
    pub warnings: Vec<String>,
}

/// 环境变量安全监控器
#[derive(Debug, Clone)]
pub struct EnvVarSecurityMonitor {
    whitelist: Arc<EnvVarWhitelist>,
}

impl EnvVarSecurityMonitor {
    /// 创建新的安全监控器
    pub fn new(whitelist: EnvVarWhitelist) -> Self {
        Self {
            whitelist: Arc::new(whitelist),
        }
    }
}

impl Default for EnvVarSecurityMonitor {
    fn default() -> Self {
        Self::new(EnvVarWhitelist::default())
    }
}

// ── 注入检测 & 报告生成 ──────────────────────────────────

impl EnvVarSecurityMonitor {
    /// 检查单个环境变量是否允许/敏感/禁止
    pub fn check_variable(&self, name: &str, value: &str) -> EnvVarCheckResult {
        let name = name.to_uppercase();

        // 检查是否在禁止列表中
        if self.whitelist.forbidden_vars.contains(name.as_str()) {
            error!("Forbidden environment variable detected: {}", name);
            return EnvVarCheckResult::Forbidden {
                name: name.to_string(),
                reason: format!(
                    "Environment variable '{}' is forbidden for security reasons",
                    name
                ),
            };
        }

        // 检查是否为敏感变量
        if self.whitelist.sensitive_vars.contains(name.as_str()) {
            debug!("Sensitive environment variable detected: {}", name);
            return EnvVarCheckResult::Sensitive {
                name: name.to_string(),
                masked_value: self.mask_value(value),
            };
        }

        // 检查是否在允许列表中
        if self.whitelist.allowed_names.contains(name.as_str()) {
            debug!("Allowed environment variable: {}", name);
            return EnvVarCheckResult::Allowed(name.to_string());
        }

        // 检查前缀是否允许
        for prefix in &self.whitelist.allowed_prefixes {
            if name.starts_with(prefix) {
                debug!("Allowed environment variable by prefix: {}", name);
                return EnvVarCheckResult::Allowed(name.to_string());
            }
        }

        // 未知变量
        warn!("Unknown environment variable detected: {}", name);
        EnvVarCheckResult::Unknown(name.to_string())
    }

    /// 对环境变量值进行脱敏处理
    fn mask_value(&self, value: &str) -> String {
        if value.len() <= 4 {
            return "****".to_string();
        }

        let visible_chars = 2;
        let start = &value[..visible_chars];
        let end = &value[value.len() - visible_chars..];
        let masked_length = value.len() - (visible_chars * 2);

        format!("{}*{}{}", start, "*".repeat(masked_length.min(20)), end)
    }

    /// 生成完整的安全报告
    pub fn generate_security_report(&self) -> EnvVarSecurityReport {
        let mut detected = Vec::new();
        let mut allowed = Vec::new();
        let mut unknown = Vec::new();
        let mut sensitive = Vec::new();
        let mut forbidden = Vec::new();
        let mut warnings = Vec::new();

        for (name, value) in std::env::vars() {
            detected.push(name.clone());

            match self.check_variable(&name, &value) {
                EnvVarCheckResult::Allowed(_) => {
                    allowed.push(name);
                }
                EnvVarCheckResult::Sensitive { name, .. } => {
                    sensitive.push(name);
                }
                EnvVarCheckResult::Unknown(name) => {
                    unknown.push(name.clone());
                    warnings.push(format!("Unknown environment variable: {}. Consider adding it to the whitelist if needed.", name));
                }
                EnvVarCheckResult::Forbidden { name, reason } => {
                    forbidden.push(name.clone());
                    warnings.push(format!("CRITICAL: {} - {}", name, reason));
                }
            }
        }

        // 计算安全评分
        let total = detected.len();
        let unknown_count = unknown.len();
        let forbidden_count = forbidden.len();

        let score: i32 = if total == 0 {
            100
        } else {
            let base_score: i32 = 100;
            let unknown_penalty = ((unknown_count as f64 / total as f64) * 20.0) as i32;
            let forbidden_penalty = ((forbidden_count as f64 / total as f64) * 100.0) as i32;
            base_score
                .saturating_sub(unknown_penalty)
                .saturating_sub(forbidden_penalty)
        };

        EnvVarSecurityReport {
            detected_variables: detected,
            allowed_variables: allowed,
            unknown_variables: unknown,
            sensitive_variables: sensitive,
            forbidden_variables: forbidden,
            security_score: score as u8,
            warnings,
        }
    }

    /// 记录安全警告
    pub fn log_security_warnings(&self) {
        let report = self.generate_security_report();

        info!("=== Environment Variable Security Report ===");
        info!("Total variables: {}", report.detected_variables.len());
        info!("Allowed: {}", report.allowed_variables.len());
        info!(
            "Sensitive: {} (masked in logs)",
            report.sensitive_variables.len()
        );
        info!("Unknown: {}", report.unknown_variables.len());
        info!("Forbidden: {}", report.forbidden_variables.len());
        info!("Security Score: {}/100", report.security_score);

        if !report.warnings.is_empty() {
            warn!("Security Warnings:");
            for warning in &report.warnings {
                warn!("  - {}", warning);
            }
        }

        if report.security_score < 70 {
            error!("Security score is below acceptable level! Review the warnings above.");
        }
    }

    /// 获取需要脱敏的环境变量值（用于日志）
    pub fn get_masked_value(&self, name: &str, value: &str) -> String {
        let upper_name = name.to_uppercase();
        if self.whitelist.sensitive_vars.contains(upper_name.as_str()) {
            self.mask_value(value)
        } else {
            value.to_string()
        }
    }
}

// ── 值验证（委托至 env_validation 类型） ──────────────────

impl EnvVarSecurityMonitor {
    /// 验证敏感变量是否安全配置
    pub fn validate_sensitive_values(&self, environment: &str) -> Vec<SensitiveVarWarning> {
        let mut warnings = Vec::new();

        let weak_defaults = [
            "password", "secret", "changeme", "default", "test", "demo", "example", "123456",
            "admin", "root", "qwerty", "letmein", "welcome", "monkey", "dragon",
        ];

        let test_patterns = [
            "test_", "demo_", "example_", "sample_", "fake_", "mock_", "xxx", "yyy", "zzz",
        ];

        for var_name in &self.whitelist.sensitive_vars {
            if let Ok(value) = std::env::var(var_name) {
                let lower_value = value.to_lowercase();

                // 检查空值（生产环境严格要求）
                if value.is_empty() && environment == "production" {
                    warnings.push(SensitiveVarWarning {
                        var_name: var_name.to_string(),
                        warning_type: SensitiveVarWarningType::EmptyValue,
                        message: format!(
                            "敏感环境变量 {} 在生产环境中为空，这可能是一个安全风险",
                            var_name
                        ),
                        severity: WarningSeverity::Critical,
                    });
                    continue;
                }

                // 检查弱默认值
                for weak in &weak_defaults {
                    if lower_value.contains(weak) {
                        warnings.push(SensitiveVarWarning {
                            var_name: var_name.to_string(),
                            warning_type: SensitiveVarWarningType::WeakDefaultValue,
                            message: format!(
                                "敏感环境变量 {} 包含弱默认值模式: '{}'",
                                var_name, weak
                            ),
                            severity: WarningSeverity::High,
                        });
                        break;
                    }
                }

                // 检查测试值模式
                for pattern in &test_patterns {
                    if lower_value.contains(pattern) {
                        warnings.push(SensitiveVarWarning {
                            var_name: var_name.to_string(),
                            warning_type: SensitiveVarWarningType::TestValue,
                            message: format!(
                                "敏感环境变量 {} 包含测试值模式: '{}'",
                                var_name, pattern
                            ),
                            severity: if environment == "production" {
                                WarningSeverity::Critical
                            } else {
                                WarningSeverity::Medium
                            },
                        });
                        break;
                    }
                }

                // 检查过短的密钥值（小于16字符）
                if value.len() < 16 && !value.is_empty() {
                    warnings.push(SensitiveVarWarning {
                        var_name: var_name.to_string(),
                        warning_type: SensitiveVarWarningType::ShortValue,
                        message: format!(
                            "敏感环境变量 {} 的值过短（{} 字符），建议至少使用 32 字符的强密钥",
                            var_name,
                            value.len()
                        ),
                        severity: WarningSeverity::Medium,
                    });
                }

                // 检查是否为常见的不安全模式
                if lower_value == var_name.to_lowercase() {
                    warnings.push(SensitiveVarWarning {
                        var_name: var_name.to_string(),
                        warning_type: SensitiveVarWarningType::InsecurePattern,
                        message: format!(
                            "敏感环境变量 {} 的值与变量名相同，这是一个严重的安全问题",
                            var_name
                        ),
                        severity: WarningSeverity::Critical,
                    });
                }
            }
        }

        warnings
    }

    /// 验证日志配置安全性
    pub fn validate_logging_security(&self) -> Vec<LoggingSecurityWarning> {
        let mut warnings = Vec::new();

        if let Ok(log_level) = std::env::var("RUST_LOG") {
            let lower_level = log_level.to_lowercase();
            if lower_level.contains("debug") || lower_level.contains("trace") {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::VerboseLogLevel,
                    message: format!("日志级别设置为 '{}'，可能会在日志中泄露敏感信息", log_level),
                    recommendation: "建议在生产环境使用 INFO 或 WARN 级别".to_string(),
                });
            }
        }

        if let Ok(log_file) = std::env::var("LOG_FILE") {
            if log_file.starts_with("/tmp") || log_file.starts_with("/var/tmp") {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::InsecureLogPath,
                    message: format!("日志文件路径 '{}' 位于临时目录，可能存在权限问题", log_file),
                    recommendation: "建议将日志文件存储在安全的目录中".to_string(),
                });
            }
        }

        for var_name in &self.whitelist.sensitive_vars {
            let debug_var = format!("{}_DEBUG", var_name);
            let log_var = format!("{}_LOG", var_name);

            if std::env::var(&debug_var).is_ok() {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::SensitiveVarDebug,
                    message: format!(
                        "发现调试变量 '{}'，可能会泄露敏感变量 '{}' 的值",
                        debug_var, var_name
                    ),
                    recommendation: "建议删除此调试变量".to_string(),
                });
            }

            if std::env::var(&log_var).is_ok() {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::SensitiveVarLogging,
                    message: format!(
                        "发现日志变量 '{}'，可能会记录敏感变量 '{}' 的值",
                        log_var, var_name
                    ),
                    recommendation: "建议删除此日志变量".to_string(),
                });
            }
        }

        warnings
    }

    /// 执行完整的安全验证
    pub fn perform_full_security_validation(&self, environment: &str) -> SecurityValidationResult {
        let sensitive_warnings = self.validate_sensitive_values(environment);
        let logging_warnings = self.validate_logging_security();

        let critical_count = sensitive_warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Critical)
            .count();
        let high_count = sensitive_warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::High)
            .count();

        let is_secure = critical_count == 0 && high_count == 0;

        SecurityValidationResult {
            is_secure,
            sensitive_var_warnings: sensitive_warnings,
            logging_warnings,
            critical_issues_count: critical_count,
            high_issues_count: high_count,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::ENV_MUTEX;

    #[test]
    fn test_env_var_check_allowed() {
        let monitor = EnvVarSecurityMonitor::default();
        assert!(matches!(
            monitor.check_variable("APP_ENVIRONMENT", "development"),
            EnvVarCheckResult::Allowed(_)
        ));
    }

    #[test]
    fn test_env_var_check_sensitive() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, masked_value } =
            monitor.check_variable("DB_PASSWORD", "mysecretpassword")
        {
            assert_eq!(name, "DB_PASSWORD");
            assert!(masked_value.contains('*'));
            assert!(!masked_value.contains("mysecretpassword"));
        } else {
            panic!("Expected Sensitive result");
        }
    }

    #[test]
    fn test_env_var_check_forbidden() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Forbidden { name, reason } =
            monitor.check_variable("LD_PRELOAD", "/malicious.so")
        {
            assert_eq!(name, "LD_PRELOAD");
            assert!(reason.contains("forbidden"));
        } else {
            panic!("Expected Forbidden result");
        }
    }

    #[test]
    fn test_mask_value() {
        let monitor = EnvVarSecurityMonitor::default();
        assert_eq!(monitor.mask_value("123"), "****");
        let masked = monitor.mask_value("myverylongpassword123");
        assert!(masked.starts_with("my"));
        assert!(masked.ends_with("23"));
        assert!(masked.contains('*'));
    }

    #[test]
    fn test_aws_credentials_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE")
        {
            assert_eq!(name, "AWS_ACCESS_KEY_ID");
        } else {
            panic!("AWS_ACCESS_KEY_ID 应该被识别为敏感变量");
        }
        if let EnvVarCheckResult::Sensitive { name, .. } = monitor.check_variable(
            "AWS_SECRET_ACCESS_KEY",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        ) {
            assert_eq!(name, "AWS_SECRET_ACCESS_KEY");
        } else {
            panic!("AWS_SECRET_ACCESS_KEY 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_smtp_credentials_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("SMTP_PASSWORD", "smtp_password_123")
        {
            assert_eq!(name, "SMTP_PASSWORD");
        } else {
            panic!("SMTP_PASSWORD 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_jwt_credentials_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("JWT_SECRET", "jwt_super_secret_key")
        {
            assert_eq!(name, "JWT_SECRET");
        } else {
            panic!("JWT_SECRET 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_encryption_key_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("ENCRYPTION_KEY", "encryption_key_123")
        {
            assert_eq!(name, "ENCRYPTION_KEY");
        } else {
            panic!("ENCRYPTION_KEY 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_session_secret_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("SESSION_SECRET", "session_secret_key")
        {
            assert_eq!(name, "SESSION_SECRET");
        } else {
            panic!("SESSION_SECRET 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_oauth_credentials_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("OAUTH_CLIENT_SECRET", "oauth_client_secret")
        {
            assert_eq!(name, "OAUTH_CLIENT_SECRET");
        } else {
            panic!("OAUTH_CLIENT_SECRET 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_third_party_api_keys_detection() {
        let monitor = EnvVarSecurityMonitor::default();
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("STRIPE_SECRET_KEY", "sk_test_123456")
        {
            assert_eq!(name, "STRIPE_SECRET_KEY");
        } else {
            panic!("STRIPE_SECRET_KEY 应该被识别为敏感变量");
        }
        if let EnvVarCheckResult::Sensitive { name, .. } =
            monitor.check_variable("TWILIO_AUTH_TOKEN", "twilio_auth_token")
        {
            assert_eq!(name, "TWILIO_AUTH_TOKEN");
        } else {
            panic!("TWILIO_AUTH_TOKEN 应该被识别为敏感变量");
        }
    }

    #[test]
    fn test_env_var_whitelist_default_fields() {
        let whitelist = EnvVarWhitelist::default();
        assert!(!whitelist.allowed_prefixes.is_empty());
        assert!(whitelist.allowed_prefixes.contains(&"CRAWLRS_"));
        assert!(whitelist.allowed_prefixes.contains(&"APP_"));
        assert!(whitelist.allowed_prefixes.contains(&"DATABASE_"));
        assert!(whitelist.allowed_prefixes.contains(&"RUST_"));
        assert!(whitelist.allowed_prefixes.contains(&"HTTP_"));
        assert!(whitelist.allowed_prefixes.contains(&"HTTPS_"));
        assert!(!whitelist.allowed_names.is_empty());
        assert!(whitelist.allowed_names.contains("APP_ENVIRONMENT"));
        assert!(whitelist.allowed_names.contains("DATABASE_URL"));
        assert!(whitelist.allowed_names.contains("SERVER_PORT"));
        assert!(whitelist.allowed_names.contains("LLM_API_KEY"));
        assert!(!whitelist.sensitive_vars.is_empty());
        assert!(whitelist.sensitive_vars.contains("DB_PASSWORD"));
        assert!(whitelist.sensitive_vars.contains("JWT_SECRET"));
        assert!(whitelist.sensitive_vars.contains("AWS_SECRET_ACCESS_KEY"));
        assert!(whitelist.sensitive_vars.contains("SMTP_PASSWORD"));
        assert!(!whitelist.forbidden_vars.is_empty());
        assert!(whitelist.forbidden_vars.contains("LD_PRELOAD"));
        assert!(whitelist.forbidden_vars.contains("RUSTFLAGS"));
        assert!(whitelist.forbidden_vars.contains("CARGO_INCREMENTAL"));
        assert!(whitelist.forbidden_vars.contains("DYLD_INSERT_LIBRARIES"));
    }

    #[test]
    fn test_env_var_security_monitor_new_constructor() {
        let whitelist = EnvVarWhitelist::default();
        let monitor = EnvVarSecurityMonitor::new(whitelist);
        let result = monitor.check_variable("APP_PORT", "8080");
        assert!(matches!(result, EnvVarCheckResult::Allowed(_)));
    }

    #[test]
    fn test_check_variable_prefix_only_match() {
        let monitor = EnvVarSecurityMonitor::default();
        let result = monitor.check_variable("CRAWLRS_CUSTOM_FEATURE_XYZ", "enabled");
        assert!(matches!(result, EnvVarCheckResult::Allowed(_)));
        if let EnvVarCheckResult::Allowed(name) = result {
            assert_eq!(name, "CRAWLRS_CUSTOM_FEATURE_XYZ");
        }
    }

    #[test]
    fn test_check_variable_unknown() {
        let monitor = EnvVarSecurityMonitor::default();
        let result = monitor.check_variable("ZZZ_UNKNOWN_RANDOM_VAR", "value");
        assert!(matches!(result, EnvVarCheckResult::Unknown(_)));
        if let EnvVarCheckResult::Unknown(name) = result {
            assert_eq!(name, "ZZZ_UNKNOWN_RANDOM_VAR");
        }
    }

    #[test]
    fn test_check_variable_lowercase_input_uppercased() {
        let monitor = EnvVarSecurityMonitor::default();
        let result = monitor.check_variable("db_password", "mysecret");
        assert!(matches!(result, EnvVarCheckResult::Sensitive { .. }));
        if let EnvVarCheckResult::Sensitive { name, .. } = result {
            assert_eq!(name, "DB_PASSWORD");
        }
    }

    #[test]
    fn test_mask_value_exactly_four_chars() {
        let monitor = EnvVarSecurityMonitor::default();
        assert_eq!(monitor.mask_value("abcd"), "****");
        assert_eq!(monitor.mask_value("ab"), "****");
        assert_eq!(monitor.mask_value(""), "****");
    }

    #[test]
    fn test_mask_value_long_value_capped_at_20_stars() {
        let monitor = EnvVarSecurityMonitor::default();
        let long_value = "abcdefghijklmnopqrstuvwxyz0123456789";
        let masked = monitor.mask_value(long_value);
        assert!(masked.starts_with("ab"));
        assert!(masked.ends_with("89"));
        let star_count = masked.matches('*').count();
        assert!(star_count <= 21);
        assert_eq!(star_count, 21);
    }

    #[test]
    fn test_get_masked_value_sensitive() {
        let monitor = EnvVarSecurityMonitor::default();
        let masked = monitor.get_masked_value("DB_PASSWORD", "mysecretpassword");
        assert!(masked.contains('*'));
        assert!(!masked.contains("mysecretpassword"));
    }

    #[test]
    fn test_get_masked_value_non_sensitive() {
        let monitor = EnvVarSecurityMonitor::default();
        let value = monitor.get_masked_value("APP_PORT", "8080");
        assert_eq!(value, "8080");
    }

    #[test]
    fn test_env_var_security_report_construction() {
        let report = EnvVarSecurityReport {
            detected_variables: vec!["VAR1".to_string(), "VAR2".to_string()],
            allowed_variables: vec!["VAR1".to_string()],
            unknown_variables: vec!["VAR2".to_string()],
            sensitive_variables: vec![],
            forbidden_variables: vec![],
            security_score: 85,
            warnings: vec!["test warning".to_string()],
        };
        assert_eq!(report.detected_variables.len(), 2);
        assert_eq!(report.allowed_variables.len(), 1);
        assert_eq!(report.unknown_variables.len(), 1);
        assert!(report.sensitive_variables.is_empty());
        assert!(report.forbidden_variables.is_empty());
        assert_eq!(report.security_score, 85);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_generate_security_report_structure() {
        let monitor = EnvVarSecurityMonitor::default();
        let report = monitor.generate_security_report();
        assert!(report.security_score <= 100);
        let total_classified = report.allowed_variables.len()
            + report.unknown_variables.len()
            + report.sensitive_variables.len()
            + report.forbidden_variables.len();
        assert_eq!(report.detected_variables.len(), total_classified);
    }

    #[test]
    fn test_log_security_warnings_does_not_panic() {
        let monitor = EnvVarSecurityMonitor::default();
        monitor.log_security_warnings();
    }

    #[test]
    fn test_env_var_check_result_variants() {
        let allowed = EnvVarCheckResult::Allowed("VAR".to_string());
        let sensitive = EnvVarCheckResult::Sensitive {
            name: "SECRET".to_string(),
            masked_value: "****".to_string(),
        };
        let unknown = EnvVarCheckResult::Unknown("UNKNOWN".to_string());
        let forbidden = EnvVarCheckResult::Forbidden {
            name: "BAD".to_string(),
            reason: "forbidden".to_string(),
        };
        match allowed {
            EnvVarCheckResult::Allowed(n) => assert_eq!(n, "VAR"),
            _ => panic!("Expected Allowed"),
        }
        match sensitive {
            EnvVarCheckResult::Sensitive { name, masked_value } => {
                assert_eq!(name, "SECRET");
                assert_eq!(masked_value, "****");
            }
            _ => panic!("Expected Sensitive"),
        }
        match unknown {
            EnvVarCheckResult::Unknown(n) => assert_eq!(n, "UNKNOWN"),
            _ => panic!("Expected Unknown"),
        }
        match forbidden {
            EnvVarCheckResult::Forbidden { name, reason } => {
                assert_eq!(name, "BAD");
                assert!(reason.contains("forbidden"));
            }
            _ => panic!("Expected Forbidden"),
        }
    }
}
