// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! URL 日志脱敏
//!
//! URL query 参数可能携带 `token` / `api_key` / `session_id` 等敏感信息，
//! 日志会持久化到磁盘。本模块提供统一的脱敏实现，供请求合并、抓取缓存与
//! worker 日志路径共用。
//!
//! 放在 `utils` 而非 `workers`：`workers` 受 `platform` / `web-axum` 特性门控，
//! 而 `utils::coalesce` 在无默认特性下也必须编译；脱敏逻辑与 worker 生命周期
//! 无关，归入通用工具可消除反向依赖。

use url::Url;

/// URL 日志最大长度（防日志膨胀）
const MAX_URL_LOG_LEN: usize = 200;

/// 脱敏 URL 用于日志输出
///
/// 移除 query 和 fragment，仅保留 `scheme://host[:port]/path`，并限制总长度
/// 防止日志膨胀；同时剥离 userinfo（用户名/密码），防止凭据泄露到日志。
///
/// 解析失败时返回 `[invalid-url]` 占位符，绝不原样返回可能含凭证的输入
/// （CWE-532 防护：日志文件中的信息泄露）。
#[must_use]
pub(crate) fn redact_url_for_log(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            // 剥离 userinfo（用户名/密码），防止凭据泄露到日志
            if parsed.username() != "" {
                let _ = parsed.set_username("");
            }
            if parsed.password().is_some() {
                parsed.set_password(None).ok();
            }
            let redacted = parsed.to_string();
            // 限制长度防止日志膨胀（URL 应为 ASCII，但用 chars 截断更安全）
            if redacted.len() > MAX_URL_LOG_LEN {
                let truncated: String = redacted.chars().take(MAX_URL_LOG_LEN).collect();
                format!("{}...", truncated)
            } else {
                redacted
            }
        }
        Err(_) => "[invalid-url]".to_string(),
    }
}

/// webhook URL 日志脱敏：只保留 `scheme://host[:port]`。
///
/// Slack/Discord/Teams 等 webhook 的密钥位于路径中（如
/// `https://hooks.slack.com/services/T000/B000/XXXX`），通用
/// [`redact_url_for_log`] 会保留 path，因此 webhook 日志必须使用本函数。
#[must_use]
pub(crate) fn redact_webhook_url_for_log(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.set_path("");
            if parsed.username() != "" {
                let _ = parsed.set_username("");
            }
            if parsed.password().is_some() {
                parsed.set_password(None).ok();
            }
            format!(
                "{}/[redacted-webhook-path]",
                parsed.as_str().trim_end_matches('/')
            )
        }
        Err(_) => "[invalid-webhook-url]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_query_and_fragment() {
        let url = "https://example.com/path?token=secret&api_key=abc#fragment";
        let redacted = redact_url_for_log(url);
        assert_eq!(redacted, "https://example.com/path");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("abc"));
    }

    #[test]
    fn strips_empty_query_only() {
        let url = "https://example.com?token=secret";
        let redacted = redact_url_for_log(url);
        assert_eq!(redacted, "https://example.com/");
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn webhook_redaction_drops_path_secret() {
        let url = "https://hooks.slack.com/services/T000/B000/SECRET";
        let redacted = redact_webhook_url_for_log(url);
        assert_eq!(redacted, "https://hooks.slack.com/[redacted-webhook-path]");
        assert!(!redacted.contains("SECRET"));
    }

    #[test]
    fn strips_userinfo_credentials() {
        let url = "https://user:password@example.com/path?token=secret"; // pragma: allowlist secret
        let redacted = redact_url_for_log(url);
        assert_eq!(redacted, "https://example.com/path");
        assert!(!redacted.contains("user"));
        assert!(!redacted.contains("password"));
    }

    #[test]
    fn preserves_path_and_port() {
        assert_eq!(
            redact_url_for_log("https://example.com/deep/nested/path"),
            "https://example.com/deep/nested/path"
        );
        assert_eq!(
            redact_url_for_log("http://localhost:8080/api"),
            "http://localhost:8080/api"
        );
    }

    #[test]
    fn invalid_url_returns_placeholder() {
        assert_eq!(redact_url_for_log("not a url"), "[invalid-url]");
    }

    #[test]
    fn truncates_long_url() {
        let long_path = "a".repeat(300);
        let url = format!("https://example.com/{}", long_path);
        let redacted = redact_url_for_log(&url);
        assert!(redacted.ends_with("..."));
        assert!(redacted.len() <= MAX_URL_LOG_LEN + 3); // +3 for "..."
    }
}
