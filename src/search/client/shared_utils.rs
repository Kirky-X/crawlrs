// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Common utilities for search engine implementations
//!
//! Provides shared helper functions to reduce code duplication
//! across search engine client implementations.

use scraper::{ElementRef, Selector};

/// Safe CSS selector parser
///
/// Parses a CSS selector string and returns a Selector.
/// Returns None if the selector is invalid.
///
/// # Arguments
///
/// * `selector_str` - CSS selector string to parse
///
/// # Returns
///
/// Option containing the parsed Selector, or None if invalid
pub fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// Parse comma-separated CSS selectors and return first valid one
///
/// # Arguments
///
/// * `selectors` - Comma-separated CSS selector strings
///
/// # Returns
///
/// First valid Selector, or None if all are invalid
pub fn parse_first_selector(selectors: &str) -> Option<Selector> {
    selectors
        .split(',')
        .find_map(|s| safe_parse_selector(s.trim()))
}

/// Extract text content from an element, cleaning up whitespace
///
/// # Arguments
///
/// * `element` - A reference to an ElementRef
///
/// # Returns
///
/// Cleaned text content as a String
pub fn extract_clean_text_from_element(element: &ElementRef) -> String {
    element.text().collect::<String>().trim().to_string()
}

/// Escape HTML entities in text to prevent XSS attacks, then trim whitespace.
///
/// 统一封装 `html_escape::encode_text` + `trim`，消除 google.rs/sogou.rs/bing.rs
/// 中重复的 XSS 防护代码（架构 MEDIUM 4：抽出 HtmlParser 公共原语）。
///
/// # Arguments
///
/// * `text` - Raw text that may contain HTML special characters
///
/// # Returns
///
/// HTML-escaped and trimmed string
pub fn escape_html_text(text: &str) -> String {
    html_escape::encode_text(text).trim().to_string()
}

/// Build a URL query string from key-value pairs.
///
/// 使用 `fold` 累积构建查询字符串，避免 `collect::<Vec<_>>().join("&")` 的中间 Vec
/// 分配（性能 LOW-11：URL 构建优化）。统一 google.rs/smart/mod.rs 中 5 处重复的
/// query string 构建逻辑。
///
/// **安全**：key 和 value 都会通过 `urlencoding::encode` 进行 URL 编码，防止
/// `&` / `=` 注入破坏查询参数解析（安全 LOW-1 + 架构 LOW-3）。
///
/// **性能**：使用 `String::with_capacity` 预分配容量（按 value 编码后最大膨胀 3 倍
/// 估算），避免 `push_str` 触发多次扩容（性能 LOW-1）。
///
/// # Arguments
///
/// * `params` - Slice of (key, value) tuples; both key and value will be URL-encoded
///
/// # Returns
///
/// URL-encoded query string (e.g. "k1=v1&k2=v2")
pub fn build_query_string<S: AsRef<str>>(params: &[(&str, S)]) -> String {
    // 预估容量：key + '=' + value(编码后最大 3 倍) + '&' 分隔符
    let estimated: usize = params
        .iter()
        .map(|(k, v)| k.len() + 1 + v.as_ref().len() * 3 + 1)
        .sum();
    params
        .iter()
        .enumerate()
        .fold(String::with_capacity(estimated), |mut acc, (i, (k, v))| {
            if i > 0 {
                acc.push('&');
            }
            acc.push_str(&urlencoding::encode(k));
            acc.push('=');
            acc.push_str(&urlencoding::encode(v.as_ref()));
            acc
        })
}

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    use super::*;
    use scraper::Html;

    /// Parse an HTML fragment and run `f` with the first element matching the selector.
    ///
    /// The `Html` tree must outlive the `ElementRef`, so we pass the element
    /// to a closure instead of returning it.
    fn with_element<F, R>(html: &str, selector_str: &str, f: F) -> R
    where
        F: FnOnce(&ElementRef) -> R,
    {
        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse(selector_str).expect("selector must be valid");
        let element = fragment
            .select(&selector)
            .next()
            .expect("html must contain a matching element");
        f(&element)
    }

    // ========== safe_parse_selector tests ==========

    #[test]
    fn test_safe_parse_selector_valid_simple() {
        let result = safe_parse_selector("div");
        assert!(result.is_some(), "a simple tag selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_valid_class() {
        let result = safe_parse_selector(".container");
        assert!(result.is_some(), "a class selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_valid_id() {
        let result = safe_parse_selector("#main");
        assert!(result.is_some(), "an id selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_valid_compound() {
        let result = safe_parse_selector("div.container > p#intro");
        assert!(result.is_some(), "a compound selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_valid_attribute() {
        let result = safe_parse_selector("a[href]");
        assert!(result.is_some(), "an attribute selector should parse");
    }

    #[test]
    fn test_safe_parse_selector_invalid_empty() {
        let result = safe_parse_selector("");
        assert!(result.is_none(), "an empty selector should not parse");
    }

    #[test]
    fn test_safe_parse_selector_invalid_combinator() {
        // A bare combinator is not a valid selector.
        let result = safe_parse_selector(">");
        assert!(result.is_none(), "a bare combinator should not parse");
    }

    #[test]
    fn test_safe_parse_selector_invalid_pseudo_only() {
        // A bare pseudo-class without a selector is invalid.
        let result = safe_parse_selector(":hover");
        assert!(result.is_none(), "a bare pseudo-class should not parse");
    }

    // ========== parse_first_selector tests ==========

    #[test]
    fn test_parse_first_selector_returns_first_valid() {
        let result = parse_first_selector("div, span, p");
        assert!(result.is_some(), "should return the first valid selector");
    }

    #[test]
    fn test_parse_first_selector_skips_invalid_leading() {
        // The first entry is invalid (bare combinator), the second is valid.
        let result = parse_first_selector(">, div");
        assert!(
            result.is_some(),
            "should skip invalid entries and return the first valid one"
        );
    }

    #[test]
    fn test_parse_first_selector_all_invalid_returns_none() {
        let result = parse_first_selector(">, :, +");
        assert!(
            result.is_none(),
            "should return None when all entries are invalid"
        );
    }

    #[test]
    fn test_parse_first_selector_empty_string_returns_none() {
        let result = parse_first_selector("");
        assert!(result.is_none(), "an empty string should yield no selector");
    }

    #[test]
    fn test_parse_first_selector_trims_whitespace() {
        let result = parse_first_selector("  div  , span");
        assert!(
            result.is_some(),
            "whitespace around selectors should be trimmed"
        );
    }

    #[test]
    fn test_parse_first_selector_single_valid() {
        let result = parse_first_selector(".item");
        assert!(result.is_some(), "a single valid selector should parse");
    }

    // ========== extract_clean_text_from_element tests ==========

    #[test]
    fn test_extract_clean_text_simple() {
        let text = with_element("<div>Hello World</div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_extract_clean_text_trims_leading_trailing_whitespace() {
        let text = with_element("<div>   Hello   </div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_extract_clean_text_preserves_inner_whitespace() {
        // ElementRef::text() concatenates text nodes without collapsing inner
        // whitespace; trim only removes leading/trailing whitespace.
        let text = with_element("<div>Hello\n  World</div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "Hello\n  World");
    }

    #[test]
    fn test_extract_clean_text_nested_elements() {
        let text = with_element("<div>Hello <span>World</span></div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_extract_clean_text_empty_element() {
        let text = with_element("<div></div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "");
    }

    #[test]
    fn test_extract_clean_text_only_whitespace() {
        let text = with_element("<div>   </div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        assert_eq!(text, "");
    }

    #[test]
    fn test_extract_clean_text_with_nested_whitespace() {
        let text = with_element("<div>  <span>  Hello  </span>  </div>", "div", |element| {
            extract_clean_text_from_element(element)
        });
        // trim removes leading/trailing whitespace from the collected string.
        assert_eq!(text, "Hello");
    }

    // ========== escape_html_text tests ==========

    #[test]
    fn test_escape_html_text_plain_text_unchanged() {
        assert_eq!(
            escape_html_text("Rust Programming Language"),
            "Rust Programming Language"
        );
    }

    #[test]
    fn test_escape_html_text_special_chars_encoded() {
        let text = "<script>alert('xss')</script> & more";
        let escaped = escape_html_text(text);
        assert!(!escaped.contains('<'), "should not contain raw <");
        assert!(!escaped.contains('>'), "should not contain raw >");
        assert!(escaped.contains("&lt;"), "should contain &lt;");
        assert!(escaped.contains("&gt;"), "should contain &gt;");
        assert!(escaped.contains("&amp;"), "should contain &amp;");
    }

    #[test]
    fn test_escape_html_text_empty_string() {
        assert_eq!(escape_html_text(""), "");
    }

    #[test]
    fn test_escape_html_text_trims_whitespace() {
        assert_eq!(escape_html_text("  trimmed content  "), "trimmed content");
    }

    // ========== build_query_string tests ==========

    #[test]
    fn test_build_query_string_single_param() {
        let params = vec![("q", "rust".to_string())];
        assert_eq!(build_query_string(&params), "q=rust");
    }

    #[test]
    fn test_build_query_string_multiple_params() {
        let params = vec![
            ("q", "rust lang".to_string()),
            ("ie", "utf8".to_string()),
            ("num", "10".to_string()),
        ];
        assert_eq!(build_query_string(&params), "q=rust%20lang&ie=utf8&num=10");
    }

    #[test]
    fn test_build_query_string_empty_params() {
        let params: Vec<(&str, String)> = vec![];
        assert_eq!(build_query_string(&params), "");
    }

    #[test]
    fn test_build_query_string_url_encodes_special_chars() {
        let params = vec![("q", "a&b=c?d".to_string())];
        let result = build_query_string(&params);
        assert!(
            !result.contains('&') || result.matches('&').count() == 0 || result.starts_with("q=a"),
            "special chars should be encoded: {}",
            result
        );
        // a&b=c?d 编码后不应包含裸 & 分隔符以外的 &
        assert_eq!(result, "q=a%26b%3Dc%3Fd");
    }

    #[test]
    fn test_build_query_string_encodes_non_ascii() {
        let params = vec![("wd", "中文搜索".to_string())];
        let result = build_query_string(&params);
        assert!(
            result.starts_with("wd="),
            "should start with key: {}",
            result
        );
        assert!(
            !result.contains('中') && !result.contains('文'),
            "non-ASCII should be percent-encoded: {}",
            result
        );
    }
}
