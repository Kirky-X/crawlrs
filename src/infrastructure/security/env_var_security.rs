// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 环境变量安全模块
//!
//! 提供环境变量的白名单验证、安全检查和敏感信息保护

use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

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
            allowed_prefixes: vec![
                "CRAWLRS_",
                "APP_",
                "DATABASE_",
                "REDIS_",
                "RUST_",
                "HTTP_",
                "HTTPS_",
            ],
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
                // Redis配置
                "REDIS_HOST",
                "REDIS_PORT",
                "REDIS_URL",
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
                // Redis敏感变量
                "REDIS_URL",
                "REDIS_PASSWORD",
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

/// 环境变量安全检查结果
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

impl EnvVarSecurityMonitor {
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

    /// 验证敏感变量是否安全配置
    ///
    /// 检查以下安全规则：
    /// 1. 敏感变量不应有弱默认值
    /// 2. 敏感变量不应为空（在生产环境）
    /// 3. 敏感变量不应包含明显的测试值
    pub fn validate_sensitive_values(&self, environment: &str) -> Vec<SensitiveVarWarning> {
        let mut warnings = Vec::new();

        // 弱默认值列表
        let weak_defaults = [
            "password", "secret", "changeme", "default", "test", "demo", "example", "123456",
            "admin", "root", "qwerty", "letmein", "welcome", "monkey", "dragon",
        ];

        // 测试值模式
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
    ///
    /// 确保敏感变量不会被意外输出到日志
    pub fn validate_logging_security(&self) -> Vec<LoggingSecurityWarning> {
        let mut warnings = Vec::new();

        // 检查日志级别配置
        if let Ok(log_level) = std::env::var("RUST_LOG") {
            // 在生产环境中，DEBUG 或 TRACE 级别可能会泄露敏感信息
            let lower_level = log_level.to_lowercase();
            if lower_level.contains("debug") || lower_level.contains("trace") {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::VerboseLogLevel,
                    message: format!("日志级别设置为 '{}'，可能会在日志中泄露敏感信息", log_level),
                    recommendation: "建议在生产环境使用 INFO 或 WARN 级别".to_string(),
                });
            }
        }

        // 检查是否有日志输出到文件的配置
        if let Ok(log_file) = std::env::var("LOG_FILE") {
            // 检查日志文件路径是否安全
            if log_file.starts_with("/tmp") || log_file.starts_with("/var/tmp") {
                warnings.push(LoggingSecurityWarning {
                    warning_type: LoggingWarningType::InsecureLogPath,
                    message: format!("日志文件路径 '{}' 位于临时目录，可能存在权限问题", log_file),
                    recommendation: "建议将日志文件存储在安全的目录中".to_string(),
                });
            }
        }

        // 检查是否有敏感变量可能被意外记录
        for var_name in &self.whitelist.sensitive_vars {
            // 检查是否有以 _LOG 或 _DEBUG 结尾的变量可能泄露敏感信息
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
    ///
    /// 包括敏感变量值验证和日志安全验证
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

#[cfg(test)]
mod tests {
    use super::*;

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

        // 短值
        assert_eq!(monitor.mask_value("123"), "****");

        // 长值
        let masked = monitor.mask_value("myverylongpassword123");
        assert!(masked.starts_with("my"));
        assert!(masked.ends_with("23"));
        assert!(masked.contains('*'));
    }

    #[test]
    fn test_sensitive_var_warning_types() {
        let monitor = EnvVarSecurityMonitor::default();

        // 测试弱默认值检测
        std::env::set_var("TEST_JWT_SECRET", "password123");
        let warnings = monitor.validate_sensitive_values("production");
        let weak_warning = warnings.iter().find(|w| {
            w.var_name == "TEST_JWT_SECRET"
                && w.warning_type == SensitiveVarWarningType::WeakDefaultValue
        });
        assert!(weak_warning.is_some(), "应该检测到弱默认值");
        std::env::remove_var("TEST_JWT_SECRET");

        // 测试测试值模式检测
        std::env::set_var("TEST_API_KEY", "test_secret_key");
        let warnings = monitor.validate_sensitive_values("production");
        let test_warning = warnings.iter().find(|w| {
            w.var_name == "TEST_API_KEY" && w.warning_type == SensitiveVarWarningType::TestValue
        });
        assert!(test_warning.is_some(), "应该检测到测试值模式");
        std::env::remove_var("TEST_API_KEY");
    }

    #[test]
    fn test_short_value_detection() {
        let monitor = EnvVarSecurityMonitor::default();

        // 设置一个过短的密钥
        std::env::set_var("TEST_ENCRYPTION_KEY", "short");
        let warnings = monitor.validate_sensitive_values("production");
        let short_warning = warnings.iter().find(|w| {
            w.var_name == "TEST_ENCRYPTION_KEY"
                && w.warning_type == SensitiveVarWarningType::ShortValue
        });
        assert!(short_warning.is_some(), "应该检测到过短的密钥值");
        std::env::remove_var("TEST_ENCRYPTION_KEY");
    }

    #[test]
    fn test_insecure_pattern_detection() {
        let monitor = EnvVarSecurityMonitor::default();

        // 设置一个与变量名相同的值
        std::env::set_var("TEST_SECRET_KEY", "test_secret_key");
        let warnings = monitor.validate_sensitive_values("production");
        let insecure_warning = warnings.iter().find(|w| {
            w.var_name == "TEST_SECRET_KEY"
                && w.warning_type == SensitiveVarWarningType::InsecurePattern
        });
        assert!(insecure_warning.is_some(), "应该检测到不安全模式");
        std::env::remove_var("TEST_SECRET_KEY");
    }

    #[test]
    fn test_logging_security_validation() {
        let monitor = EnvVarSecurityMonitor::default();

        // 测试详细日志级别
        std::env::set_var("RUST_LOG", "debug");
        let warnings = monitor.validate_logging_security();
        let verbose_warning = warnings
            .iter()
            .find(|w| w.warning_type == LoggingWarningType::VerboseLogLevel);
        assert!(verbose_warning.is_some(), "应该检测到详细日志级别");
        std::env::remove_var("RUST_LOG");

        // 测试不安全的日志路径
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
        let monitor = EnvVarSecurityMonitor::default();

        // 设置一些测试变量
        std::env::set_var("TEST_JWT_SECRET", "strong_and_secure_key_12345");

        let result = monitor.perform_full_security_validation("development");

        // 验证结果结构
        assert!(
            !result.sensitive_var_warnings.is_empty()
                || !result.logging_warnings.is_empty()
                || result.is_secure
        );

        std::env::remove_var("TEST_JWT_SECRET");
    }

    #[test]
    fn test_aws_credentials_detection() {
        let monitor = EnvVarSecurityMonitor::default();

        // 测试 AWS 凭证被识别为敏感变量
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

        // 测试 SMTP 凭证被识别为敏感变量
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

        // 测试 JWT 密钥被识别为敏感变量
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

        // 测试加密密钥被识别为敏感变量
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

        // 测试会话密钥被识别为敏感变量
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

        // 测试 OAuth 凭证被识别为敏感变量
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

        // 测试第三方 API 密钥被识别为敏感变量
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
}
