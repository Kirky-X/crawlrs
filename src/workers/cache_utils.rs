// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 抓取缓存工具集（架构审查 HIGH-2 SRP 拆分，T059/R-cache-002）
//!
//! 本模块从 `scrape_worker` 抽离缓存相关纯函数，符合单一职责原则：
//! - [`generate_scrape_cache_key`]：cache key 生成（HIGH-1 改进：纳入 ScrapeOptions 影响字段）
//! - [`redact_url_for_log`]：URL 日志脱敏（防 query/fragment 凭据泄露）
//! - [`filter_sensitive_headers`]：敏感响应头过滤（CWE-200）
//! - [`SanitizedScrapeResponse`]：borrowed 序列化结构体（性能 HIGH-1，避免完整克隆）
//!
//! `scrape_worker` 的 `try_read_scrape_cache` / `try_write_scrape_cache` 通过
//! 这些工具完成 cache key 计算与安全过滤，调用方在 `process_scrape_task`
//! 中先用 [`generate_scrape_cache_key`] 计算 key，再传入读写方法。

use std::collections::HashMap;

use serde::{Serialize, Serializer};
use url::Url;

use crate::common::CacheContext;
use crate::engines::engine_client::{ScrapeOptions, ScrapeResponse};

// =============================================================================
// cache key 生成（HIGH-1 改进）
// =============================================================================

/// 生成抓取结果缓存 key（T059/R-cache-002，HIGH-1 改进）
///
/// # 格式
///
/// ```text
/// scrape:{method}:{url}?fp={fingerprint}
/// ```
///
/// 其中 `fingerprint` 由 [`options_fingerprint`] 计算，纳入影响响应内容的
/// `ScrapeOptions` 字段，避免同 URL 不同 options 的缓存串扰。
///
/// # HIGH-1 改进：纳入 ScrapeOptions 影响字段
///
/// 原实现仅 `scrape:{method}:{url}`，存在以下串扰风险：
///
/// | 字段 | 影响维度 | 串扰场景 |
/// |------|---------|---------|
/// | `headers` | 自定义请求头影响服务端响应 | 带 `Accept-Language: zh` 与 `en` 的请求共享缓存，返回错误语言 |
/// | `needs_js` | 决定是否走浏览器引擎渲染 | 同 URL 一次 needs_js=true（拿到渲染后 DOM）一次 false（拿到原始 HTML）共享缓存，返回错误内容 |
/// | `session_id` | 粘性会话固定代理，影响 IP 风控结果 | 不同 session 走不同代理，A 已被目标站封禁、B 未封禁，共享缓存导致 B 拿到 A 的失败响应 |
///
/// 其余字段（`timeout` / `mobile` / `body` / `actions` 等）也影响响应但风险较低，
/// 为平衡 key 长度与正确性，仅纳入上述三个高影响字段。
///
/// # 性能权衡
///
/// `options_fingerprint` 用 [`std::collections::hash_map::DefaultHasher`]
/// （内部 SipHash-1-3）计算 64-bit 哈希，再以 hex 编码（16 字符）附加到 key。
/// 选 DefaultHasher 而非 fnv/cityhash：标准库零新依赖（规则25），短输入性能足够。
///
/// # 算法稳定性说明
///
/// `DefaultHasher` 文档明确 "internal algorithm is not guaranteed to be stable
/// across versions"。Rust 版本升级可能导致 cache key 变化，进而缓存全部失效
/// （降级为 miss → 重新抓取，不是错误）。考虑到服务端 Rust 版本升级频率低、
/// 且服务重启本身也会让内存缓存清空，此降级行为可接受。
///
/// # 安全
///
/// `session_id` 经 [`crate::engines::engine_client::validate_session_id`]
/// 校验（仅可打印 ASCII，<=128 字节），不会注入控制字符到 key。
#[must_use]
pub fn generate_scrape_cache_key(ctx: &CacheContext, options: &ScrapeOptions) -> String {
    let fp = options_fingerprint(options);
    format!("scrape:{:?}:{}?fp={}", ctx.method, ctx.url, fp)
}

/// 计算 `ScrapeOptions` 中影响响应内容的字段的指纹（HIGH-1）
///
/// 纳入字段：`headers` / `needs_js` / `session_id`。
/// 用 [`std::collections::hash_map::DefaultHasher`]（SipHash-1-3）计算 64-bit 哈希，
/// 输出 hex 字符串（16 字符）。
///
/// # 为何选 DefaultHasher
///
/// - 标准库零依赖（规则25）
/// - SipHash-1-3 抗碰撞足够：cache key 冲突=缓存串扰，非安全敏感场景
/// - Rust 当前版本稳定，跨版本变更最坏情况是缓存全部失效（降级，非错误）
///
/// # 为何只纳入三个字段
///
/// 详见 [`generate_scrape_cache_key`] 的串扰分析表。
#[must_use]
fn options_fingerprint(options: &ScrapeOptions) -> String {
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    // headers：按 key 排序后写入，保证不同顺序的相同 headers 集合产生相同指纹
    let mut sorted_headers: Vec<(&String, &String)> = options.headers.iter().collect();
    sorted_headers.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in sorted_headers {
        // T022 修复：使用长度前缀编码替代 NUL 分隔符，防止含 \0 的 key/value 碰撞
        hasher.write_usize(k.len());
        hasher.write(k.as_bytes());
        hasher.write_usize(v.len());
        hasher.write(v.as_bytes());
    }
    hasher.write_u8(0xFF); // headers 与 needs_js 的分界

    // needs_js：1 字节
    hasher.write_u8(u8::from(options.needs_js));

    // session_id：None 写 0，Some 写长度 + 内容
    match &options.session_id {
        None => hasher.write_u8(0),
        Some(sid) => {
            hasher.write_u8(1);
            hasher.write_usize(sid.len());
            hasher.write(sid.as_bytes());
        }
    }

    let hash = hasher.finish();
    format!("{:016x}", hash)
}

// =============================================================================
// URL 日志脱敏（T062 安全审查 MEDIUM-2）
// =============================================================================

/// URL 日志最大长度（防日志膨胀）
const MAX_URL_LOG_LEN: usize = 200;

/// 脱敏 URL 用于日志输出（T062 安全审查 MEDIUM-2 修复）
///
/// URL query 参数可能携带 `token` / `api_key` / `session_id` 等敏感信息，
/// warn 级别日志会持久化到磁盘。此函数移除 query 和 fragment，仅保留
/// `scheme://host[:port]/path`，并限制总长度防止日志膨胀。
///
/// 解析失败时返回 `[invalid-url]` 占位符，绝不原样返回可能含凭证的输入
/// （CWE-532 防护：日志文件中的信息泄露）。
#[must_use]
pub fn redact_url_for_log(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            // T021 修复：剥离 userinfo（用户名/密码），防止凭据泄露到日志
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

// =============================================================================
// 敏感响应头过滤（T062 安全审查 LOW-2）
// =============================================================================

/// 敏感响应头集合（T062 安全审查 LOW-2 修复）
///
/// 这些响应头可能携带凭证或会话信息，序列化到缓存后可能被其他用户读取
/// （CWE-200：信息暴露给未授权角色）。缓存前必须过滤。
///
/// 使用小写匹配（HTTP header 名称大小写不敏感）。
const SENSITIVE_RESPONSE_HEADERS: &[&str] = &[
    "set-cookie",
    "cookie",
    "authorization",
    "proxy-authorization",
    "www-authenticate",
    "x-api-key",
    "x-auth-token",
    "x-session-id",
];

/// 过滤敏感响应头（T062 安全审查 LOW-2 修复）
///
/// 原地移除 `SENSITIVE_RESPONSE_HEADERS` 中列出的头。
/// 用于 [`SanitizedScrapeResponse`] 序列化前清理响应头，防止凭证泄露到缓存。
///
/// 性能审查 MEDIUM-1 / LOW-2：用 `retain` 原地修改 +
/// `eq_ignore_ascii_case` 零分配比较，避免双重 HashMap 分配。
pub fn filter_sensitive_headers(headers: &mut HashMap<String, String>) {
    headers.retain(|k, _| {
        !SENSITIVE_RESPONSE_HEADERS
            .iter()
            .any(|s| k.eq_ignore_ascii_case(s))
    });
}

// =============================================================================
// 性能 HIGH-1：borrowed 序列化结构体，避免完整克隆 ScrapeResponse
// =============================================================================

/// 过滤后用于序列化的借用结构体（性能 HIGH-1）
///
/// 原 `try_write_scrape_cache` 实现：
/// 1. `let mut sanitized = response.clone();` —— 完整克隆 ScrapeResponse
///    （包含 `content: String` 可能数 MB、`screenshot: Option<String>` 可能 100KB+）
/// 2. `filter_sensitive_headers(&mut sanitized.headers);` —— 原地过滤
/// 3. `serde_json::to_string(&sanitized)` —— 序列化
///
/// 性能问题：步骤 1 克隆了整个 response（包括 content/screenshot 等大字段），
/// 但只用到了 headers 字段做过滤，其余字段原样参与序列化。这是不必要的堆分配。
///
/// 本结构体借用原 response 的所有字段，仅 headers 字段在序列化时通过
/// [`SanitizedHeaders`] 自定义 Serialize 跳过敏感头，实现零克隆序列化。
///
/// # 序列化结果与原 ScrapeResponse 一致
///
/// 字段顺序与 `ScrapeResponse` 的 `#[derive(Serialize)]` 保持一致，
/// 反序列化时可直接还原为 `ScrapeResponse`，无需适配层。
#[derive(Serialize)]
pub struct SanitizedScrapeResponse<'a> {
    pub status_code: u16,
    pub content: &'a str,
    pub screenshot: Option<&'a String>,
    pub content_type: &'a str,
    /// 自定义 Serialize 跳过敏感头
    #[serde(serialize_with = "serialize_sanitized_headers")]
    pub headers: &'a HashMap<String, String>,
    pub response_time_ms: u64,
    pub final_url: Option<&'a String>,
    pub markdown: Option<&'a String>,
}

/// 借用 `ScrapeResponse` 构造 `SanitizedScrapeResponse`，零克隆
///
/// 调用方在 `try_write_scrape_cache` 中：
///
/// ```ignore
/// let sanitized = SanitizedScrapeResponse::from_response(&response);
/// let json = serde_json::to_string(&sanitized)?;
/// ```
impl<'a> SanitizedScrapeResponse<'a> {
    #[must_use]
    pub fn from_response(response: &'a ScrapeResponse) -> Self {
        Self {
            status_code: response.status_code,
            content: &response.content,
            screenshot: response.screenshot.as_ref(),
            content_type: &response.content_type,
            headers: &response.headers,
            response_time_ms: response.response_time_ms,
            final_url: response.final_url.as_ref(),
            markdown: response.markdown.as_ref(),
        }
    }
}

/// 自定义 Serialize：序列化时跳过敏感响应头（性能 HIGH-1）
///
/// 用 `serialize_map` 逐项写入，对每个 key 用 `eq_ignore_ascii_case`
/// 判断是否在 [`SENSITIVE_RESPONSE_HEADERS`] 黑名单中。
///
/// 与 [`filter_sensitive_headers`] 行为等价，但**不修改原 HashMap**，
/// 避免克隆整个 ScrapeResponse 仅为过滤 headers。
fn serialize_sanitized_headers<S>(
    headers: &HashMap<String, String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;

    let mut map = serializer.serialize_map(Some(headers.len()))?;
    for (k, v) in headers {
        if !SENSITIVE_RESPONSE_HEADERS
            .iter()
            .any(|s| k.eq_ignore_ascii_case(s))
        {
            map.serialize_entry(k, v)?;
        }
    }
    map.end()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::CacheMode;
    use crate::engines::engine_client::HttpMethod;

    fn make_ctx(url: &str, method: HttpMethod) -> CacheContext {
        CacheContext {
            url: url.to_string(),
            method,
            mode: CacheMode::Enabled,
        }
    }

    // ============ generate_scrape_cache_key ============

    #[test]
    fn test_generate_scrape_cache_key_format() {
        let ctx = make_ctx("https://example.com", HttpMethod::Get);
        let opts = ScrapeOptions::default();
        let key = generate_scrape_cache_key(&ctx, &opts);
        // 格式：scrape:{method}:{url}?fp={16-hex}
        assert!(key.starts_with("scrape:Get:https://example.com?fp="));
        assert_eq!(key.len(), "scrape:Get:https://example.com?fp=".len() + 16);
    }

    #[test]
    fn test_generate_scrape_cache_key_different_methods_produce_different_keys() {
        let url = "https://example.com";
        let opts = ScrapeOptions::default();
        let get_key = generate_scrape_cache_key(&make_ctx(url, HttpMethod::Get), &opts);
        let post_key = generate_scrape_cache_key(&make_ctx(url, HttpMethod::Post), &opts);
        assert_ne!(get_key, post_key);
    }

    #[test]
    fn test_generate_scrape_cache_key_different_urls_produce_different_keys() {
        let opts = ScrapeOptions::default();
        let key1 = generate_scrape_cache_key(&make_ctx("https://a.com", HttpMethod::Get), &opts);
        let key2 = generate_scrape_cache_key(&make_ctx("https://b.com", HttpMethod::Get), &opts);
        assert_ne!(key1, key2);
    }

    // ===== HIGH-1: options 影响字段串扰防护 =====

    #[test]
    fn test_cache_key_differs_by_headers() {
        // 同 URL + 同 method，不同 headers 应产生不同 key
        // 场景：Accept-Language: zh vs en 不能共享缓存
        let ctx = make_ctx("https://example.com", HttpMethod::Get);

        let mut opts_zh = ScrapeOptions::default();
        opts_zh
            .headers
            .insert("Accept-Language".to_string(), "zh".to_string());

        let mut opts_en = ScrapeOptions::default();
        opts_en
            .headers
            .insert("Accept-Language".to_string(), "en".to_string());

        let key_zh = generate_scrape_cache_key(&ctx, &opts_zh);
        let key_en = generate_scrape_cache_key(&ctx, &opts_en);
        assert_ne!(
            key_zh, key_en,
            "different headers must produce different cache keys"
        );
    }

    #[test]
    fn test_cache_key_differs_by_needs_js() {
        // 同 URL + 同 method，needs_js=true vs false 应产生不同 key
        // 场景：浏览器渲染后 DOM 与原始 HTML 不能共享缓存
        let ctx = make_ctx("https://example.com", HttpMethod::Get);

        let mut opts_js = ScrapeOptions::default();
        opts_js.needs_js = true;

        let mut opts_no_js = ScrapeOptions::default();
        opts_no_js.needs_js = false;

        let key_js = generate_scrape_cache_key(&ctx, &opts_js);
        let key_no_js = generate_scrape_cache_key(&ctx, &opts_no_js);
        assert_ne!(
            key_js, key_no_js,
            "needs_js=true vs false must produce different keys"
        );
    }

    #[test]
    fn test_cache_key_differs_by_session_id() {
        // 同 URL + 同 method，不同 session_id 应产生不同 key
        // 场景：不同粘性会话走不同代理，IP 风控结果不同，不能共享缓存
        let ctx = make_ctx("https://example.com", HttpMethod::Get);

        let mut opts_s1 = ScrapeOptions::default();
        opts_s1.session_id = Some("session-abc".to_string());

        let mut opts_s2 = ScrapeOptions::default();
        opts_s2.session_id = Some("session-xyz".to_string());

        let key_s1 = generate_scrape_cache_key(&ctx, &opts_s1);
        let key_s2 = generate_scrape_cache_key(&ctx, &opts_s2);
        assert_ne!(
            key_s1, key_s2,
            "different session_id must produce different keys"
        );
    }

    #[test]
    fn test_cache_key_same_options_produce_same_key() {
        // 同 URL + 同 method + 同 options 应产生相同 key（基准对照）
        let ctx = make_ctx("https://example.com", HttpMethod::Get);
        let opts1 = ScrapeOptions::default();
        let opts2 = ScrapeOptions::default();
        let key1 = generate_scrape_cache_key(&ctx, &opts1);
        let key2 = generate_scrape_cache_key(&ctx, &opts2);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_headers_order_independent() {
        // headers 顺序不同但内容相同 → 相同 key（指纹按 sorted 顺序计算）
        let ctx = make_ctx("https://example.com", HttpMethod::Get);

        let mut opts_a = ScrapeOptions::default();
        opts_a
            .headers
            .insert("X-Custom".to_string(), "v1".to_string());
        opts_a
            .headers
            .insert("Accept".to_string(), "json".to_string());

        let mut opts_b = ScrapeOptions::default();
        opts_b
            .headers
            .insert("Accept".to_string(), "json".to_string());
        opts_b
            .headers
            .insert("X-Custom".to_string(), "v1".to_string());

        let key_a = generate_scrape_cache_key(&ctx, &opts_a);
        let key_b = generate_scrape_cache_key(&ctx, &opts_b);
        assert_eq!(key_a, key_b, "headers order must not affect cache key");
    }

    // ============ redact_url_for_log ============

    #[test]
    fn test_redact_url_for_log_strips_query_and_fragment() {
        let url = "https://example.com/path?token=secret&api_key=abc#fragment";
        let redacted = redact_url_for_log(url);
        assert_eq!(redacted, "https://example.com/path");
    }

    #[test]
    fn test_redact_url_for_log_invalid_url_returns_placeholder() {
        let redacted = redact_url_for_log("not a url");
        assert_eq!(redacted, "[invalid-url]");
    }

    #[test]
    fn test_redact_url_for_log_truncates_long_url() {
        let long_path = "a".repeat(300);
        let url = format!("https://example.com/{}", long_path);
        let redacted = redact_url_for_log(&url);
        assert!(redacted.ends_with("..."));
        assert!(redacted.len() <= MAX_URL_LOG_LEN + 3); // +3 for "..."
    }

    // ============ filter_sensitive_headers ============

    #[test]
    fn test_filter_sensitive_headers_removes_known_sensitive() {
        let mut headers = HashMap::new();
        headers.insert("Set-Cookie".to_string(), "session=abc".to_string());
        headers.insert("Authorization".to_string(), "Bearer xyz".to_string());
        headers.insert("X-API-Key".to_string(), "key123".to_string());
        headers.insert("Content-Type".to_string(), "text/html".to_string());

        filter_sensitive_headers(&mut headers);

        assert!(!headers.contains_key("Set-Cookie"));
        assert!(!headers.contains_key("Authorization"));
        assert!(!headers.contains_key("X-API-Key"));
        assert!(headers.contains_key("Content-Type"));
    }

    #[test]
    fn test_filter_sensitive_headers_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("set-cookie".to_string(), "v".to_string());
        headers.insert("SET-COOKIE".to_string(), "v".to_string());
        headers.insert("Set-Cookie".to_string(), "v".to_string());

        filter_sensitive_headers(&mut headers);

        assert!(headers.is_empty(), "all case variants should be removed");
    }

    #[test]
    fn test_filter_sensitive_headers_empty_map_no_op() {
        let mut headers = HashMap::new();
        filter_sensitive_headers(&mut headers);
        assert!(headers.is_empty());
    }

    // ============ SanitizedScrapeResponse（性能 HIGH-1） ============

    #[test]
    fn test_sanitized_serialize_skips_sensitive_headers() {
        let mut headers = HashMap::new();
        headers.insert("Set-Cookie".to_string(), "secret".to_string());
        headers.insert("Content-Type".to_string(), "text/html".to_string());

        let response = ScrapeResponse {
            status_code: 200,
            content: "<html></html>".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers,
            response_time_ms: 42,
            final_url: Some("https://example.com".to_string()),
            markdown: None,
        };

        let sanitized = SanitizedScrapeResponse::from_response(&response);
        let json = serde_json::to_string(&sanitized).expect("serialize should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // 敏感头不应出现在序列化结果
        let headers_obj = parsed["headers"]
            .as_object()
            .expect("headers should be object");
        assert!(!headers_obj.contains_key("Set-Cookie"));
        assert!(headers_obj.contains_key("Content-Type"));
    }

    #[test]
    fn test_sanitized_serialize_round_trip_restores_response() {
        // 序列化 → 反序列化为 ScrapeResponse，应能还原（除敏感头）
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "text/html".to_string());
        headers.insert("X-Trace-Id".to_string(), "abc-123".to_string());

        let response = ScrapeResponse {
            status_code: 200,
            content: "hello".to_string(),
            screenshot: Some("base64data".to_string()),
            content_type: "text/html".to_string(),
            headers,
            response_time_ms: 100,
            final_url: None,
            markdown: Some("# Title".to_string()),
        };

        let sanitized = SanitizedScrapeResponse::from_response(&response);
        let json = serde_json::to_string(&sanitized).unwrap();
        let restored: ScrapeResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.status_code, 200);
        assert_eq!(restored.content, "hello");
        assert_eq!(restored.screenshot.as_deref(), Some("base64data"));
        assert_eq!(restored.content_type, "text/html");
        assert_eq!(restored.headers.len(), 2);
        assert_eq!(
            restored.headers.get("Content-Type").map(|v| v.as_str()),
            Some("text/html")
        );
        assert_eq!(restored.response_time_ms, 100);
        assert_eq!(restored.markdown.as_deref(), Some("# Title"));
    }

    #[test]
    fn test_sanitized_serialize_does_not_modify_original() {
        // 借用序列化不应修改原 response.headers（性能 HIGH-1 核心保证）
        let mut headers = HashMap::new();
        headers.insert("Set-Cookie".to_string(), "secret".to_string());

        let response = ScrapeResponse {
            status_code: 200,
            content: "test".to_string(),
            screenshot: None,
            content_type: "text/plain".to_string(),
            headers,
            response_time_ms: 1,
            final_url: None,
            markdown: None,
        };

        let sanitized = SanitizedScrapeResponse::from_response(&response);
        let _ = serde_json::to_string(&sanitized).unwrap();

        // 原 response.headers 不应被修改
        assert_eq!(response.headers.len(), 1);
        assert!(response.headers.contains_key("Set-Cookie"));
    }
}
