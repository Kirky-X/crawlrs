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
}
