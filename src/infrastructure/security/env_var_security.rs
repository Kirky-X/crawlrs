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
                "DB_PASSWORD",
                "DATABASE_URL",
                "REDIS_URL",
                "LLM_API_KEY",
                "GOOGLE_SEARCH_API_KEY",
                "BING_SEARCH_API_KEY",
                "BAIDU_SEARCH_API_KEY",
                "SOGOU_SEARCH_API_KEY",
                "GRAFANA_PASSWORD",
                "PGADMIN_PASSWORD",
                "S3_ACCESS_KEY_ID",
                "S3_SECRET_ACCESS_KEY",
            ]),
            forbidden_vars: HashSet::from([
                // 可能导致安全问题的环境变量
                "CARGO_INCREMENTAL",
                "RUSTFLAGS",
                "LD_LIBRARY_PATH",
                "LD_PRELOAD",
                "DYLD_INSERT_LIBRARIES",
                "PATH", // 限制PATH修改
                "HOME", // 避免HOME操纵
                "USER", // 避免用户信息操纵
                "USERNAME",
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
}
