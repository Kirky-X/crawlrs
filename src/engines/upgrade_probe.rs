// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 流式 HTTP→Chrome 升级探测模块（T014）
//!
//! 评估 HTTP 响应是否疑似 SPA 空壳，需要升级到浏览器引擎（如 Playwright）渲染。
//! 强信号立即判定升级；弱信号累加达阈值后升级。

use reqwest::header::HeaderMap;

/// 默认升级阈值，score >= threshold 时 upgrade=true
pub const DEFAULT_THRESHOLD: u32 = 10;

/// 性能审查 HIGH-1 修复：evaluate 仅扫描响应体前缀以降低开销。
///
/// 设计意图是 prefix-scan（见 `evaluate` docstring），调用方应截取 body 前
/// `PROBE_PREFIX_LEN` 字节传入。64KB 足以覆盖典型 SPA 空壳的 head+顶层 body。
pub const PROBE_PREFIX_LEN: usize = 64 * 1024;

/// 强信号加分（命中任一 SPA 框架标记 / hydration 空壳 / noscript 提示）
const STRONG_SIGNAL_SCORE: u32 = 10;

/// 弱信号：每个非追踪 `<script src>` 加分
const WEAK_SCRIPT_SRC_SCORE: u32 = 1;

/// 弱信号：每个 `<link rel=modulepreload>` 加分
const WEAK_MODULEPRELOAD_SCORE: u32 = 2;

/// 弱信号：可见文本极少但脚本多
const WEAK_TEXT_SPARSE_SCORE: u32 = 3;

/// 可见文本极少阈值（剥离 HTML 标签与 script/style 内容后的非空白字符数）
const VISIBLE_TEXT_SPARSE_THRESHOLD: usize = 200;

/// 脚本多阈值（`<script` 标签出现次数）
const SCRIPT_MANY_THRESHOLD: u32 = 3;

/// JS 升级探测结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeVerdict {
    /// 总评分
    pub score: u32,
    /// 是否需要升级到浏览器引擎
    pub upgrade: bool,
    /// 决策原因（信号名拼接，便于排障）
    pub reason: String,
}

/// JS 升级探测器
///
/// 评估 HTTP 响应是否疑似 SPA 空壳。
/// 默认阈值 10，可通过 [`JsUpgradeProbe::new`] 自定义。
#[derive(Debug, Clone)]
pub struct JsUpgradeProbe {
    /// 升级阈值，score >= threshold 时 upgrade=true
    pub threshold: u32,
}

impl Default for JsUpgradeProbe {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
        }
    }
}

impl JsUpgradeProbe {
    /// 创建指定阈值的探测器
    pub fn new(threshold: u32) -> Self {
        Self { threshold }
    }

    /// 评估 HTTP 响应是否需要升级到浏览器引擎
    ///
    /// - `headers`: HTTP 响应头，用于 Content-Type 前置短路（非 HTML 直接判定不升级）
    /// - `body_prefix`: 响应体前缀（仅扫描前面部分以降低开销）
    ///
    /// 评分规则：
    /// - 强信号（score += 10）：`__NUXT_DATA__` / `__NEXT_DATA__` / `window.__INITIAL_STATE__`
    ///   / `data-reactroot` / `<div id="root"></div>` 空壳 + hydration / `<noscript>` 提示需 JS
    /// - 弱信号：每个非追踪 `<script src>`（排除 GA/gtm/analytics/pixel）+1；
    ///   每个 `<link rel=modulepreload>` +2；可见文本极少但脚本多 +3
    /// - `score >= threshold → upgrade=true`
    pub fn evaluate(&self, headers: &HeaderMap, body_prefix: &str) -> ProbeVerdict {
        // 1. Content-Type 前置短路：非 HTML 不升级
        if let Some(ct) = headers.get(reqwest::header::CONTENT_TYPE) {
            if let Ok(ct_str) = ct.to_str() {
                let ct_lower = ct_str.to_ascii_lowercase();
                if !ct_lower.contains("html") {
                    return ProbeVerdict {
                        score: 0,
                        upgrade: false,
                        reason: format!("content-type non-html: {}", ct_str),
                    };
                }
            }
        }

        let mut score: u32 = 0;
        let mut reasons: Vec<&'static str> = Vec::new();

        // 2. 强信号
        for sig in STRONG_FRAMEWORK_SIGNALS {
            if body_prefix.contains(*sig) {
                score += STRONG_SIGNAL_SCORE;
                reasons.push(*sig);
            }
        }
        if body_prefix.contains(REACT_ROOT_SHELL)
            && body_prefix.to_ascii_lowercase().contains("hydrate")
        {
            score += STRONG_SIGNAL_SCORE;
            reasons.push("react-root-shell-hydrate");
        }
        if body_prefix.contains("<noscript>") && needs_js_hint(body_prefix) {
            score += STRONG_SIGNAL_SCORE;
            reasons.push("noscript-js-hint");
        }

        // 3. 弱信号
        let script_src_count = count_non_tracking_script_srcs(body_prefix);
        if script_src_count > 0 {
            score += script_src_count * WEAK_SCRIPT_SRC_SCORE;
            reasons.push("non-tracking-script-src");
        }

        let modulepreload_count = count_modulepreloads(body_prefix);
        if modulepreload_count > 0 {
            score += modulepreload_count * WEAK_MODULEPRELOAD_SCORE;
            reasons.push("modulepreload");
        }

        if is_text_sparse(body_prefix) && count_all_scripts(body_prefix) >= SCRIPT_MANY_THRESHOLD {
            score += WEAK_TEXT_SPARSE_SCORE;
            reasons.push("text-sparse-script-heavy");
        }

        let upgrade = score >= self.threshold;
        let reason = if reasons.is_empty() {
            "no-js-signals".to_string()
        } else {
            reasons.join(",")
        };

        ProbeVerdict {
            score,
            upgrade,
            reason,
        }
    }
}

/// SPA 框架数据挂载点
const STRONG_FRAMEWORK_SIGNALS: &[&str] = &[
    "__NUXT_DATA__",
    "__NEXT_DATA__",
    "window.__INITIAL_STATE__",
    "data-reactroot",
];

/// React 空壳标记
const REACT_ROOT_SHELL: &str = r#"<div id="root"></div>"#;

/// 判断 noscript 内容是否包含需要 JS 的提示
fn needs_js_hint(body: &str) -> bool {
    let Some(start) = body.find("<noscript>") else {
        return false;
    };
    let Some(rel_end) = body[start..].find("</noscript>") else {
        return false;
    };
    let content = &body[start..start + rel_end];
    let lower = content.to_ascii_lowercase();
    lower.contains("enable javascript")
        || lower.contains("please enable javascript")
        || lower.contains("requires javascript")
        || lower.contains("javascript is required")
        || lower.contains("javascript is disabled")
        || lower.contains("enable js")
}

/// 统计非追踪 `<script src>` 数量
///
/// 排除 GA / GTM / analytics / pixel 等追踪脚本
fn count_non_tracking_script_srcs(body: &str) -> u32 {
    let mut count = 0u32;
    let mut search_pos = 0;
    while let Some(rel_start) = body[search_pos..].find("<script") {
        let abs_start = search_pos + rel_start;
        let Some(tag_end) = body[abs_start..].find('>') else {
            break;
        };
        let tag = &body[abs_start..abs_start + tag_end];
        if let Some(src) = extract_script_src(tag) {
            if !is_tracking_script(&src.to_ascii_lowercase()) {
                count += 1;
            }
        }
        search_pos = abs_start + tag_end + 1;
    }
    count
}

/// 从 `<script ...>` 标签内提取 `src` 属性值
fn extract_script_src(tag: &str) -> Option<&str> {
    let mut rest = tag;
    while let Some(idx) = rest.find("src") {
        let after = &rest[idx + 3..];
        // 跳过空格 / = / 制表符
        let bytes = after.as_bytes();
        let mut i = 0;
        while i < bytes.len()
            && (bytes[i] == b' '
                || bytes[i] == b'='
                || bytes[i] == b'\t'
                || bytes[i] == b'\n'
                || bytes[i] == b'\r')
        {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            // 跳过非引号写法（如 src=foo.js），仅处理带引号的常见情况
            rest = &after[i..];
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != quote {
            j += 1;
        }
        if j > bytes.len() {
            return None;
        }
        return Some(&after[start..j]);
    }
    None
}

/// 判断 script src URL 是否为追踪脚本
fn is_tracking_script(url: &str) -> bool {
    url.contains("google-analytics")
        || url.contains("googletagmanager")
        || url.contains("/gtm.js")
        || url.contains("/gtm_")
        || url.contains("analytics.js")
        || url.contains("facebook.net/en_us/fbevents.js")
        || url.contains("/pixel.js")
        || url.contains("pixel-")
        || url.contains("hotjar")
        || url.contains("segment.com/analytics.js")
        || url.contains("mixpanel")
        || url.contains("amplitude")
        || url.contains("scorecardresearch")
        || url.contains("quantserve")
        || url.contains("chartbeat")
        || url.contains("bat.bing.com")
        || url.contains("clarity.ms")
        || url.contains("tiktok.com/i18n/pixel")
        || url.contains("snap.licdn.com")
        || url.contains("adsbygoogle")
        || url.contains("doubleclick")
        || url.contains("facebook.com/tr")
}

/// 统计 `<link rel="modulepreload">` 数量
fn count_modulepreloads(body: &str) -> u32 {
    let mut count = 0u32;
    let mut search_pos = 0;
    while let Some(rel_start) = body[search_pos..].find("<link") {
        let abs_start = search_pos + rel_start;
        let Some(tag_end) = body[abs_start..].find('>') else {
            break;
        };
        let tag = &body[abs_start..abs_start + tag_end];
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.contains("rel") && tag_lower.contains("modulepreload") {
            count += 1;
        }
        search_pos = abs_start + tag_end + 1;
    }
    count
}

/// 统计所有 `<script` 标签数量（含带/不带 src）
fn count_all_scripts(body: &str) -> u32 {
    let mut count = 0u32;
    let mut search_pos = 0;
    while let Some(rel_start) = body[search_pos..].find("<script") {
        count += 1;
        let abs_start = search_pos + rel_start;
        search_pos = abs_start + "<script".len();
    }
    count
}

/// 判断可见文本是否极少
///
/// 剥离 `<script>` / `<style>` 内容与所有 HTML 标签后，剩余非空白字符 <= 阈值
fn is_text_sparse(body: &str) -> bool {
    let stripped = remove_tag_block(body, "<script", "</script>");
    let stripped = remove_tag_block(&stripped, "<style", "</style>");
    let visible = strip_html_tags(&stripped);
    let visible_count = visible.chars().filter(|c| !c.is_whitespace()).count();
    visible_count <= VISIBLE_TEXT_SPARSE_THRESHOLD
}

/// 删除指定开始/结束标签之间的内容（包含标签本身）
fn remove_tag_block(body: &str, start_tag: &str, end_tag: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut search = 0;
    while search < body.len() {
        if let Some(rel) = body[search..].find(start_tag) {
            let abs_start = search + rel;
            out.push_str(&body[search..abs_start]);
            let after = &body[abs_start..];
            if let Some(rel_end) = after.find(end_tag) {
                search = abs_start + rel_end + end_tag.len();
            } else {
                // 未闭合，丢弃剩余
                return out;
            }
        } else {
            out.push_str(&body[search..]);
            break;
        }
    }
    out
}

/// 剥离所有 `<...>` HTML 标签
fn strip_html_tags(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_tag = false;
    for ch in body.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    /// 长可见文本（> 200 个非空白字符），用于避免触发 text-sparse bonus
    const LONG_VISIBLE_TEXT: &str = "This is a body with substantial visible text content designed to exceed the text-sparse threshold of two hundred non-whitespace characters. We include this paragraph to ensure the probe does not falsely trigger the text-sparse-script-heavy bonus during testing. The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";

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

    #[test]
    fn next_data_strong_signal_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<!DOCTYPE html><html><head><script id="__NEXT_DATA__" type="application/json">{"props":{}}</script></head><body><div id="__next"></div></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade, "expected upgrade, got {:?}", v);
        assert!(v.score >= 10, "score {} < 10", v.score);
        assert!(v.reason.contains("__NEXT_DATA__"));
    }

    #[test]
    fn nuxt_data_strong_signal_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body =
            r#"<html><body><script>window.__NUXT_DATA__ = {"data":[]};</script></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade);
        assert!(v.reason.contains("__NUXT_DATA__"));
    }

    #[test]
    fn initial_state_strong_signal_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><script>window.__INITIAL_STATE__ = {};</script></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade);
        assert!(v.reason.contains("window.__INITIAL_STATE__"));
    }

    #[test]
    fn data_reactroot_strong_signal_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><div data-reactroot="true"></div></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade);
        assert!(v.reason.contains("data-reactroot"));
    }

    #[test]
    fn react_root_shell_with_hydrate_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><div id="root"></div><script>ReactDOM.hydrate(<App/>, ...);</script></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade);
        assert!(v.reason.contains("react-root-shell-hydrate"));
    }

    #[test]
    fn react_root_shell_without_hydrate_does_not_trigger_strong_signal() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><div id="root"></div></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(!v.reason.contains("react-root-shell-hydrate"));
    }

    #[test]
    fn noscript_enable_javascript_triggers_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><noscript>Please enable JavaScript to run this app.</noscript></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(v.upgrade);
        assert!(v.reason.contains("noscript-js-hint"));
    }

    #[test]
    fn noscript_unrelated_text_does_not_trigger() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body><noscript>Your browser is too old.</noscript></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(!v.reason.contains("noscript-js-hint"));
    }

    #[test]
    fn static_page_does_not_trigger_upgrade() {
        let probe = JsUpgradeProbe::default();
        let body = r#"<!DOCTYPE html><html><head><title>Static</title></head><body><h1>Hello World</h1><p>This is a static page with plenty of visible text content to avoid being marked as text-sparse. Adding more content here to ensure we exceed the threshold for visible characters easily.</p><p>Another paragraph with substantial content.</p></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert!(!v.upgrade, "static page should not upgrade, got {:?}", v);
        assert_eq!(v.score, 0);
        assert_eq!(v.reason, "no-js-signals");
    }

    #[test]
    fn non_tracking_script_src_accumulates_weak_signal() {
        let probe = JsUpgradeProbe::default();
        // 8 个非追踪 script src = 8 分（未达阈值 10）
        let mut body = String::from("<html><body>");
        for _ in 0..8 {
            body.push_str(r#"<script src="/assets/app.js"></script>"#);
        }
        body.push_str(LONG_VISIBLE_TEXT);
        body.push_str("</body></html>");
        let v = probe.evaluate(&html_headers(), &body);
        assert!(!v.upgrade, "8 srcs (score 8) < threshold 10, got {:?}", v);
        assert_eq!(v.score, 8);
        assert!(v.reason.contains("non-tracking-script-src"));
    }

    #[test]
    fn modulepreload_accumulates_weak_signal() {
        let probe = JsUpgradeProbe::default();
        // 5 个 modulepreload = 10 分
        let mut body = String::from("<html><head>");
        for _ in 0..5 {
            body.push_str(r#"<link rel="modulepreload" href="/assets/chunk.js">"#);
        }
        body.push_str("</head><body>");
        body.push_str(LONG_VISIBLE_TEXT);
        body.push_str("</body></html>");
        let v = probe.evaluate(&html_headers(), &body);
        assert!(
            v.upgrade,
            "5 modulepreloads should reach threshold, got {:?}",
            v
        );
        assert_eq!(v.score, 10);
        assert!(v.reason.contains("modulepreload"));
    }

    #[test]
    fn tracking_scripts_are_excluded_from_score() {
        let probe = JsUpgradeProbe::default();
        let body = format!(
            r#"<html><head>
            <script src="https://www.google-analytics.com/analytics.js"></script>
            <script src="https://www.googletagmanager.com/gtm.js"></script>
            <script src="https://connect.facebook.net/en_US/fbevents.js"></script>
            <script src="https://snap.licdn.com/li.lms-analytics/insight.min.js"></script>
            <script src="https://bat.bing.com/bat.js"></script>
            <script src="https://www.facebook.com/tr?id=123"></script>
        </head><body>{}</body></html>"#,
            LONG_VISIBLE_TEXT
        );
        let v = probe.evaluate(&html_headers(), &body);
        assert_eq!(v.score, 0, "tracking scripts should not score, got {:?}", v);
        assert!(!v.upgrade);
    }

    #[test]
    fn text_sparse_and_script_heavy_adds_bonus() {
        let probe = JsUpgradeProbe::default();
        // 6 个非追踪 script src (6 分) + 文本极少脚本多 (3 分) = 9 分，未达阈值 10
        let mut body = String::from("<html><head>");
        for _ in 0..6 {
            body.push_str(r#"<script src="/assets/x.js"></script>"#);
        }
        body.push_str("</head><body>Hi</body></html>");
        let v = probe.evaluate(&html_headers(), &body);
        assert_eq!(v.score, 9);
        assert!(!v.upgrade);
        assert!(v.reason.contains("text-sparse-script-heavy"));
        assert!(v.reason.contains("non-tracking-script-src"));
    }

    #[test]
    fn mixed_weak_signals_reach_threshold() {
        let probe = JsUpgradeProbe::default();
        // 7 个非追踪 script src (7) + 2 个 modulepreload (4) = 11 分
        let mut body = String::from("<html><head>");
        for _ in 0..7 {
            body.push_str(r#"<script src="/assets/a.js"></script>"#);
        }
        for _ in 0..2 {
            body.push_str(r#"<link rel="modulepreload" href="/assets/b.js">"#);
        }
        body.push_str("</head><body>");
        body.push_str(LONG_VISIBLE_TEXT);
        body.push_str("</body></html>");
        let v = probe.evaluate(&html_headers(), &body);
        assert!(
            v.upgrade,
            "mixed signals should reach threshold, got {:?}",
            v
        );
        assert_eq!(v.score, 11);
    }

    #[test]
    fn custom_threshold_affects_decision() {
        // threshold=15，强信号 10 分不升级
        let probe = JsUpgradeProbe::new(15);
        let body = r#"<html><body><script>window.__INITIAL_STATE__ = {};</script></body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert_eq!(v.score, 10);
        assert!(!v.upgrade, "score 10 < threshold 15 should not upgrade");
    }

    #[test]
    fn non_html_content_type_short_circuits() {
        let probe = JsUpgradeProbe::default();
        // 即使 body 含 SPA 标记，JSON Content-Type 也不升级
        let body = r#"{"__NEXT_DATA__": "fake"}"#;
        let v = probe.evaluate(&json_headers(), body);
        assert!(!v.upgrade);
        assert_eq!(v.score, 0);
        assert!(v.reason.starts_with("content-type non-html"));
    }

    #[test]
    fn missing_content_type_does_not_short_circuit() {
        let probe = JsUpgradeProbe::default();
        // 不插入 Content-Type，应继续走 body 评估
        let h = HeaderMap::new();
        let body = r#"<html><body><script>window.__INITIAL_STATE__ = {};</script></body></html>"#;
        let v = probe.evaluate(&h, body);
        assert!(
            v.upgrade,
            "missing CT should proceed to body eval, got {:?}",
            v
        );
    }

    #[test]
    fn default_threshold_is_10() {
        assert_eq!(DEFAULT_THRESHOLD, 10);
        assert_eq!(JsUpgradeProbe::default().threshold, 10);
    }

    #[test]
    fn empty_body_does_not_trigger_upgrade() {
        let probe = JsUpgradeProbe::default();
        let v = probe.evaluate(&html_headers(), "");
        assert!(!v.upgrade);
        assert_eq!(v.score, 0);
        assert_eq!(v.reason, "no-js-signals");
    }

    #[test]
    fn single_strong_signal_with_multiple_signals_does_not_double_count() {
        // 同一强信号多次出现只算一次（design.md: 强信号立即升级 = +10）
        let probe = JsUpgradeProbe::default();
        let body = r#"<html><body>
            <script>window.__INITIAL_STATE__ = {};</script>
            <script>window.__INITIAL_STATE__ = {"x":1};</script>
            </body></html>"#;
        let v = probe.evaluate(&html_headers(), body);
        assert_eq!(v.score, 10, "same strong signal should not double-count");
        assert!(v.upgrade);
    }
}
