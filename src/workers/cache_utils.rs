// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 抓取缓存工具集
//!
//! 本模块从 `scrape_worker` 抽离缓存相关纯函数，符合单一职责原则：
//! - `generate_scrape_cache_key`：cache key 生成（租户 + 方法 + URL + ScrapeOptions 内容指纹）
//! - `filter_sensitive_headers`：敏感响应头过滤（CWE-200）
//! - `SanitizedScrapeResponse`：borrowed 序列化结构体（性能，避免完整克隆）
//!
//! URL 日志脱敏位于 `crate::utils::url_redaction::redact_url_for_log`：
//! 该逻辑不属于 worker 生命周期，放在 `utils` 可让无默认特性构建也编译。
//!
//! `scrape_worker` 的 `try_read_scrape_cache` / `try_write_scrape_cache` 通过
//! 这些工具完成 cache key 计算与安全过滤，调用方在 `process_scrape_task`
//! 中先用 `generate_scrape_cache_key` 计算 key，再传入读写方法。
//!
//! 请求身份指纹的唯一实现位于 [`crate::workers::request_identity::request_fingerprint`]，
//! 本模块直接复用；禁止在缓存链路另建字段清单。

use std::collections::HashMap;

use serde::{Serialize, Serializer};

use crate::common::CacheContext;
use crate::engines::engine_client::{CapturedResponse, ScrapeOptions, ScrapeResponse};
use crate::workers::request_identity::request_fingerprint;

// cache key 生成

/// 生成抓取结果缓存 key
///
/// # 格式
///
/// ```text
/// scrape:{team_id}:{method}:{url}?fp={fingerprint}
/// ```
///
/// 四个维度共同定义"两个请求能否共享缓存结果"：
/// - `team_id`：租户隔离边界。缓存条目不再跨租户共享
///   （CWE-200：跨租户内容泄露路径不可达）
/// - `method`：HTTP 方法
/// - `url`：目标 URL
/// - `fingerprint`：`ScrapeOptions` 内容相关字段的结构化指纹，直接复用
///   [`request_fingerprint`]，与请求合并键使用同一实现
///
/// # 内容相关字段指纹
///
/// 旧实现手工维护 `headers` / `needs_js` / `session_id` 三个字段，
/// 与合并键的字段面不一致：`ScrapeOptions` 新增字段时缓存键不会自动跟上，
/// 造成同 URL 不同选项的缓存串扰。现改为复用 coalesce 的结构化编码：
///
/// - 所有影响抓取结果的 `ScrapeOptions` 字段参与（method/body/needs_js/
///   session_id/mobile/needs_screenshot/screenshot_config/actions/wait_for/
///   block_ads/block_media/use_fire_engine/needs_mllm/skip_tls_verification/
///   headers/proxy/timeout/sync_wait_ms/needs_tls_fingerprint 等）
/// - `cache_mode` 是缓存控制开关，**不参与**：`None` 与 `Some(Enabled)` 语义
///   等价，`ReadOnly` / `Bypass` 只改变本次读写行为。纳入会导致 ReadOnly
///   读写 key 不同而永远 miss、Bypass 写入无人读取。合并键另有由 `cache_mode`
///   派生的 `cache_read_allowed` 布尔投影（见 `coalesce_coordinator::key`），
///   缓存键不受其影响，四模式保持同一键。
/// - headers 先小写名再按（名, 值）排序，`HashMap` 迭代序不影响指纹
/// - 新增字段自动纳入，**禁止**在缓存链路维护第二份字段清单
///
/// # 旧条目
///
/// key 组成变化后旧缓存条目不再命中，按现有 TTL 自然过期，无需迁移。
///
/// # 性能
///
/// 指纹为 `DefaultHasher`（SipHash-1-3）压缩后的 64-bit 哈希，
/// 以 16 字符 hex 附加到 key。标准库零新依赖。
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
    let fingerprint = request_fingerprint(options);
    format!(
        "scrape:{}:{:?}:{}?fp={:016x}",
        ctx.team_id, ctx.method, ctx.url, fingerprint
    )
}

// 敏感响应头过滤

/// 敏感响应头集合
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

/// 过滤敏感响应头
///
/// 原地移除 `SENSITIVE_RESPONSE_HEADERS` 中列出的头。
/// 用于 [`SanitizedScrapeResponse`] 序列化前清理响应头，防止凭证泄露到缓存。
///
/// 用 `retain` 原地修改 +
/// `eq_ignore_ascii_case` 零分配比较，避免双重 HashMap 分配。
pub fn filter_sensitive_headers(headers: &mut HashMap<String, String>) {
    headers.retain(|k, _| {
        !SENSITIVE_RESPONSE_HEADERS
            .iter()
            .any(|s| k.eq_ignore_ascii_case(s))
    });
}

// 性能 borrowed 序列化结构体，避免完整克隆 ScrapeResponse

/// 过滤后用于序列化的借用结构体（性能）
///
/// 原 `try_write_scrape_cache` 实现：
/// 1. `let mut sanitized = response.clone;` —— 完整克隆 ScrapeResponse
///    （包含 `content: String` 可能数 MB、`screenshot: Option<String>` 可能 100KB+）
/// 2. `filter_sensitive_headers(&mut sanitized.headers);` —— 原地过滤
/// 3. `serde_json::to_string(&sanitized)` —— 序列化
///
/// 性能问题：步骤 1 克隆了整个 response（包括 content/screenshot 等大字段），
/// 但只用到了 headers 字段做过滤，其余字段原样参与序列化。这是不必要的堆分配。
///
/// 本结构体借用原 response 的所有字段，仅 headers 字段在序列化时通过
/// `SanitizedHeaders` 自定义 Serialize 跳过敏感头，实现零克隆序列化。
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
    /// XHR/Fetch 后台响应捕获（F6）；空时不写入缓存 JSON，
    /// 与 `ScrapeResponse` 的 skip_serializing_if 对齐，保证缓存读写对称
    #[serde(skip_serializing_if = "<[CapturedResponse]>::is_empty")]
    pub captured_responses: &'a [CapturedResponse],
    /// JS 降级标记（内存水位 Critical 静态回退）；None 不写入缓存 JSON，
    /// 与 `ScrapeResponse` 对齐——降级语义随缓存读写对称保留
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_degraded: &'a Option<bool>,
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
            captured_responses: &response.captured_responses,
            js_degraded: &response.js_degraded,
        }
    }
}

/// 自定义 Serialize：序列化时跳过敏感响应头（性能）
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

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::CacheMode;
    use crate::domain::id::Id;
    use crate::engines::engine_client::HttpMethod;

    /// 固定租户：同一用例内构造的 key 共享租户维度
    fn test_team() -> Id {
        Id::from_u128(0x7e57_0000_0000_0000_0000_0000_0000_0001)
    }

    /// 另一个租户：用于跨租户隔离断言
    fn other_team() -> Id {
        Id::from_u128(0x7e57_0000_0000_0000_0000_0000_0000_0002)
    }

    fn make_ctx(url: &str, method: HttpMethod) -> CacheContext {
        make_ctx_for(test_team(), url, method)
    }

    fn make_ctx_for(team_id: Id, url: &str, method: HttpMethod) -> CacheContext {
        CacheContext {
            team_id,
            url: url.to_string(),
            method,
            mode: CacheMode::Enabled,
        }
    }

    // generate_scrape_cache_key

    #[test]
    fn test_generate_scrape_cache_key_format() {
        let ctx = make_ctx("https://example.com", HttpMethod::Get);
        let opts = ScrapeOptions::default();
        let key = generate_scrape_cache_key(&ctx, &opts);
        // 格式：scrape:{team_id}:{method}:{url}?fp={16-hex}
        let prefix = format!("scrape:{}:Get:https://example.com?fp=", test_team());
        assert!(key.starts_with(&prefix));
        assert_eq!(key.len(), prefix.len() + 16);
    }

    #[test]
    fn test_cache_key_differs_by_team_id() {
        // 同 URL + 同 method + 同 options，不同租户必须产生不同 key。
        // 场景：租户 A 的私有页面内容不得被租户 B 的缓存命中读到。
        let url = "https://example.com/private";
        let opts = ScrapeOptions::default();

        let key_a =
            generate_scrape_cache_key(&make_ctx_for(test_team(), url, HttpMethod::Get), &opts);
        let key_b =
            generate_scrape_cache_key(&make_ctx_for(other_team(), url, HttpMethod::Get), &opts);

        assert_ne!(
            key_a, key_b,
            "different team_id must produce different cache keys"
        );
    }

    #[test]
    fn test_cache_key_fingerprint_matches_shared_request_fingerprint() {
        // 单一实现来源：缓存键指纹必须与 `request_fingerprint` 逐位一致，
        // 防止缓存链路私自维护第二份字段清单。合并键另叠加
        // `cache_read_allowed` 投影（见 coalesce_coordinator::key），不与缓存键相等。
        let mut opts = ScrapeOptions {
            needs_js: true,
            ..Default::default()
        };
        opts.headers
            .insert("X-Trace".to_string(), "abc".to_string());

        let key =
            generate_scrape_cache_key(&make_ctx("https://example.com", HttpMethod::Get), &opts);
        let expected = request_fingerprint(&opts);

        assert!(
            key.ends_with(&format!("?fp={:016x}", expected)),
            "cache key fingerprint must reuse workers::request_identity::request_fingerprint"
        );
    }

    #[test]
    fn test_cache_key_ignores_cache_mode_control_switch() {
        // 四种 cache_mode 的读写行为不同，但抓取结果身份相同：
        // 缓存键必须四模式一致，否则 ReadOnly 读写 key 不同永远 miss、
        // Bypass 写入无人读取。
        let ctx = make_ctx("https://example.com/cache-mode", HttpMethod::Get);
        let baseline = generate_scrape_cache_key(
            &ctx,
            &ScrapeOptions {
                cache_mode: None,
                ..Default::default()
            },
        );

        for mode in [
            CacheMode::Enabled,
            CacheMode::Disabled,
            CacheMode::ReadOnly,
            CacheMode::Bypass,
        ] {
            let opts = ScrapeOptions {
                cache_mode: Some(mode),
                ..Default::default()
            };
            assert_eq!(
                generate_scrape_cache_key(&ctx, &opts),
                baseline,
                "cache key must ignore cache_mode ({mode:?})"
            );
        }
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

    // options 影响字段串扰防护

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
    fn test_cache_key_same_team_differs_by_needs_js() {
        // 同 team + 同 URL + 同 method，needs_js=true vs false 应产生不同 key
        // 场景：浏览器渲染后 DOM 与原始 HTML 不能共享缓存
        let ctx = make_ctx("https://example.com", HttpMethod::Get);

        let opts_js = ScrapeOptions {
            needs_js: true,
            ..ScrapeOptions::default()
        };

        let opts_no_js = ScrapeOptions {
            needs_js: false,
            ..ScrapeOptions::default()
        };

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

        let opts_s1 = ScrapeOptions {
            session_id: Some("session-abc".to_string()),
            ..ScrapeOptions::default()
        };

        let opts_s2 = ScrapeOptions {
            session_id: Some("session-xyz".to_string()),
            ..ScrapeOptions::default()
        };

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

    // filter_sensitive_headers

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

    // SanitizedScrapeResponse（性能）

    #[test]
    fn test_sanitized_serialize_skips_sensitive_headers() {
        let mut headers = HashMap::new();
        headers.insert("Set-Cookie".to_string(), "secret".to_string());
        headers.insert("Content-Type".to_string(), "text/html".to_string());

        let response = ScrapeResponse {
            js_degraded: None,
            captured_responses: Vec::new(),
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
            js_degraded: None,
            captured_responses: Vec::new(),
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
        assert!(restored.captured_responses.is_empty());
    }

    #[test]
    fn test_sanitized_roundtrip_preserves_captured_responses() {
        // F6：非空捕获经缓存写读后完整还原
        let response = ScrapeResponse {
            js_degraded: None,
            captured_responses: vec![CapturedResponse {
                url: "https://example.com/api/data".to_string(),
                status: 200,
                mime_type: "application/json".to_string(),
                body: "{\"ok\":true}".to_string(),
            }],
            status_code: 200,
            content: "hello".to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers: HashMap::new(),
            response_time_ms: 10,
            final_url: None,
            markdown: None,
        };

        let sanitized = SanitizedScrapeResponse::from_response(&response);
        let json = serde_json::to_string(&sanitized).unwrap();
        let restored: ScrapeResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.captured_responses.len(), 1);
        assert_eq!(
            restored.captured_responses[0].url,
            "https://example.com/api/data"
        );
        assert_eq!(restored.captured_responses[0].status, 200);
        assert_eq!(restored.captured_responses[0].body, "{\"ok\":true}");
    }

    #[test]
    fn test_sanitized_serialize_does_not_modify_original() {
        // 借用序列化不应修改原 response.headers（性能核心保证）
        let mut headers = HashMap::new();
        headers.insert("Set-Cookie".to_string(), "secret".to_string());

        let response = ScrapeResponse {
            js_degraded: None,
            captured_responses: Vec::new(),
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
