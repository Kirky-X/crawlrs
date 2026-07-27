// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 反爬虫检测模式表（移植 crawl4ai `antibot_detector.py` 三层检测的 patterns）
//!
//! - [`AntiBotTech`]：识别的反爬技术类别（11 个变体）。
//! - Tier1：高置信 WAF 结构标记，`regex` 编译为 `Regex` 列表，对任意页大小都做扫描。
//!   少量模式含 backreference（如 `Reference\s*#(\d+)`）故整体走 regex，不走 AC。
//! - Tier2：通用词字面量集，纯字面量走 `aho_corasick::AhoCorasick` 自动机，仅对 <10KB 页扫描。
//! - Tier3：结构完整性正则（无 `<body>` / 可见文本过短 / 脚本重无内容）。
//!
//! 规则 10：本文件仅承载常量与模式定义，`classify` 实现在 `classifier.rs`。

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use once_cell::sync::Lazy;
use regex::Regex;

/// 反爬技术类别（与 crawl4ai `AntibotTech` 对齐）
///
/// 顺序与 patterns 表中的索引对应；新增变体时必须追加到末尾以保持 ABI 兼容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AntiBotTech {
    /// Cloudflare：CF Ray / Challenge Platform / Just a moment 等
    Cloudflare,
    /// Akamai：BM BL / Reference # / _abck cookie 等
    Akamai,
    /// PerimeterX（HUMAN）：px-captcha / _pxhd cookie 等
    PerimeterX,
    /// DataDome：datadome cookie / captcha-delivery.com
    DataDome,
    /// Imperva（Incapsula）：incap_ses / visid_incap 等
    Imperva,
    /// Sucuri：sucuri_cloudproxy / X-Sucuri-ID
    Sucuri,
    /// Kasada：kd_448 / PointDefense
    Kasada,
    /// AWS WAF：captcha.awswaf.com / awsWafCookie
    AwsWaf,
    /// 速率限制（HTTP 429）
    RateLimited,
    /// 结构性阻断（JS 空壳 / 无可见正文 / 脚本重无内容）
    StructuralBlock,
    /// 已识别但未知归属
    Unknown,
}

// -----------------------------------------------------------------------------
// Tier1：高置信 WAF 结构标记（regex，任意页大小）
// -----------------------------------------------------------------------------

/// Tier1 单条模式：`(正则模式, 命中后归属的反爬技术)`
///
/// 高置信度结构标记，例如 `/cdn-cgi/challenge-platform/`、`_abck` cookie、
/// `Reference #18` Akamai 错误页等。命中任一即可直接判定。
///
/// 注：少量模式含 backreference（如 `Reference\s*#(\d+)`），故整体走 regex 而非 AC。
pub static TIER1_PATTERNS: &[(&str, AntiBotTech)] = &[
    // --- Cloudflare ---
    (r"(?i)/cdn-cgi/challenge-platform/", AntiBotTech::Cloudflare),
    (r"(?i)cf-browser-verification", AntiBotTech::Cloudflare),
    (r"(?i)cf-challenge", AntiBotTech::Cloudflare),
    (r"(?i)just a moment[\.\.\.]", AntiBotTech::Cloudflare),
    (r"(?i)__cf_bm", AntiBotTech::Cloudflare),
    (r"(?i)cf-ray[:\s]", AntiBotTech::Cloudflare),
    (
        r"(?i)cloudflare[\s\-_]?browser[\s\-_]?check",
        AntiBotTech::Cloudflare,
    ),
    // --- Akamai ---
    (r"(?i)reference\s*#(\d+)", AntiBotTech::Akamai),
    (r"(?i)akamai_bmbl", AntiBotTech::Akamai),
    (r"(?i)_abck\b", AntiBotTech::Akamai),
    (r"(?i)pardon our interruption", AntiBotTech::Akamai),
    // --- PerimeterX ---
    (r"(?i)px-captcha", AntiBotTech::PerimeterX),
    (r"(?i)perimeterx", AntiBotTech::PerimeterX),
    (r"(?i)_pxhd\b", AntiBotTech::PerimeterX),
    (r"(?i)px-cdn\.net", AntiBotTech::PerimeterX),
    // --- DataDome ---
    (r"(?i)datadome", AntiBotTech::DataDome),
    (r"(?i)captcha-delivery\.com", AntiBotTech::DataDome),
    (r"(?i)ct\.captcha-delivery", AntiBotTech::DataDome),
    // --- Imperva / Incapsula ---
    (r"(?i)incap_ses", AntiBotTech::Imperva),
    (r"(?i)visid_incap", AntiBotTech::Imperva),
    (r"(?i)incapsula", AntiBotTech::Imperva),
    // --- Sucuri ---
    (r"(?i)sucuri_cloudproxy", AntiBotTech::Sucuri),
    (r"(?i)x-sucuri-id", AntiBotTech::Sucuri),
    (r"(?i)sucuri[\s\-_]?firewall", AntiBotTech::Sucuri),
    // --- Kasada ---
    (r"(?i)kasada", AntiBotTech::Kasada),
    (r"(?i)pointdefense", AntiBotTech::Kasada),
    // --- AWS WAF ---
    (r"(?i)aws-waf", AntiBotTech::AwsWaf),
    (r"(?i)captcha\.awswaf\.com", AntiBotTech::AwsWaf),
    (r"(?i)awsWafCookie", AntiBotTech::AwsWaf),
];

/// Tier1 编译后的 `(Regex, AntiBotTech)` 列表
///
/// 首次访问编译一次，全局复用。`Regex::new` 在静态模式上失败属于构建期错误，
/// 故使用 `expect` 直接 panic（编译期已可验证模式合法性）。
pub static TIER1_REGEXES: Lazy<Vec<(Regex, AntiBotTech)>> = Lazy::new(|| {
    TIER1_PATTERNS
        .iter()
        .map(|(pat, tech)| (Regex::new(pat).expect("antibot tier1 regex"), *tech))
        .collect()
});

// -----------------------------------------------------------------------------
// Tier2：通用词字面量集（aho-corasick 自动机，仅 <10KB 页扫描）
// -----------------------------------------------------------------------------

/// Tier2 通用反爬提示词字面量表（小写匹配，使用 AC 自动机）
///
/// 这些词单独命中并不能高置信判定为反爬（可能是合法内容），故仅作为 4xx/5xx + 短页场景
/// 的辅助证据，结合状态码与 body 长度综合判断。
pub static TIER2_LITERALS: &[&str] = &[
    "access denied",
    "blocked",
    "captcha",
    "challenge",
    "forbidden",
    "rate limit",
    "rate-limited",
    "verify you are human",
    "checking your browser",
    "ddos protection",
    "please verify you are not a bot",
    "robot check",
    "security check",
    "human verification",
    "bot detection",
    "protection by",
    "are you a robot",
    "incapsula incident",
    "attention required",
    "unusual traffic",
];

/// Tier2 AhoCorasick 自动机（多模式一次扫描，ASCII 大小写不敏感）
///
/// 错误页常见 "Access Denied" / "BLOCKED" / "Captcha" 等大小写混合写法，
/// 故开启 `ascii_case_insensitive`，避免对 body 预先 lowercase 产生的 String 分配。
pub static TIER2_AUTOMATON: Lazy<AhoCorasick> = Lazy::new(|| {
    AhoCorasickBuilder::new()
        .ascii_case_insensitive(true)
        .build(TIER2_LITERALS)
        .expect("antibot tier2 automaton")
});

/// Tier2 触发的 body 大小上限（10 KB）
pub const TIER2_BODY_SIZE_LIMIT: usize = 10 * 1024;

// -----------------------------------------------------------------------------
// Tier3：结构完整性正则
// -----------------------------------------------------------------------------

/// Tier3-1：HTML 缺失 `<body>` 标签（任意大小写）
pub static TIER3_NO_BODY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)<body\b").expect("antibot tier3 no-body regex"));

/// Tier3-2：HTML `<script>` 块（用于统计脚本字节占比）
pub static TIER3_SCRIPT_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<script\b[^>]*>.*?</script>").expect("antibot tier3 script-block")
});

/// Tier3-3：所有 HTML 标签（用于剥离后统计可见文本长度）
pub static TIER3_ANY_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)<[^>]+>").expect("antibot tier3 any-tag"));

/// Tier3：判定 StructuralBlock 所需的最小命中信号数（无 body / 可见文本<50 / 脚本重无内容）
pub const TIER3_MIN_SIGNALS: usize = 2;

/// Tier3：可见文本长度阈值（小于此值视为可疑空壳）
pub const TIER3_VISIBLE_TEXT_MIN: usize = 50;

/// Tier3：脚本字节阈值（脚本总字节超过此值且可见文本 < 200 字符视为"脚本重无内容"）
pub const TIER3_SCRIPT_HEAVY_BYTES: usize = 5 * 1024;

/// Tier3：脚本重判定的辅助可见文本上限
pub const TIER3_SCRIPT_HEAVY_VISIBLE_MAX: usize = 200;

/// "近空页"判定阈值：200 状态码下 body 剥离空白后 < 此值视为 JS 空壳
pub const NEAR_EMPTY_BODY_LEN: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助：检测 body 是否命中指定 Tier1 tech
    fn tier1_match(body: &str, expected: AntiBotTech) -> bool {
        TIER1_REGEXES
            .iter()
            .find(|(re, _)| re.is_match(body))
            .map(|(_, t)| *t)
            .map_or(false, |t| t == expected)
    }

    // -------- Tier1：每个 WAF 至少 1 个标记测试（共 8 个 WAF） --------

    #[test]
    fn tier1_cloudflare_challenge_platform() {
        assert!(tier1_match(
            "<script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/jsch/v1\"></script>",
            AntiBotTech::Cloudflare
        ));
    }

    #[test]
    fn tier1_cloudflare_just_a_moment() {
        assert!(tier1_match(
            "<title>Just a moment...</title>",
            AntiBotTech::Cloudflare
        ));
    }

    #[test]
    fn tier1_akamai_reference() {
        assert!(tier1_match(
            "<p>Reference #18.abc123.456</p>",
            AntiBotTech::Akamai
        ));
    }

    #[test]
    fn tier1_akamai_pardon_interruption() {
        assert!(tier1_match(
            "<h1>Pardon Our Interruption</h1><p>As you were browsing something about your browser made us think you were a bot.</p>",
            AntiBotTech::Akamai
        ));
    }

    #[test]
    fn tier1_perimeterx_captcha() {
        assert!(tier1_match(
            "<div class=\"px-captcha\"></div>",
            AntiBotTech::PerimeterX
        ));
    }

    #[test]
    fn tier1_datadome_cookie() {
        assert!(tier1_match(
            "Set-Cookie: datadome=ABCDEF;",
            AntiBotTech::DataDome
        ));
    }

    #[test]
    fn tier1_imperva_incap_ses() {
        assert!(tier1_match(
            "Set-Cookie: incap_ses_123='XYZ';",
            AntiBotTech::Imperva
        ));
    }

    #[test]
    fn tier1_sucuri_cloudproxy() {
        assert!(tier1_match(
            "<!-- sucuri_cloudproxy -->",
            AntiBotTech::Sucuri
        ));
    }

    #[test]
    fn tier1_kasada_pointdefense() {
        assert!(tier1_match(
            "<script src=\"/pointdefense/agent.js\"></script>",
            AntiBotTech::Kasada
        ));
    }

    #[test]
    fn tier1_awswaf_captcha_host() {
        assert!(tier1_match(
            "<iframe src=\"https://captcha.awswaf.com/captcha/captcha.html\"></iframe>",
            AntiBotTech::AwsWaf
        ));
    }

    /// 验证 Tier1 至少包含 20 个模式（任务硬指标）
    #[test]
    fn tier1_pattern_count_meets_minimum() {
        assert!(
            TIER1_PATTERNS.len() >= 20,
            "expected >= 20 tier1 patterns, got {}",
            TIER1_PATTERNS.len()
        );
    }

    /// 验证所有 Tier1 正则可编译且 Lazy 加载成功
    #[test]
    fn tier1_regexes_compiled() {
        assert!(
            !TIER1_REGEXES.is_empty(),
            "tier1 regexes lazy should be non-empty"
        );
        // 触发 Lazy 强制初始化
        let _ = Lazy::force(&TIER1_REGEXES);
    }

    // -------- Tier2：AhoCorasick 自动机 --------

    #[test]
    fn tier2_automaton_matches_blocked() {
        assert!(TIER2_AUTOMATON.is_match("You have been blocked by our system"));
    }

    #[test]
    fn tier2_automaton_matches_captcha() {
        assert!(TIER2_AUTOMATON.is_match("please complete the CAPTCHA below"));
    }

    #[test]
    fn tier2_automaton_no_match_clean() {
        assert!(!TIER2_AUTOMATON.is_match("hello world this is a clean page"));
    }

    #[test]
    fn tier2_literal_count_meets_minimum() {
        assert!(
            TIER2_LITERALS.len() >= 10,
            "expected >= 10 tier2 literals, got {}",
            TIER2_LITERALS.len()
        );
    }

    #[test]
    fn tier2_body_size_limit_is_10kb() {
        assert_eq!(TIER2_BODY_SIZE_LIMIT, 10 * 1024);
    }

    // -------- Tier3：结构正则 --------

    #[test]
    fn tier3_no_body_detects_missing_body_tag() {
        // 只有 head 没有 body
        assert!(!TIER3_NO_BODY.is_match("<html><head><title>x</title></head></html>"));
    }

    #[test]
    fn tier3_no_body_passes_when_body_present() {
        assert!(TIER3_NO_BODY.is_match("<html><body>hi</body></html>"));
    }

    #[test]
    fn tier3_script_block_extracts_one_block() {
        let body = "<script>a=1</script><div>x</div><script>b=2</script>";
        let count = TIER3_SCRIPT_BLOCK.find_iter(body).count();
        assert_eq!(count, 2, "expected 2 script blocks, got {}", count);
    }

    #[test]
    fn tier3_any_tag_strips_tags() {
        let body = "<p>hello <b>world</b></p>";
        let stripped = TIER3_ANY_TAG.replace_all(body, "");
        assert_eq!(stripped, "hello world");
    }

    #[test]
    fn tier3_thresholds_sane() {
        assert_eq!(TIER3_MIN_SIGNALS, 2);
        assert_eq!(TIER3_VISIBLE_TEXT_MIN, 50);
        assert_eq!(TIER3_SCRIPT_HEAVY_BYTES, 5 * 1024);
        assert_eq!(TIER3_SCRIPT_HEAVY_VISIBLE_MAX, 200);
        assert_eq!(NEAR_EMPTY_BODY_LEN, 200);
    }
}
