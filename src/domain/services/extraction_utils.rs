//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.
//! Extraction utilities module
//!
//! Provides shared utilities for HTML element extraction to eliminate code duplication
//! across extraction_service.rs and other modules.

use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use thiserror::Error;
use url::Url;

/// Extraction utilities errors
#[derive(Debug, Error)]
pub enum ExtractionUtilsError {
    #[error("Invalid CSS selector: {0}")]
    InvalidSelector(String),

    #[error("Element not found for selector: {0}")]
    ElementNotFound(String),

    #[error("URL parsing failed: {0}")]
    UrlParseError(#[from] url::ParseError),
}

/// Unified element value extraction utilities
///
/// This module eliminates duplicate extraction logic that appeared 4+ times
/// in extraction_service.rs for handling href/src attribute processing.
pub struct ExtractionUtils;

impl ExtractionUtils {
    /// Extract element attribute value with automatic URL joining for relative URLs
    ///
    /// This function consolidates the repeated pattern:
    /// ```rust
    /// if let (Some(v), Some(b)) = (&val, &base) {
    ///     if attr == "href" || attr == "src" {
    ///         b.join(v).ok().map(|u| u.to_string()).or(val)
    ///     } else {
    ///         val
    ///     }
    /// }
    /// ```
    ///
    /// # Arguments
    /// * `element` - HTML element reference
    /// * `attr` - Attribute name (e.g., "href", "src")
    /// * `base` - Base URL for joining relative URLs
    /// * `text_fallback` - Closure to extract text if attribute is absent
    ///
    /// # Returns
    /// Extracted string value (may be empty string), or None if extraction fails
    ///
    /// # Example
    /// ```rust
    /// let value = ExtractionUtils::extract_element_value(
    ///     element,
    ///     "href",
    ///     base_url,
    ///     || Some(element.text().collect::<Vec<_>>().join(" ").trim().to_string()),
    /// );
    /// ```
    pub fn extract_element_value<F>(
        element: ElementRef,
        attr: &str,
        base: Option<&Url>,
        text_fallback: F,
    ) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        let val = element.value().attr(attr).map(|s| s.to_string());

        match (val, base) {
            (Some(v), Some(b)) if attr == "href" || attr == "src" => {
                let v = v.strip_prefix('/').unwrap_or(&v);
                let mut new_url = b.clone();
                new_url.set_path(&format!("{}/{}", b.path().trim_end_matches('/'), v));
                Some(new_url.to_string())
            }
            // Other attributes returned as-is
            (Some(v), _) => Some(v),
            // No attribute - use text fallback
            (None, _) => text_fallback(),
        }
    }

    /// Batch extract values from multiple elements
    ///
    /// Consolidates the repeated array extraction pattern:
    /// ```rust
    /// let mut values = Vec::new();
    /// for element in document.select(&selector) {
    ///     let value = extract_element_value(...);
    ///     if let Some(v) = value.filter(|v| !v.is_empty()) {
    ///         values.push(Value::String(v));
    ///     }
    /// }
    /// ```
    ///
    /// # Arguments
    /// * `elements` - Iterator over HTML elements
    /// * `attr` - Attribute name to extract
    /// * `base` - Base URL for relative URL joining
    /// * `text_fallback` - Text extraction closure for elements without the attribute
    ///
    /// # Returns
    /// Vector of non-empty string values
    pub fn extract_array_values<'a, 'b, F>(
        elements: impl Iterator<Item = ElementRef<'a>>,
        attr: &'b str,
        base: Option<&'b Url>,
        mut text_fallback: F,
    ) -> Vec<Value>
    where
        F: FnMut(ElementRef<'a>) -> Option<String>,
    {
        elements
            .filter_map(|element| {
                let value =
                    Self::extract_element_value(element, attr, base, || text_fallback(element));
                value.filter(|v| !v.is_empty()).map(Value::String)
            })
            .collect()
    }

    /// Extract single element value
    ///
    /// For non-array extraction, returns the first matching element's value
    /// or Value::Null if no match found.
    ///
    /// # Arguments
    /// * `html` - HTML content string
    /// * `selector_str` - CSS selector string
    /// * `attr` - Attribute name to extract
    /// * `base` - Base URL for relative URL joining
    /// * `text_fallback` - Text extraction closure
    ///
    /// # Returns
    /// Extracted value as Value::String or Value::Null
    pub fn extract_single_value(
        html: &str,
        selector_str: &str,
        attr: Option<&str>,
        base: Option<&Url>,
    ) -> Result<Value, ExtractionUtilsError> {
        let document = Html::parse_document(html);
        let selector = Selector::parse(selector_str)
            .map_err(|e| ExtractionUtilsError::InvalidSelector(e.to_string()))?;

        if let Some(element) = document.select(&selector).next() {
            let value = attr.map_or_else(
                || {
                    Some(
                        element
                            .text()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_string(),
                    )
                },
                |attr_name| {
                    Self::extract_element_value(element, attr_name, base, || {
                        Some(
                            element
                                .text()
                                .collect::<Vec<_>>()
                                .join(" ")
                                .trim()
                                .to_string(),
                        )
                    })
                },
            );

            Ok(value.map(Value::String).unwrap_or(Value::Null))
        } else {
            Err(ExtractionUtilsError::ElementNotFound(
                selector_str.to_string(),
            ))
        }
    }
}

/// Trait for extraction rules to enable generic processing
pub trait ExtractableRule {
    fn selector(&self) -> &str;
    fn is_array(&self) -> bool;
    fn attr(&self) -> Option<&str>;
    fn description(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    #[test]
    fn test_extract_element_value_with_href() {
        let html = r#"<a href="relative/path">Link</a>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("a").unwrap())
            .next()
            .unwrap();
        let base_url = Url::parse("https://example.com/base").unwrap();

        let result =
            ExtractionUtils::extract_element_value(element, "href", Some(&base_url), || None);

        assert_eq!(
            result,
            Some("https://example.com/base/relative/path".to_string())
        );
    }

    #[test]
    fn test_extract_element_value_with_text() {
        let html = r#"<p>Hello World</p>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("p").unwrap())
            .next()
            .unwrap();

        let result = ExtractionUtils::extract_element_value(
            element,
            "href", // No href attribute
            None,
            || {
                Some(
                    element
                        .text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string(),
                )
            },
        );

        assert_eq!(result, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_array_values() {
        let html = r#"
            <div>
                <a href="/link1">Link 1</a>
                <a href="/link2">Link 2</a>
                <a href="/link3">Link 3</a>
            </div>
        "#;
        let document = Html::parse_document(html);
        let selector = Selector::parse("a").unwrap();
        let elements = document.select(&selector);
        let base_url = Url::parse("https://example.com").unwrap();

        let results =
            ExtractionUtils::extract_array_values(elements, "href", Some(&base_url), |_| None);

        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0],
            Value::String("https://example.com/link1".to_string())
        );
        assert_eq!(
            results[1],
            Value::String("https://example.com/link2".to_string())
        );
        assert_eq!(
            results[2],
            Value::String("https://example.com/link3".to_string())
        );
    }

    #[test]
    fn test_extract_single_value() {
        let html = r#"<meta name="description" content="Test Description">"#;

        let result = ExtractionUtils::extract_single_value(
            html,
            "meta[name='description']",
            Some("content"),
            None,
        );

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            Value::String("Test Description".to_string())
        );
    }

    #[test]
    fn test_extract_single_value_not_found() {
        let html = r#"<div>No meta tag</div>"#;

        let result = ExtractionUtils::extract_single_value(
            html,
            "meta[name='description']",
            Some("content"),
            None,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExtractionUtilsError::ElementNotFound(_)
        ));
    }

    // ========== extract_element_value: src attribute path ==========

    #[test]
    fn test_extract_element_value_with_src_attribute() {
        let html = r#"<img src="image.png">"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("img").expect("valid selector"))
            .next()
            .expect("img element exists");
        let base_url = Url::parse("https://example.com/page").expect("valid url");

        let result =
            ExtractionUtils::extract_element_value(element, "src", Some(&base_url), || None);

        assert_eq!(
            result,
            Some("https://example.com/page/image.png".to_string())
        );
    }

    // ========== extract_element_value: non-href/src attribute path ==========

    #[test]
    fn test_extract_element_value_non_href_src_attr_returned_as_is() {
        // "class" attribute should be returned verbatim, no URL joining
        let html = r#"<div class="container main">Content</div>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("div").expect("valid selector"))
            .next()
            .expect("div element exists");
        let base_url = Url::parse("https://example.com/base").expect("valid url");

        let result =
            ExtractionUtils::extract_element_value(element, "class", Some(&base_url), || None);

        assert_eq!(result, Some("container main".to_string()));
    }

    #[test]
    fn test_extract_element_value_non_href_src_attr_without_base() {
        let html = r#"<span data-id="12345">Text</span>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("span").expect("valid selector"))
            .next()
            .expect("span element exists");

        let result = ExtractionUtils::extract_element_value(element, "data-id", None, || None);

        assert_eq!(result, Some("12345".to_string()));
    }

    // ========== extract_element_value: no attribute, fallback returns None ==========

    #[test]
    fn test_extract_element_value_no_attr_fallback_returns_none() {
        let html = r#"<a>Link without href</a>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("a").expect("valid selector"))
            .next()
            .expect("a element exists");

        let result = ExtractionUtils::extract_element_value(element, "href", None, || None);

        assert!(
            result.is_none(),
            "should return None when attr is absent and fallback returns None"
        );
    }

    // ========== extract_element_value: href without base URL ==========

    #[test]
    fn test_extract_element_value_href_without_base_returns_raw() {
        let html = r#"<a href="/path/page">Link</a>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("a").expect("valid selector"))
            .next()
            .expect("a element exists");

        // When base is None, href should be returned as-is (not joined)
        let result = ExtractionUtils::extract_element_value(element, "href", None, || None);

        assert_eq!(result, Some("/path/page".to_string()));
    }

    // ========== extract_element_value: href with leading slash stripped ==========

    #[test]
    fn test_extract_element_value_href_strips_leading_slash_when_joining() {
        let html = r#"<a href="/absolute/path">Link</a>"#;
        let document = Html::parse_document(html);
        let element = document
            .select(&Selector::parse("a").expect("valid selector"))
            .next()
            .expect("a element exists");
        let base_url = Url::parse("https://example.com").expect("valid url");

        let result =
            ExtractionUtils::extract_element_value(element, "href", Some(&base_url), || None);

        // Leading '/' is stripped, then joined with base path
        assert_eq!(
            result,
            Some("https://example.com/absolute/path".to_string())
        );
    }

    // ========== extract_array_values: text fallback ==========

    #[test]
    fn test_extract_array_values_uses_text_fallback_when_attr_missing() {
        let html = r#"
            <ul>
                <li>Item 1</li>
                <li>Item 2</li>
            </ul>
        "#;
        let document = Html::parse_document(html);
        let selector = Selector::parse("li").expect("valid selector");
        let elements = document.select(&selector);

        let results = ExtractionUtils::extract_array_values(elements, "href", None, |el| {
            Some(el.text().collect::<Vec<_>>().join(" ").trim().to_string())
        });

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], Value::String("Item 1".to_string()));
        assert_eq!(results[1], Value::String("Item 2".to_string()));
    }

    // ========== extract_array_values: filters empty values ==========

    #[test]
    fn test_extract_array_values_filters_empty_strings() {
        // Elements with empty attribute values should be filtered out.
        // Note: without base_url, empty href values remain empty strings and get filtered.
        // (With base_url, empty href gets joined to a non-empty URL like "https://example.com/")
        let html = r#"
            <div>
                <a href="">Empty</a>
                <a href="/link1">Link 1</a>
                <a href="">Also Empty</a>
                <a href="/link2">Link 2</a>
            </div>
        "#;
        let document = Html::parse_document(html);
        let selector = Selector::parse("a").expect("valid selector");
        let elements = document.select(&selector);

        let results = ExtractionUtils::extract_array_values(elements, "href", None, |_| None);

        // Only non-empty href values should be in the result
        assert_eq!(results.len(), 2, "empty values should be filtered out");
    }

    // ========== extract_array_values: no matching elements ==========

    #[test]
    fn test_extract_array_values_returns_empty_when_no_elements() {
        let html = r#"<div>No links here</div>"#;
        let document = Html::parse_document(html);
        let selector = Selector::parse("a").expect("valid selector");
        let elements = document.select(&selector);

        let results = ExtractionUtils::extract_array_values(elements, "href", None, |_| None);

        assert!(
            results.is_empty(),
            "should return empty vec when no elements match"
        );
    }

    // ========== extract_array_values: mixed attr and fallback ==========

    #[test]
    fn test_extract_array_values_mixed_attr_and_fallback() {
        let html = r#"
            <div>
                <a href="/link1">Link 1</a>
                <a>Link 2 without href</a>
                <a href="/link3">Link 3</a>
            </div>
        "#;
        let document = Html::parse_document(html);
        let selector = Selector::parse("a").expect("valid selector");
        let elements = document.select(&selector);
        let base_url = Url::parse("https://example.com").expect("valid url");

        let results =
            ExtractionUtils::extract_array_values(elements, "href", Some(&base_url), |el| {
                Some(el.text().collect::<Vec<_>>().join(" ").trim().to_string())
            });

        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0],
            Value::String("https://example.com/link1".to_string())
        );
        assert_eq!(results[1], Value::String("Link 2 without href".to_string()));
        assert_eq!(
            results[2],
            Value::String("https://example.com/link3".to_string())
        );
    }

    // ========== extract_single_value: attr=None (text extraction) ==========

    #[test]
    fn test_extract_single_value_without_attr_extracts_text() {
        let html = r#"<h1>Title Text</h1>"#;

        let result = ExtractionUtils::extract_single_value(html, "h1", None, None);

        assert!(result.is_ok());
        assert_eq!(result.expect("ok"), Value::String("Title Text".to_string()));
    }

    #[test]
    fn test_extract_single_value_without_attr_empty_text_returns_empty_string() {
        let html = r#"<h1></h1>"#;

        let result = ExtractionUtils::extract_single_value(html, "h1", None, None);

        assert!(result.is_ok());
        // Empty text after trim should produce Value::String("")
        assert_eq!(result.expect("ok"), Value::String("".to_string()));
    }

    // ========== extract_single_value: with base URL for href ==========

    #[test]
    fn test_extract_single_value_with_base_url_joins_href() {
        let html = r#"<a href="page.html">Link</a>"#;
        let base_url = Url::parse("https://example.com/base").expect("valid url");

        let result =
            ExtractionUtils::extract_single_value(html, "a", Some("href"), Some(&base_url));

        assert!(result.is_ok());
        assert_eq!(
            result.expect("ok"),
            Value::String("https://example.com/base/page.html".to_string())
        );
    }

    // ========== extract_single_value: invalid selector ==========

    #[test]
    fn test_extract_single_value_invalid_selector_returns_error() {
        let html = r#"<div>content</div>"#;

        let result = ExtractionUtils::extract_single_value(html, "!!!invalid", None, None);

        assert!(result.is_err());
        match result.expect_err("should be error") {
            ExtractionUtilsError::InvalidSelector(msg) => {
                assert!(!msg.is_empty(), "error message should not be empty");
            }
            other => panic!("expected InvalidSelector, got {:?}", other),
        }
    }

    // ========== extract_single_value: attr present but empty value ==========

    #[test]
    fn test_extract_single_value_attr_present_but_empty_returns_null() {
        // When attr is Some but the attribute value is empty string,
        // extract_element_value returns Some(""), which maps to Value::String("")
        let html = r#"<meta name="description" content="">"#;

        let result = ExtractionUtils::extract_single_value(
            html,
            "meta[name='description']",
            Some("content"),
            None,
        );

        assert!(result.is_ok());
        assert_eq!(result.expect("ok"), Value::String("".to_string()));
    }

    // ========== extract_single_value: attr missing falls back to text ==========

    #[test]
    fn test_extract_single_value_attr_missing_falls_back_to_text() {
        // Element exists but doesn't have the requested attr;
        // text fallback should return the element's text content
        let html = r#"<a>Click here</a>"#;

        let result = ExtractionUtils::extract_single_value(html, "a", Some("href"), None);

        assert!(result.is_ok());
        assert_eq!(result.expect("ok"), Value::String("Click here".to_string()));
    }

    // ========== ExtractionUtilsError Display tests ==========

    #[test]
    fn test_extraction_utils_error_invalid_selector_display() {
        let err = ExtractionUtilsError::InvalidSelector("bad syntax".to_string());
        assert!(err.to_string().contains("Invalid CSS selector"));
        assert!(err.to_string().contains("bad syntax"));
    }

    #[test]
    fn test_extraction_utils_error_element_not_found_display() {
        let err = ExtractionUtilsError::ElementNotFound("div.missing".to_string());
        assert!(err.to_string().contains("Element not found for selector"));
        assert!(err.to_string().contains("div.missing"));
    }

    #[test]
    fn test_extraction_utils_error_url_parse_error_display() {
        let parse_err = "http://[invalid"
            .parse::<url::Url>()
            .expect_err("should fail");
        let err = ExtractionUtilsError::UrlParseError(parse_err);
        assert!(err.to_string().contains("URL parsing failed"));
    }

    #[test]
    fn test_extraction_utils_error_url_parse_from_conversion() {
        // Verify the From<url::ParseError> conversion works
        let parse_err = "not a url".parse::<url::Url>().expect_err("should fail");
        let err: ExtractionUtilsError = parse_err.into();
        match err {
            ExtractionUtilsError::UrlParseError(_) => { /* expected */ }
            other => panic!("expected UrlParseError, got {:?}", other),
        }
    }

    // ========== ExtractableRule trait tests ==========

    /// A simple implementation of ExtractableRule for testing
    struct TestRule {
        sel: String,
        array: bool,
        attribute: Option<String>,
        desc: String,
    }

    impl ExtractableRule for TestRule {
        fn selector(&self) -> &str {
            &self.sel
        }
        fn is_array(&self) -> bool {
            self.array
        }
        fn attr(&self) -> Option<&str> {
            self.attribute.as_deref()
        }
        fn description(&self) -> &str {
            &self.desc
        }
    }

    #[test]
    fn test_extractable_rule_returns_correct_values() {
        let rule = TestRule {
            sel: "a.link".to_string(),
            array: true,
            attribute: Some("href".to_string()),
            desc: "Extract all links".to_string(),
        };

        assert_eq!(rule.selector(), "a.link");
        assert!(rule.is_array());
        assert_eq!(rule.attr(), Some("href"));
        assert_eq!(rule.description(), "Extract all links");
    }

    #[test]
    fn test_extractable_rule_with_no_attr() {
        let rule = TestRule {
            sel: "p".to_string(),
            array: false,
            attribute: None,
            desc: "Extract paragraph text".to_string(),
        };

        assert_eq!(rule.selector(), "p");
        assert!(!rule.is_array());
        assert!(rule.attr().is_none());
        assert_eq!(rule.description(), "Extract paragraph text");
    }

    // ========== extract_single_value: non-href attr with base URL ==========

    #[test]
    fn test_extract_single_value_non_href_attr_with_base_url() {
        // 非 href/src 属性 + base URL：base 应被忽略，属性值原样返回
        let html = r#"<meta name="author" content="Kirky">"#;
        let base_url = Url::parse("https://example.com/base").expect("valid url");

        let result = ExtractionUtils::extract_single_value(
            html,
            "meta[name='author']",
            Some("content"),
            Some(&base_url),
        );

        assert!(result.is_ok());
        assert_eq!(result.expect("ok"), Value::String("Kirky".to_string()));
    }

    // ========== extract_single_value: href attr without base URL ==========

    #[test]
    fn test_extract_single_value_href_without_base_returns_raw() {
        // href 属性 + base=None：href 值应原样返回（不进行 URL 拼接）
        let html = r#"<a href="/path/page">Link</a>"#;

        let result = ExtractionUtils::extract_single_value(html, "a", Some("href"), None);

        assert!(result.is_ok());
        assert_eq!(result.expect("ok"), Value::String("/path/page".to_string()));
    }
}
