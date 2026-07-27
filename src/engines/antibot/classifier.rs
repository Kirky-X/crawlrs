// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 反爬虫检测分类器（移植 crawl4ai `antibot_detector.py` 的三层判定逻辑）
//!
//! [`classify`] 是唯一入口，按以下顺序检测（与 Python 版对齐）：
//! 1. `429` → [`AntiBotTech::RateLimited`]（needs_browser=true）
//! 2. Tier1 命中（含大页 strip `<script>`/`<style>` 后深扫）
//! 3. `403`/`503` 且非 data-HTML（JSON/XML） → block 判定
//! 4. `4xx`/`5xx` + 短页（<10KB） → Tier2 AC 自动机扫描
//! 5. `200` + 近空 body → JS 空壳（[`AntiBotTech::StructuralBlock`]，needs_browser=true）
//! 6. Tier3 结构完整性（无 `<body>` / 可见文本<50 / 脚本重无内容，多信号打分）
//!
//! [`looks_like_data`] 用于识别 JSON/XML 避免对 API 响应误报。

use super::patterns::{
    AntiBotTech, NEAR_EMPTY_BODY_LEN, TIER1_REGEXES, TIER2_AUTOMATON, TIER2_BODY_SIZE_LIMIT,
    TIER3_ANY_TAG, TIER3_MIN_SIGNALS, TIER3_NO_BODY, TIER3_SCRIPT_BLOCK,
    TIER3_SCRIPT_HEAVY_BYTES, TIER3_SCRIPT_HEAVY_VISIBLE_MAX, TIER3_VISIBLE_TEXT_MIN,
};
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use reqwest::header::HeaderMap;

/// 单次检测结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// 命中的反爬技术类别
    pub tech: AntiBotTech,
    /// 人类可读的命中原因（含命中模式与位置说明）
    pub reason: String,
    /// 是否需要浏览器引擎重试（true 表示该反爬必须靠 JS 渲染才能突破）
    pub needs_browser: bool,
}

impl Detection {
    /// 构造一个 Detection
    fn new(tech: AntiBotTech, reason: impl Into<String>, needs_browser: bool) -> Self {
        Self {
            tech,
            reason: reason.into(),
            needs_browser,
        }
    }
}

/// 用于识别 data-HTML（JSON / XML）避免对 API 响应误报
///
/// 仅检查 body 起始字符与 Content-Type header。返回 true 时 [`classify`] 直接返回 `None`。
static JSON_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*[\[\{]").expect("antibot json-prefix regex")
});

static XML_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*<\?xml\b").expect("antibot xml-prefix regex")
});

/// 判定 body 是否为 data-HTML（JSON / XML），避免对 API 响应误报
fn looks_like_data(body: &str, headers: &HeaderMap) -> bool {
    // Content-Type 优先判定
    if let Some(ct) = headers.get(reqwest::header::CONTENT_TYPE) {
        if let Ok(ct_str) = ct.to_str() {
            let ct_lower = ct_str.to_ascii_lowercase();
            if ct_lower.contains("application/json")
                || ct_lower.contains("text/xml")
                || ct_lower.contains("application/xml")
                || ct_lower.contains("application/atom+xml")
                || ct_lower.contains("application/rss+xml")
            {
                return true;
            }
        }
    }
    // body 起始字符辅助判定
    if JSON_PREFIX.is_match(body) || XML_PREFIX.is_match(body) {
        return true;
    }
    false
}

/// `<style>` 块正则（与 `TIER3_SCRIPT_BLOCK` 配套，用于大页 strip）
static STYLE_BLOCK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)<style\b[^>]*>.*?</style>").expect("antibot style-block regex"));

/// 剥离 `<script>` 与 `<style>` 块，返回清洗后的 body
///
/// 用于大页深扫：避免把脚本里的反爬字面量误判为页内提示。
fn strip_scripts_and_styles(body: &str) -> String {
    let s = TIER3_SCRIPT_BLOCK.replace_all(body, "");
    STYLE_BLOCK.replace_all(&s, "").to_string()
}

/// Tier1 检测：扫描 body（含大页 strip 后深扫）返回命中的 `(tech, reason)` 元组
///
/// 性能审查 MEDIUM-3 修复：`stripped_cache` 由 [`classify`] 顶层传入，
/// 跨 tier1_match / Step 5 / tier3_signals 共享同一份 `strip_scripts_and_styles` 结果。
/// 原实现对大页 + Tier1 未命中 + Tier3 路径会重复 strip 2 次（每次 ~1-5ms / 100KB）。
fn tier1_match(body: &str, stripped_cache: &OnceCell<String>) -> Option<(AntiBotTech, String)> {
    // 直接扫描
    for (re, tech) in TIER1_REGEXES.iter() {
        if re.is_match(body) {
            return Some((*tech, tier1_reason(*tech)));
        }
    }
    // 大页：strip script/style 后深扫
    if body.len() > TIER2_BODY_SIZE_LIMIT {
        let stripped = stripped_cache.get_or_init(|| strip_scripts_and_styles(body));
        for (re, tech) in TIER1_REGEXES.iter() {
            if re.is_match(stripped) {
                return Some((*tech, tier1_reason(*tech)));
            }
        }
    }
    None
}

/// 每个 WAF 的命中说明（与 [`AntiBotTech`] 变体顺序对齐）
fn tier1_reason(tech: AntiBotTech) -> String {
    match tech {
        AntiBotTech::Cloudflare => "cloudflare challenge structure matched".to_string(),
        AntiBotTech::Akamai => "akamai reference / bmbl signature matched".to_string(),
        AntiBotTech::PerimeterX => "perimeterx captcha / cookie matched".to_string(),
        AntiBotTech::DataDome => "datadome cookie / host matched".to_string(),
        AntiBotTech::Imperva => "incapsula cookie matched".to_string(),
        AntiBotTech::Sucuri => "sucuri firewall signature matched".to_string(),
        AntiBotTech::Kasada => "kasada pointdefense signature matched".to_string(),
        AntiBotTech::AwsWaf => "aws waf captcha / cookie matched".to_string(),
        AntiBotTech::RateLimited => "http 429 rate limit".to_string(),
        AntiBotTech::StructuralBlock => "structural block / js shell detected".to_string(),
        AntiBotTech::Unknown => "unknown antibot signature".to_string(),
    }
}

/// Tier3 结构信号统计：返回命中的信号数与说明列表
///
/// 可见文本统计需先剥离 `<script>`/`<style>` 块，否则脚本内容会被计入
/// 可见文本，使"脚本重无内容"信号失效。
///
/// 性能审查 MEDIUM-3 修复：`stripped_cache` 由 [`classify`] 顶层传入，
/// 复用 tier1_match / Step 5 已计算的 stripped 结果（若已计算），避免重复 strip。
fn tier3_signals(body: &str, stripped_cache: &OnceCell<String>) -> (usize, Vec<&'static str>) {
    let mut signals: Vec<&'static str> = Vec::new();

    // 先剥离 script/style 块再统计可见文本（复用 cache）
    let stripped_of_scripts = stripped_cache.get_or_init(|| strip_scripts_and_styles(body));
    let stripped = TIER3_ANY_TAG.replace_all(stripped_of_scripts, "");

    // 信号 1：无 `<body>` 标签
    if !TIER3_NO_BODY.is_match(body) {
        signals.push("missing <body> tag");
    }

    // 信号 2：可见文本过短
    let visible_len = stripped.trim().len();
    if visible_len < TIER3_VISIBLE_TEXT_MIN {
        signals.push("visible text < 50 chars");
    }

    // 信号 3：脚本重且无内容
    let script_bytes: usize = TIER3_SCRIPT_BLOCK
        .find_iter(body)
        .map(|m| m.as_str().len())
        .sum();
    if script_bytes > TIER3_SCRIPT_HEAVY_BYTES && visible_len < TIER3_SCRIPT_HEAVY_VISIBLE_MAX {
        signals.push("script-heavy with minimal content");
    }

    (signals.len(), signals)
}

/// 主入口：分类 HTTP 响应为反爬检测或 None
///
/// 实现与 crawl4ai `antibot_detector.py` 对齐：先看状态码与 data-HTML，再依次走 Tier1 →
/// Tier2 → Tier3，并在大页场景下 strip script/style 后深扫。
pub fn classify(status: u16, body: &str, headers: &HeaderMap, _url: &str) -> Option<Detection> {
    // 性能审查 MEDIUM-3 修复：strip_scripts_and_styles 结果缓存
    //
    // 原实现对大页 + Tier1 未命中 + Tier3 路径会重复调用 strip_scripts_and_styles
    // 2-3 次（tier1_match 内 + Step 5 near-empty 检测 + tier3_signals 内），每次
    // 对 100KB+ 大页耗时 ~1-5ms。OnceCell 单次填充，多次借用，零重复 strip。
    //
    // 单次 classify 调用是单线程，无需 Sync；用 once_cell::sync::OnceCell 以兼容
    // 后续若将 classify 改为 Send 上下文（OnceCell<String> 是 Sync）。
    let stripped_cache: OnceCell<String> = OnceCell::new();

    // Step 0：data-HTML（JSON/XML）直接跳过避免误报
    if looks_like_data(body, headers) {
        return None;
    }

    // Step 1：429 → RateLimited（needs_browser=true）
    if status == 429 {
        return Some(Detection::new(
            AntiBotTech::RateLimited,
            "HTTP 429 rate limited",
            true,
        ));
    }

    // Step 2：Tier1 命中（任意状态码，含大页 strip 深扫）
    if let Some((tech, hint)) = tier1_match(body, &stripped_cache) {
        let needs_browser = matches!(
            tech,
            AntiBotTech::Cloudflare
                | AntiBotTech::Akamai
                | AntiBotTech::PerimeterX
                | AntiBotTech::DataDome
                | AntiBotTech::Imperva
                | AntiBotTech::Kasada
                | AntiBotTech::AwsWaf
        );
        return Some(Detection::new(tech, hint, needs_browser));
    }

    // Step 3：403/503 非 data-HTML → block 判定（needs_browser=true）
    if status == 403 || status == 503 {
        let needs_browser = true;
        let reason = if status == 403 {
            "HTTP 403 blocked without data-HTML body"
        } else {
            "HTTP 503 service unavailable block"
        };
        return Some(Detection::new(
            AntiBotTech::Unknown,
            reason.to_string(),
            needs_browser,
        ));
    }

    // Step 4：4xx/5xx + 短页（<10KB）→ Tier2 AC 自动机
    let is_error = (400..500).contains(&status) || (500..600).contains(&status);
    if is_error && body.len() < TIER2_BODY_SIZE_LIMIT {
        if let Some(mat) = TIER2_AUTOMATON.find(body) {
            let matched = &body[mat.start()..mat.end()];
            return Some(Detection::new(
                AntiBotTech::Unknown,
                format!("tier2 generic keyword matched: {}", matched),
                false,
            ));
        }
        // 错误状态 + 短页 + 非 data-HTML + Tier2 无命中：仍疑似 block
        return Some(Detection::new(
            AntiBotTech::Unknown,
            format!("HTTP {} short body without data content", status),
            false,
        ));
    }

    // Step 5：200 + 近空 body → JS 空壳（StructuralBlock, needs_browser=true）
    //
    // 双条件：body 整体字节少 AND 剥离 script/style + tag 后可见文本也少。
    // 仅凭 body.len() 会让短小但内容充实的正常页（~100 字可见文本）被误判。
    //
    // 性能审查 MEDIUM-3 修复：复用 stripped_cache，与 tier1_match / tier3_signals
    // 共享同一份 stripped 结果（若已计算则零成本复用）。
    if (200..300).contains(&status) {
        if body.len() < NEAR_EMPTY_BODY_LEN {
            let stripped_of_scripts = stripped_cache.get_or_init(|| strip_scripts_and_styles(body));
            let stripped = TIER3_ANY_TAG.replace_all(stripped_of_scripts, "");
            if stripped.trim().len() < TIER3_VISIBLE_TEXT_MIN {
                return Some(Detection::new(
                    AntiBotTech::StructuralBlock,
                    "near-empty body likely JS shell",
                    true,
                ));
            }
        }

        // Step 6：Tier3 结构完整性（多信号打分）
        let (count, signals) = tier3_signals(body, &stripped_cache);
        if count >= TIER3_MIN_SIGNALS {
            return Some(Detection::new(
                AntiBotTech::StructuralBlock,
                format!("tier3 structural signals: {}", signals.join(", ")),
                true,
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn html_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        h
    }

    fn json_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        h
    }

    // -------- Step 1：429 → RateLimited --------

    #[test]
    fn classify_429_returns_rate_limited_needs_browser() {
        let det = classify(429, "rate limited", &empty_headers(), "https://example.com")
            .expect("429 should detect");
        assert_eq!(det.tech, AntiBotTech::RateLimited);
        assert!(det.needs_browser, "rate limited should need browser");
    }

    // -------- Step 0：JSON 响应返回 None --------

    #[test]
    fn classify_json_response_returns_none() {
        let body = r#"{"error":"blocked","reason":"captcha"}"#;
        let det = classify(429, body, &json_headers(), "https://api.example.com/v1");
        assert!(det.is_none(), "JSON response should not be flagged");
    }

    #[test]
    fn classify_xml_response_returns_none() {
        let body = "<?xml version=\"1.0\"?><error>blocked</error>";
        let det = classify(403, body, &empty_headers(), "https://example.com/feed");
        assert!(det.is_none(), "XML response should not be flagged");
    }

    // -------- Step 2：Tier1 命中 --------

    #[test]
    fn classify_cloudflare_tier1_match() {
        let body = concat!(
            "<html><head><title>Just a moment...</title></head>",
            "<body><script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/jsch/v1\"></script></body></html>"
        );
        let det = classify(403, body, &html_headers(), "https://example.com")
            .expect("cloudflare body should detect");
        assert_eq!(det.tech, AntiBotTech::Cloudflare);
        assert!(det.needs_browser);
    }

    #[test]
    fn classify_akamai_tier1_match() {
        let body = "<html><body>Reference #18.abc.456</body></html>";
        let det = classify(403, body, &html_headers(), "https://example.com")
            .expect("akamai body should detect");
        assert_eq!(det.tech, AntiBotTech::Akamai);
        assert!(det.needs_browser);
    }

    #[test]
    fn classify_datadome_tier1_match_overrides_status() {
        // 即使 200 + DataDome 标记也要命中
        let body = "<html><body><script src=\"https://ct.captcha-delivery.com/c.js\"></script></body></html>";
        let det = classify(200, body, &html_headers(), "https://example.com")
            .expect("datadome tier1 should detect regardless of status");
        assert_eq!(det.tech, AntiBotTech::DataDome);
    }

    #[test]
    fn classify_tier1_strips_large_page_for_deep_scan() {
        // 构造大页：5KB 噪声脚本后跟 Cloudflare 标记
        let noise = "<script>".to_string() + &"x".repeat(6 * 1024) + "</script>";
        let body = format!(
            "<html><head>{}</head><body><script src=\"/cdn-cgi/challenge-platform/x\"></script></body></html>",
            noise
        );
        let det = classify(200, &body, &html_headers(), "https://example.com")
            .expect("tier1 deep scan on large page should detect");
        assert_eq!(det.tech, AntiBotTech::Cloudflare);
    }

    // -------- Step 3：403/503 非 data-HTML → block --------

    #[test]
    fn classify_403_empty_body_returns_block() {
        let det = classify(403, "", &html_headers(), "https://example.com")
            .expect("403 empty body should detect");
        assert_eq!(det.tech, AntiBotTech::Unknown);
        assert!(det.reason.contains("403"));
    }

    #[test]
    fn classify_503_short_body_returns_block() {
        let det = classify(503, "Service Unavailable", &html_headers(), "https://example.com")
            .expect("503 short body should detect");
        assert_eq!(det.tech, AntiBotTech::Unknown);
    }

    // -------- Step 4：4xx + 短页 → Tier2 通用词 --------
    //
    // 注：403/503 走 Step 3 直接 block，不会到 Tier2；此处用 400 触发 Tier2 扫描。

    #[test]
    fn classify_4xx_tier2_keyword_match() {
        let det = classify(400, "You have been blocked by our system", &html_headers(), "https://example.com")
            .expect("4xx tier2 keyword should detect");
        assert_eq!(det.tech, AntiBotTech::Unknown);
        assert!(det.reason.contains("tier2"));
    }

    #[test]
    fn classify_4xx_short_no_keyword_still_blocks() {
        let det = classify(400, "bad request", &html_headers(), "https://example.com")
            .expect("4xx short body should still block");
        assert_eq!(det.tech, AntiBotTech::Unknown);
    }

    // -------- Step 5：200 + 近空 → StructuralBlock --------

    #[test]
    fn classify_200_near_empty_returns_structural_block() {
        // 仅一个 <script> 占位
        let body = "<html><body><script>boot()</script></body></html>";
        let det = classify(200, body, &html_headers(), "https://example.com")
            .expect("200 near-empty body should detect");
        assert_eq!(det.tech, AntiBotTech::StructuralBlock);
        assert!(det.needs_browser);
    }

    // -------- Step 6：Tier3 多信号 --------

    #[test]
    fn classify_200_script_heavy_no_content_returns_structural_block() {
        // 构造：有 <body>，可见文本极少，但脚本字节超大且无可见内容
        let big_script = format!("<script>{}</script>", "var x=1;".repeat(1500)); // >5KB
        let body = format!(
            "<html><body>{}</body></html>",
            big_script
        );
        let det = classify(200, &body, &html_headers(), "https://example.com")
            .expect("script-heavy 200 should detect");
        assert_eq!(det.tech, AntiBotTech::StructuralBlock);
        assert!(det.reason.contains("tier3") || det.reason.contains("near-empty"));
    }

    #[test]
    fn classify_200_missing_body_and_low_text_returns_structural_block() {
        // 无 <body>，可见文本 <50
        let body = "<html><head><title>Checking your browser</title></head></html>";
        let det = classify(200, body, &html_headers(), "https://example.com")
            .expect("missing body + low text should detect");
        assert_eq!(det.tech, AntiBotTech::StructuralBlock);
    }

    // -------- 正常大页返回 None --------

    #[test]
    fn classify_normal_large_html_returns_none() {
        // 正常 HTML 页：有 body，可见文本充足，无反爬标记
        let body = format!(
            "<html><head><title>Example Domain</title></head>\
             <body><h1>Example Domain</h1>\
             <p>{}</p>\
             <a href=\"https://www.iana.org/domains/example\">More information...</a>\
             </body></html>",
            "This domain is for use in illustrative examples in documents. ".repeat(20)
        );
        let det = classify(200, &body, &html_headers(), "https://example.com");
        assert!(det.is_none(), "normal large page should not detect");
    }

    #[test]
    fn classify_normal_200_returns_none() {
        let body = "<html><body><h1>Welcome to example.com</h1>\
                    <p>This is a legitimate page with enough visible text content to pass Tier3.</p>\
                    </body></html>";
        let det = classify(200, body, &html_headers(), "https://example.com");
        assert!(det.is_none());
    }

    // -------- looks_like_data 单测 --------

    #[test]
    fn looks_like_data_json_content_type() {
        assert!(looks_like_data("not json", &json_headers()));
    }

    #[test]
    fn looks_like_data_xml_body_prefix() {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );
        // XML 起始且无 XML content-type → 走 body 起始判定
        assert!(looks_like_data(
            "<?xml version=\"1.0\"?><rss></rss>",
            &h
        ));
    }

    #[test]
    fn looks_like_data_html_returns_false() {
        assert!(!looks_like_data("<html><body>hi</body></html>", &html_headers()));
    }

    // -------- Detection 构造 --------

    #[test]
    fn detection_new_basic() {
        let d = Detection::new(AntiBotTech::Cloudflare, "test", true);
        assert_eq!(d.tech, AntiBotTech::Cloudflare);
        assert_eq!(d.reason, "test");
        assert!(d.needs_browser);
    }
}
