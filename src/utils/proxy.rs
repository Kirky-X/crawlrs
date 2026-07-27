// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理 URL 工具：校验 + 脱敏
//!
//! 设计目的：
//! - **校验**：防止命令行参数注入（安全审查 H-1，playwright `--proxy-server` 拼接漏洞）。
//!   严格校验 proxy URL 必须是合法 URL 且 scheme 在白名单内（http/https/socks5/socks4），
//!   避免 `format!("--proxy-server={}", proxy)` 被 Chrome 解析为多个 argv。
//! - **脱敏**：日志中输出代理 URL 时屏蔽 `user:pass@` 凭证，避免凭证泄露。
//!
//! 复用：原 `flare_solverr.rs::redact_proxy_url` 移植至此共享（规则7 先读再写）。

use url::Url;

/// 占位符：URL 解析失败时返回，绝不原样返回可能携带凭证的输入字符串
pub const INVALID_PLACEHOLDER: &str = "[INVALID-PROXY-URL]";

/// 代理 URL 校验错误
#[derive(Debug, thiserror::Error)]
pub enum ProxyUrlError {
    /// URL 格式不合法（url::Url::parse 失败）
    #[error("invalid URL format: {0}")]
    InvalidFormat(String),
    /// URL 缺少 host
    #[error("missing host in proxy URL")]
    MissingHost,
    /// scheme 不在白名单
    #[error("unsupported scheme '{0}': allowed {1:?}")]
    UnsupportedScheme(String, &'static [&'static str]),
    /// URL 包含空白字符（可能被 shell/argv 解析为参数分隔符）
    #[error("URL contains whitespace")]
    ContainsWhitespace,
}

/// 校验 proxy URL — 严格白名单模式
///
/// 用于 Playwright/Chrome `--proxy-server=<url>` 等命令行参数拼接场景，
/// 防止参数注入（如 `"http://x --enable-bad-flag"` 被 Chrome 拆为多个 argv）。
///
/// # 校验规则
///
/// 1. 通过 `url::Url::parse` 解析成功
/// 2. URL 不含空白字符（`\s`，包括空格/Tab/换行）
/// 3. scheme 在 `allowed_schemes` 白名单内
/// 4. host 非空
///
/// # 参数
///
/// - `proxy_url`: 待校验的 proxy URL 字符串
/// - `allowed_schemes`: 允许的 scheme 列表（如 `&["http", "https", "socks5", "socks4"]`）
///
/// # 返回
///
/// - `Ok(String)`: 校验通过，返回规范化后的 URL 字符串
/// - `Err(ProxyUrlError)`: 校验失败原因
///
/// # 示例
///
/// ```
/// # use crawlrs::utils::proxy::{validate_proxy_url, ProxyUrlError};
/// const ALLOWED: &[&str] = &["http", "https", "socks5", "socks4"];
///
/// assert!(validate_proxy_url("http://proxy.example.com:8080", ALLOWED).is_ok());
/// assert!(validate_proxy_url("socks5://user:pass@host:1080", ALLOWED).is_ok());
///
/// match validate_proxy_url("file:///etc/passwd", ALLOWED) {
///     Err(ProxyUrlError::UnsupportedScheme(s, _)) => assert_eq!(s, "file"),
///     other => panic!("expected UnsupportedScheme, got {other:?}"),
/// }
/// ```
#[must_use]
pub fn validate_proxy_url(
    proxy_url: &str,
    allowed_schemes: &'static [&'static str],
) -> Result<String, ProxyUrlError> {
    // 1. 白名单检查 scheme（先于 parse，避免依赖 url crate 的 scheme 解析行为）
    //    scheme 必须是 ASCII alpha 开头，由 [a-zA-Z0-9+.-] 组成
    let scheme_end = proxy_url
        .find("://")
        .ok_or_else(|| ProxyUrlError::InvalidFormat("missing '://' separator".to_string()))?;
    let scheme = &proxy_url[..scheme_end];
    if scheme.is_empty() {
        return Err(ProxyUrlError::InvalidFormat("empty scheme".to_string()));
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return Err(ProxyUrlError::InvalidFormat(format!(
            "invalid scheme characters: {scheme}"
        )));
    }
    if !allowed_schemes.contains(&scheme) {
        return Err(ProxyUrlError::UnsupportedScheme(
            scheme.to_string(),
            allowed_schemes,
        ));
    }

    // 2. 空白字符检查（防止 argv 分隔）
    if proxy_url.chars().any(|c| c.is_whitespace()) {
        return Err(ProxyUrlError::ContainsWhitespace);
    }

    // 3. 完整 URL 解析（验证 host 存在、port 合法等）
    let parsed = Url::parse(proxy_url)
        .map_err(|e| ProxyUrlError::InvalidFormat(e.to_string()))?;

    // 4. host 必须非空
    if parsed.host_str().is_none() || parsed.host_str().map(|h| h.is_empty()).unwrap_or(true) {
        return Err(ProxyUrlError::MissingHost);
    }

    Ok(proxy_url.to_string())
}

/// 脱敏代理 URL 中的凭证信息
///
/// 若代理 URL 含 `user:pass@host` 形式的 userinfo，将其替换为 `***@host`，
/// 防止日志泄露凭证。
///
/// # 失败降级策略
///
/// URL 解析失败时返回 [`INVALID_PLACEHOLDER`]（`[INVALID-PROXY-URL]`）占位符，
/// 绝不原样返回可能携带凭证的输入字符串，避免日志泄露风险。
///
/// # 示例
///
/// ```
/// # use crawlrs::utils::proxy::redact_proxy_url;
/// assert_eq!(redact_proxy_url("http://proxy.example.com:8080"), "http://proxy.example.com:8080");
/// assert_eq!(redact_proxy_url("http://user:secret@proxy.example.com:8080"), "http://***@proxy.example.com:8080");
/// assert_eq!(redact_proxy_url("not a url"), "[INVALID-PROXY-URL]");
/// ```
#[must_use]
pub fn redact_proxy_url(proxy_url: &str) -> String {
    let parsed = match Url::parse(proxy_url) {
        Ok(u) => u,
        Err(_) => return INVALID_PLACEHOLDER.to_string(),
    };

    let username = parsed.username();
    let password = parsed.password();

    if username.is_empty() && password.is_none() {
        // 无凭证，原样返回
        return proxy_url.to_string();
    }

    // 重建脱敏 URL：保留 scheme/host/port/path，替换 userinfo
    let mut redacted = String::new();
    redacted.push_str(parsed.scheme());
    redacted.push_str("://***@");

    if let Some(host_str) = parsed.host_str() {
        redacted.push_str(host_str);
    }
    if let Some(port) = parsed.port() {
        redacted.push_str(&format!(":{}", port));
    }
    // 仅在 path 非空且非默认 "/" 时追加（url::Url::parse 会将无路径 URL
    // 规范化为带尾 "/"，追加它会污染脱敏输出，例如 "host:8080/"）
    let path = parsed.path();
    if !path.is_empty() && path != "/" {
        redacted.push_str(path);
    }
    if let Some(query) = parsed.query() {
        redacted.push_str(&format!("?{}", query));
    }
    if let Some(fragment) = parsed.fragment() {
        redacted.push_str(&format!("#{}", fragment));
    }

    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Playwright/Chrome 允许的 proxy scheme 白名单
    const CHROME_ALLOWED: &[&str] = &["http", "https", "socks5", "socks4"];

    // =========================================================================
    // validate_proxy_url 测试
    // =========================================================================

    #[test]
    fn validate_proxy_url_http_passes() {
        assert!(validate_proxy_url("http://proxy.example.com:8080", CHROME_ALLOWED).is_ok());
    }

    #[test]
    fn validate_proxy_url_https_passes() {
        assert!(validate_proxy_url("https://proxy.example.com:443", CHROME_ALLOWED).is_ok());
    }

    #[test]
    fn validate_proxy_url_socks5_passes() {
        assert!(validate_proxy_url("socks5://proxy.example.com:1080", CHROME_ALLOWED).is_ok());
    }

    #[test]
    fn validate_proxy_url_socks4_passes() {
        assert!(validate_proxy_url("socks4://proxy.example.com:1080", CHROME_ALLOWED).is_ok());
    }

    #[test]
    fn validate_proxy_url_with_credentials_passes() {
        assert!(
            validate_proxy_url("http://user:secret@proxy.example.com:8080", CHROME_ALLOWED)
                .is_ok()
        );
    }

    #[test]
    fn validate_proxy_url_with_ipv6_passes() {
        assert!(
            validate_proxy_url("http://[::1]:8080", CHROME_ALLOWED).is_ok(),
            "IPv6 host should pass"
        );
    }

    #[test]
    fn validate_proxy_url_with_path_and_query_passes() {
        assert!(
            validate_proxy_url("http://proxy.example.com:8080/path?q=1", CHROME_ALLOWED).is_ok()
        );
    }

    #[test]
    fn validate_proxy_url_rejects_file_scheme() {
        match validate_proxy_url("file:///etc/passwd", CHROME_ALLOWED) {
            Err(ProxyUrlError::UnsupportedScheme(s, allowed)) => {
                assert_eq!(s, "file");
                assert_eq!(allowed, CHROME_ALLOWED);
            }
            other => panic!("expected UnsupportedScheme, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_ftp_scheme() {
        match validate_proxy_url("ftp://proxy.example.com:21", CHROME_ALLOWED) {
            Err(ProxyUrlError::UnsupportedScheme(s, _)) => assert_eq!(s, "ftp"),
            other => panic!("expected UnsupportedScheme, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_gopher_scheme() {
        match validate_proxy_url("gopher://proxy.example.com:70", CHROME_ALLOWED) {
            Err(ProxyUrlError::UnsupportedScheme(s, _)) => assert_eq!(s, "gopher"),
            other => panic!("expected UnsupportedScheme, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_whitespace_in_url() {
        // 空格注入测试：`"http://x --enable-bad-flag"` 应被拒绝
        match validate_proxy_url("http://evil.com --enable-bad-flag", CHROME_ALLOWED) {
            Err(ProxyUrlError::ContainsWhitespace) => {}
            other => panic!("expected ContainsWhitespace, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_tab_in_url() {
        match validate_proxy_url("http://evil.com\t--bad", CHROME_ALLOWED) {
            Err(ProxyUrlError::ContainsWhitespace) => {}
            other => panic!("expected ContainsWhitespace, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_newline_in_url() {
        match validate_proxy_url("http://evil.com\n--bad", CHROME_ALLOWED) {
            Err(ProxyUrlError::ContainsWhitespace) => {}
            other => panic!("expected ContainsWhitespace, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_missing_scheme_separator() {
        match validate_proxy_url("proxy.example.com:8080", CHROME_ALLOWED) {
            Err(ProxyUrlError::InvalidFormat(_)) => {}
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_empty_scheme() {
        match validate_proxy_url("://example.com", CHROME_ALLOWED) {
            Err(ProxyUrlError::InvalidFormat(_)) => {}
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_empty_input() {
        assert!(validate_proxy_url("", CHROME_ALLOWED).is_err());
    }

    #[test]
    fn validate_proxy_url_rejects_missing_host() {
        // scheme 合法但无 host：url::Url::parse 对 http/https 等要求 host 非空的
        // scheme 直接返回 InvalidFormat("empty host")，MissingHost 变体作为防御性
        // 兜底（若未来 url crate 行为变化或扩展到不强制 host 的 scheme）
        let result = validate_proxy_url("http://", CHROME_ALLOWED);
        assert!(
            matches!(
                result,
                Err(ProxyUrlError::InvalidFormat(_)) | Err(ProxyUrlError::MissingHost)
            ),
            "expected InvalidFormat or MissingHost, got {result:?}"
        );
    }

    #[test]
    fn validate_proxy_url_rejects_invalid_scheme_chars() {
        // scheme 含非法字符（如空格）
        match validate_proxy_url("ht tp://example.com", CHROME_ALLOWED) {
            Err(ProxyUrlError::InvalidFormat(_)) => {}
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_rejects_invalid_url_format() {
        match validate_proxy_url("http://[invalid-ipv6", CHROME_ALLOWED) {
            Err(ProxyUrlError::InvalidFormat(_)) => {}
            other => panic!("expected InvalidFormat, got {other:?}"),
        }
    }

    #[test]
    fn validate_proxy_url_preserves_input_string_in_ok() {
        // 校验通过的 URL 应原样返回（不规范化），保持可预测性
        let input = "socks5://user:pass@proxy.example.com:1080";
        let result = validate_proxy_url(input, CHROME_ALLOWED).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn validate_proxy_url_custom_scheme_whitelist() {
        // FlareSolverr 只允许 http/https
        const FLARESOLVERR_ALLOWED: &[&str] = &["http", "https"];
        assert!(validate_proxy_url("http://x.com", FLARESOLVERR_ALLOWED).is_ok());
        assert!(validate_proxy_url("https://x.com", FLARESOLVERR_ALLOWED).is_ok());
        assert!(validate_proxy_url("socks5://x.com", FLARESOLVERR_ALLOWED).is_err());
    }

    // =========================================================================
    // redact_proxy_url 测试（移植自 flare_solverr.rs::redact_proxy_url_tests）
    // =========================================================================

    #[test]
    fn redact_proxy_url_no_credentials() {
        // 无凭证的代理 URL 应原样返回
        assert_eq!(
            redact_proxy_url("http://proxy.example.com:8080"),
            "http://proxy.example.com:8080"
        );
    }

    #[test]
    fn redact_proxy_url_with_user_only() {
        // 仅有用户名的代理 URL 应脱敏用户名
        assert_eq!(
            redact_proxy_url("http://user@proxy.example.com:8080"),
            "http://***@proxy.example.com:8080"
        );
    }

    #[test]
    fn redact_proxy_url_with_user_and_password() {
        // 含 user:pass 的代理 URL 应完全脱敏 userinfo
        assert_eq!(
            redact_proxy_url("http://user:secret@proxy.example.com:8080"),
            "http://***@proxy.example.com:8080"
        );
    }

    #[test]
    fn redact_proxy_url_invalid_url_returns_placeholder() {
        // 安全要求：解析失败的 URL 绝不原样返回（防止泄露凭证），
        // 必须返回 [INVALID-PROXY-URL] 占位符
        assert_eq!(redact_proxy_url("not a url at all"), INVALID_PLACEHOLDER);
    }

    #[test]
    fn redact_proxy_url_preserves_path_and_query() {
        // 脱敏后应保留 path 和 query
        assert_eq!(
            redact_proxy_url("http://user:pass@proxy.example.com:8080/path?q=1"),
            "http://***@proxy.example.com:8080/path?q=1"
        );
    }

    #[test]
    fn redact_proxy_url_https() {
        // HTTPS 协议应正确处理
        // 注意：url::Url::parse 会规范化掉默认端口（443 for https）
        assert_eq!(
            redact_proxy_url("https://user:pass@proxy.example.com:443"),
            "https://***@proxy.example.com"
        );
    }

    #[test]
    fn redact_proxy_url_ipv6() {
        // IPv6 主机应保留方括号格式
        assert_eq!(
            redact_proxy_url("http://user:pass@[::1]:8080"),
            "http://***@[::1]:8080"
        );
    }

    #[test]
    fn redact_proxy_url_url_encoded_credentials() {
        // URL 编码的凭证（含特殊字符）应被脱敏（不保留原始编码值）
        assert_eq!(
            redact_proxy_url("http://us%40er:p%40ss@proxy.example.com:8080"),
            "http://***@proxy.example.com:8080"
        );
    }

    #[test]
    fn redact_proxy_url_preserves_fragment() {
        // 脱敏后应保留 fragment
        // 注意：path "/" 是 url::Url::parse 自动补全的，已在 redact_proxy_url 中过滤掉
        assert_eq!(
            redact_proxy_url("http://user:pass@proxy.example.com:8080/#section"),
            "http://***@proxy.example.com:8080#section"
        );
    }

    #[test]
    fn redact_proxy_url_empty_string() {
        // 空字符串应返回 [INVALID-PROXY-URL]（不 panic）
        assert_eq!(redact_proxy_url(""), INVALID_PLACEHOLDER);
    }

    #[test]
    fn redact_proxy_url_only_userinfo_no_host() {
        // 无 host 的格式错误 URL 应返回 [INVALID-PROXY-URL]
        assert_eq!(redact_proxy_url("http://user:pass@"), INVALID_PLACEHOLDER);
    }

    #[test]
    fn redact_proxy_url_socks5_scheme() {
        // SOCKS5 scheme 应正确处理（脱敏 userinfo）
        assert_eq!(
            redact_proxy_url("socks5://user:pass@proxy.example.com:1080"),
            "socks5://***@proxy.example.com:1080"
        );
    }
}
