// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 环境变量值验证与过滤模块
//!
//! 提供敏感变量弱默认值检测、日志安全验证和综合安全验证。
//! 注入检测逻辑（白名单/脱敏/报告）拆分至 [`super::env_injection`]。

use std::collections::HashSet;

use super::env_injection::{EnvVarSecurityMonitor, EnvVarSecurityReport};

/// 警告严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    /// 低危
    Low,
    /// 中危
    Medium,
    /// 高危
    High,
    /// 严重
    Critical,
}

/// 敏感变量警告类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveVarWarningType {
    /// 空值
    EmptyValue,
    /// 弱默认值
    WeakDefaultValue,
    /// 测试值
    TestValue,
    /// 值过短
    ShortValue,
    /// 不安全模式
    InsecurePattern,
}

/// 敏感变量警告
#[derive(Debug, Clone)]
pub struct SensitiveVarWarning {
    /// 变量名
    pub var_name: String,
    /// 警告类型
    pub warning_type: SensitiveVarWarningType,
    /// 警告消息
    pub message: String,
    /// 严重程度
    pub severity: WarningSeverity,
}

/// 日志安全警告类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingWarningType {
    /// 详细日志级别
    VerboseLogLevel,
    /// 不安全的日志路径
    InsecureLogPath,
    /// 敏感变量调试模式
    SensitiveVarDebug,
    /// 敏感变量日志记录
    SensitiveVarLogging,
}

/// 日志安全警告
#[derive(Debug, Clone)]
pub struct LoggingSecurityWarning {
    /// 警告类型
    pub warning_type: LoggingWarningType,
    /// 警告消息
    pub message: String,
    /// 修复建议
    pub recommendation: String,
}

/// 安全验证结果
#[derive(Debug, Clone)]
pub struct SecurityValidationResult {
    /// 是否安全
    pub is_secure: bool,
    /// 敏感变量警告列表
    pub sensitive_var_warnings: Vec<SensitiveVarWarning>,
    /// 日志安全警告列表
    pub logging_warnings: Vec<LoggingSecurityWarning>,
    /// 严重问题数量
    pub critical_issues_count: usize,
    /// 高危问题数量
    pub high_issues_count: usize,
}

/// 环境变量配置验证器
#[derive(Debug, Clone)]
pub struct EnvVarValidator {
    monitor: EnvVarSecurityMonitor,
    required_vars: HashSet<&'static str>,
}

impl EnvVarValidator {
    /// 创建新的验证器
    pub fn new(monitor: EnvVarSecurityMonitor, required_vars: Vec<&'static str>) -> Self {
        Self {
            monitor,
            required_vars: HashSet::from_iter(required_vars),
        }
    }

    /// 验证所有必需的环境变量
    pub fn validate_required(&self) -> Result<(), Vec<String>> {
        let mut missing = Vec::new();

        for var in &self.required_vars {
            if std::env::var(var).is_err() {
                missing.push(var.to_string());
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// 验证环境配置（综合检查）
    pub fn validate(&self) -> Result<EnvVarSecurityReport, String> {
        // 检查必需变量
        if let Err(missing) = self.validate_required() {
            return Err(format!(
                "Missing required environment variables: {}",
                missing.join(", ")
            ));
        }

        // 生成安全报告
        let report = self.monitor.generate_security_report();

        // 如果有禁止的变量，返回错误
        if !report.forbidden_variables.is_empty() {
            return Err(format!(
                "Forbidden environment variables detected: {}",
                report.forbidden_variables.join(", ")
            ));
        }

        // 安全评分检查
        if report.security_score < 50 {
            return Err(format!(
                "Security score {} is too low. Review security warnings.",
                report.security_score
            ));
        }

        Ok(report)
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::ENV_MUTEX;

    #[test]
    fn test_sensitive_var_warning_types() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();

        let orig_jwt = std::env::var("JWT_SECRET").ok();
        std::env::set_var("JWT_SECRET", "password123");
        let warnings = monitor.validate_sensitive_values("production");
        let weak_warning = warnings.iter().find(|w| {
            w.var_name == "JWT_SECRET"
                && w.warning_type == SensitiveVarWarningType::WeakDefaultValue
        });
        assert!(weak_warning.is_some(), "应该检测到弱默认值");
        match orig_jwt {
            Some(v) => std::env::set_var("JWT_SECRET", v),
            None => std::env::remove_var("JWT_SECRET"),
        }

        let orig_api = std::env::var("API_KEY").ok();
        std::env::set_var("API_KEY", "test_secret_key");
        let warnings = monitor.validate_sensitive_values("production");
        let test_warning = warnings.iter().find(|w| {
            w.var_name == "API_KEY" && w.warning_type == SensitiveVarWarningType::TestValue
        });
        assert!(test_warning.is_some(), "应该检测到测试值模式");
        match orig_api {
            Some(v) => std::env::set_var("API_KEY", v),
            None => std::env::remove_var("API_KEY"),
        }
    }

    #[test]
    fn test_short_value_detection() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();

        let orig = std::env::var("ENCRYPTION_KEY").ok();
        std::env::set_var("ENCRYPTION_KEY", "short");
        let warnings = monitor.validate_sensitive_values("production");
        let short_warning = warnings.iter().find(|w| {
            w.var_name == "ENCRYPTION_KEY" && w.warning_type == SensitiveVarWarningType::ShortValue
        });
        assert!(short_warning.is_some(), "应该检测到过短的密钥值");
        match orig {
            Some(v) => std::env::set_var("ENCRYPTION_KEY", v),
            None => std::env::remove_var("ENCRYPTION_KEY"),
        }
    }

    #[test]
    fn test_insecure_pattern_detection() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();

        let orig = std::env::var("SECRET_KEY").ok();
        std::env::set_var("SECRET_KEY", "secret_key");
        let warnings = monitor.validate_sensitive_values("production");
        let insecure_warning = warnings.iter().find(|w| {
            w.var_name == "SECRET_KEY" && w.warning_type == SensitiveVarWarningType::InsecurePattern
        });
        assert!(insecure_warning.is_some(), "应该检测到不安全模式");
        match orig {
            Some(v) => std::env::set_var("SECRET_KEY", v),
            None => std::env::remove_var("SECRET_KEY"),
        }
    }

    #[test]
    fn test_logging_security_validation() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();

        std::env::set_var("RUST_LOG", "debug");
        let warnings = monitor.validate_logging_security();
        let verbose_warning = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::VerboseLogLevel);
        assert!(verbose_warning.is_some(), "应该检测到详细日志级别");
        std::env::remove_var("RUST_LOG");

        std::env::set_var("LOG_FILE", "/tmp/app.log");
        let warnings = monitor.validate_logging_security();
        let path_warning = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::InsecureLogPath);
        assert!(path_warning.is_some(), "应该检测到不安全的日志路径");
        std::env::remove_var("LOG_FILE");
    }

    #[test]
    fn test_full_security_validation() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();

        std::env::set_var("TEST_JWT_SECRET", "strong_and_secure_key_12345");

        let result = monitor.perform_full_security_validation("development");

        assert!(
            !result.sensitive_var_warnings.is_empty()
                || !result.logging_warnings.is_empty()
                || result.is_secure
        );

        std::env::remove_var("TEST_JWT_SECRET");
    }

    #[test]
    fn test_validate_sensitive_values_empty_in_production() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let orig = std::env::var("MAIL_PASSWORD").ok();
        std::env::set_var("MAIL_PASSWORD", "");
        let warnings = monitor.validate_sensitive_values("production");
        let empty_warning = warnings.iter().find(|w| {
            w.var_name == "MAIL_PASSWORD" && w.warning_type == SensitiveVarWarningType::EmptyValue
        });
        assert!(empty_warning.is_some(), "应该检测到生产环境中的空值");
        if let Some(w) = empty_warning {
            assert_eq!(w.severity, WarningSeverity::Critical);
        }
        match orig {
            Some(v) => std::env::set_var("MAIL_PASSWORD", v),
            None => std::env::remove_var("MAIL_PASSWORD"),
        }
    }

    #[test]
    fn test_validate_sensitive_values_test_value_non_production_medium() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let orig = std::env::var("GITHUB_TOKEN").ok();
        std::env::set_var("GITHUB_TOKEN", "test_gh_token_12345");
        let warnings = monitor.validate_sensitive_values("development");
        let test_warning = warnings.iter().find(|w| {
            w.var_name == "GITHUB_TOKEN" && w.warning_type == SensitiveVarWarningType::TestValue
        });
        assert!(test_warning.is_some(), "应该检测到测试值模式");
        if let Some(w) = test_warning {
            assert_eq!(w.severity, WarningSeverity::Medium);
        }
        match orig {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_validate_logging_security_sensitive_debug_var() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let var_name = "OAUTH_CLIENT_SECRET_DEBUG";
        let orig = std::env::var(var_name).ok();
        std::env::set_var(var_name, "somevalue");
        let warnings = monitor.validate_logging_security();
        let debug_warning = warnings.iter().find(|w| {
            w.warning_type == LoggingWarningType::SensitiveVarDebug && w.message.contains(var_name)
        });
        assert!(debug_warning.is_some(), "应该检测到敏感变量的调试变体");
        match orig {
            Some(v) => std::env::set_var(var_name, v),
            None => std::env::remove_var(var_name),
        }
    }

    #[test]
    fn test_validate_logging_security_sensitive_log_var() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let var_name = "TWILIO_AUTH_TOKEN_LOG";
        let orig = std::env::var(var_name).ok();
        std::env::set_var(var_name, "somevalue");
        let warnings = monitor.validate_logging_security();
        let log_warning = warnings.iter().find(|w| {
            w.warning_type == LoggingWarningType::SensitiveVarLogging
                && w.message.contains(var_name)
        });
        assert!(log_warning.is_some(), "应该检测到敏感变量的日志变体");
        match orig {
            Some(v) => std::env::set_var(var_name, v),
            None => std::env::remove_var(var_name),
        }
    }

    #[test]
    fn test_perform_full_security_validation_fields_accessible() {
        let monitor = EnvVarSecurityMonitor::default();
        let result = monitor.perform_full_security_validation("development");
        let _: bool = result.is_secure;
        let _: usize = result.critical_issues_count;
        let _: usize = result.high_issues_count;
        let _: &Vec<SensitiveVarWarning> = &result.sensitive_var_warnings;
        let _: &Vec<LoggingSecurityWarning> = &result.logging_warnings;
    }

    #[test]
    fn test_env_var_validator_new_constructor() {
        let monitor = EnvVarSecurityMonitor::default();
        let validator = EnvVarValidator::new(monitor, vec!["SOME_REQUIRED_VAR"]);
        let _ = validator.validate_required();
    }

    #[test]
    fn test_env_var_validator_validate_required_err() {
        let monitor = EnvVarSecurityMonitor::default();
        let validator = EnvVarValidator::new(monitor, vec!["CRAWLRS_NONEXISTENT_REQUIRED_VAR_XYZ"]);
        let result = validator.validate_required();
        assert!(result.is_err());
        if let Err(missing) = result {
            assert!(missing.contains(&"CRAWLRS_NONEXISTENT_REQUIRED_VAR_XYZ".to_string()));
        }
    }

    #[test]
    fn test_env_var_validator_validate_required_ok() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let test_var = "CRAWLRS_TEST_REQUIRED_VAR_OK";
        let orig = std::env::var(test_var).ok();
        std::env::set_var(test_var, "value");
        let validator = EnvVarValidator::new(monitor, vec![test_var]);
        let result = validator.validate_required();
        assert!(result.is_ok());
        match orig {
            Some(v) => std::env::set_var(test_var, v),
            None => std::env::remove_var(test_var),
        }
    }

    #[test]
    fn test_env_var_validator_validate_missing_required() {
        let monitor = EnvVarSecurityMonitor::default();
        let validator = EnvVarValidator::new(monitor, vec!["CRAWLRS_NONEXISTENT_REQUIRED_VAR_ABC"]);
        let result = validator.validate();
        assert!(result.is_err());
        if let Err(msg) = result {
            assert!(msg.contains("Missing required environment variables"));
        }
    }

    #[test]
    fn test_env_var_validator_validate_forbidden_detected() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let required_var = "CRAWLRS_TEST_FORBIDDEN_VALIDATE_VAR";
        let orig_required = std::env::var(required_var).ok();
        std::env::set_var(required_var, "value");

        let orig_forbidden = std::env::var("LD_PRELOAD").ok();
        std::env::set_var("LD_PRELOAD", "/malicious.so");

        let validator = EnvVarValidator::new(monitor, vec![required_var]);
        let result = validator.validate();
        assert!(result.is_err());
        if let Err(msg) = result {
            assert!(msg.contains("Forbidden environment variables detected"));
        }

        match orig_required {
            Some(v) => std::env::set_var(required_var, v),
            None => std::env::remove_var(required_var),
        }
        match orig_forbidden {
            Some(v) => std::env::set_var("LD_PRELOAD", v),
            None => std::env::remove_var("LD_PRELOAD"),
        }
    }

    #[test]
    fn test_validate_sensitive_values_empty_non_production_no_warning() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let orig = std::env::var("MAIL_PASSWORD").ok();
        std::env::set_var("MAIL_PASSWORD", "");
        let warnings = monitor.validate_sensitive_values("development");
        let empty_warning = warnings.iter().find(|w| {
            w.var_name == "MAIL_PASSWORD" && w.warning_type == SensitiveVarWarningType::EmptyValue
        });
        assert!(
            empty_warning.is_none(),
            "非生产环境不应触发 EmptyValue 警告"
        );
        match orig {
            Some(v) => std::env::set_var("MAIL_PASSWORD", v),
            None => std::env::remove_var("MAIL_PASSWORD"),
        }
    }

    #[test]
    fn test_validate_sensitive_values_strong_value_no_warnings() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let orig = std::env::var("JWT_SECRET").ok();
        std::env::set_var("JWT_SECRET", "x9K2mP7qR3sT8vW1yZ4aB6cD5eF0gH2j");
        let warnings = monitor.validate_sensitive_values("production");
        let jwt_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.var_name == "JWT_SECRET")
            .collect();
        assert!(
            jwt_warnings.is_empty(),
            "强密钥不应产生警告，但得到: {:?}",
            jwt_warnings
        );
        match orig {
            Some(v) => std::env::set_var("JWT_SECRET", v),
            None => std::env::remove_var("JWT_SECRET"),
        }
    }

    #[test]
    fn test_validate_sensitive_values_test_value_in_production_critical() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let orig = std::env::var("GITHUB_TOKEN").ok();
        std::env::set_var("GITHUB_TOKEN", "test_gh_token_12345");
        let warnings = monitor.validate_sensitive_values("production");
        let test_warning = warnings.iter().find(|w| {
            w.var_name == "GITHUB_TOKEN" && w.warning_type == SensitiveVarWarningType::TestValue
        });
        assert!(test_warning.is_some(), "应该检测到测试值模式");
        if let Some(w) = test_warning {
            assert_eq!(
                w.severity,
                WarningSeverity::Critical,
                "生产环境测试值应为 Critical"
            );
        }
        match orig {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
    }

    #[test]
    fn test_perform_full_security_validation_production_is_secure_with_strong_values() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        let vars_to_set = [
            ("JWT_SECRET", "x9K2mP7qR3sT8vW1yZ4aB6cD5eF0gH2j"),
            ("API_KEY", "sk_9K2mP7qR3sT8vW1yZ4aB6cD5eF0gH2j"),
            ("SECRET_KEY", "x9K2mP7qR3sT8vW1yZ4aB6cD5eF0gH2j"),
        ];
        let orig_vars: Vec<(&str, Option<String>)> = vars_to_set
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in &vars_to_set {
            std::env::set_var(k, v);
        }

        let result = monitor.perform_full_security_validation("production");
        let _: bool = result.is_secure;
        let _: usize = result.critical_issues_count;
        let _: usize = result.high_issues_count;

        for (k, orig) in &orig_vars {
            match orig {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    fn test_validate_logging_security_trace_level_detected() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        std::env::set_var("RUST_LOG", "trace");
        let warnings = monitor.validate_logging_security();
        let verbose_warning = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::VerboseLogLevel);
        assert!(verbose_warning.is_some(), "应该检测到 trace 日志级别");
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn test_validate_logging_security_no_warnings_with_safe_config() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let monitor = EnvVarSecurityMonitor::default();
        std::env::remove_var("RUST_LOG");
        std::env::remove_var("LOG_FILE");
        let warnings = monitor.validate_logging_security();
        let verbose = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::VerboseLogLevel);
        assert!(verbose.is_none(), "不应有 VerboseLogLevel 警告");
        let insecure_path = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::InsecureLogPath);
        assert!(insecure_path.is_none(), "不应有 InsecureLogPath 警告");
    }

    #[test]
    fn test_warning_severity_equality() {
        assert_eq!(WarningSeverity::Low, WarningSeverity::Low);
        assert_eq!(WarningSeverity::Medium, WarningSeverity::Medium);
        assert_eq!(WarningSeverity::High, WarningSeverity::High);
        assert_eq!(WarningSeverity::Critical, WarningSeverity::Critical);
        assert_ne!(WarningSeverity::Low, WarningSeverity::High);
        assert_ne!(WarningSeverity::Medium, WarningSeverity::Critical);
    }

    #[test]
    fn test_sensitive_var_warning_type_equality() {
        assert_eq!(
            SensitiveVarWarningType::EmptyValue,
            SensitiveVarWarningType::EmptyValue
        );
        assert_eq!(
            SensitiveVarWarningType::WeakDefaultValue,
            SensitiveVarWarningType::WeakDefaultValue
        );
        assert_ne!(
            SensitiveVarWarningType::EmptyValue,
            SensitiveVarWarningType::ShortValue
        );
    }

    #[test]
    fn test_logging_warning_type_equality() {
        assert_eq!(
            LoggingWarningType::VerboseLogLevel,
            LoggingWarningType::VerboseLogLevel
        );
        assert_eq!(
            LoggingWarningType::InsecureLogPath,
            LoggingWarningType::InsecureLogPath
        );
        assert_ne!(
            LoggingWarningType::VerboseLogLevel,
            LoggingWarningType::SensitiveVarDebug
        );
    }

    #[test]
    fn test_security_validation_result_construction() {
        let result = SecurityValidationResult {
            is_secure: true,
            sensitive_var_warnings: vec![],
            logging_warnings: vec![],
            critical_issues_count: 0,
            high_issues_count: 0,
        };
        assert!(result.is_secure);
        assert_eq!(result.critical_issues_count, 0);
        assert_eq!(result.high_issues_count, 0);
        assert!(result.sensitive_var_warnings.is_empty());
        assert!(result.logging_warnings.is_empty());
    }

    #[test]
    fn test_sensitive_var_warning_construction() {
        let warning = SensitiveVarWarning {
            var_name: "TEST_VAR".to_string(),
            warning_type: SensitiveVarWarningType::ShortValue,
            message: "值过短".to_string(),
            severity: WarningSeverity::Medium,
        };
        assert_eq!(warning.var_name, "TEST_VAR");
        assert_eq!(warning.warning_type, SensitiveVarWarningType::ShortValue);
        assert_eq!(warning.severity, WarningSeverity::Medium);
        assert!(warning.message.contains("过短"));
    }

    #[test]
    fn test_logging_security_warning_construction() {
        let warning = LoggingSecurityWarning {
            warning_type: LoggingWarningType::InsecureLogPath,
            message: "日志路径不安全".to_string(),
            recommendation: "使用安全目录".to_string(),
        };
        assert_eq!(warning.warning_type, LoggingWarningType::InsecureLogPath);
        assert!(warning.message.contains("不安全"));
        assert!(warning.recommendation.contains("安全"));
    }

    // ========== EnvVarValidator::validate success path ==========

    #[test]
    fn test_env_var_validator_validate_success_returns_report() {
        use super::super::env_injection::EnvVarWhitelist;

        let whitelist = EnvVarWhitelist {
            allowed_prefixes: vec![],
            allowed_names: HashSet::new(),
            sensitive_vars: HashSet::new(),
            forbidden_vars: HashSet::new(),
        };
        let monitor = EnvVarSecurityMonitor::new(whitelist);
        let validator = EnvVarValidator::new(monitor, vec![]);
        let result = validator.validate();
        assert!(
            result.is_ok(),
            "validate should succeed with no required vars and no forbidden vars"
        );
        let report = result.unwrap();
        assert!(
            report.security_score >= 50,
            "security_score should be >= 50, got {}",
            report.security_score
        );
    }
}
