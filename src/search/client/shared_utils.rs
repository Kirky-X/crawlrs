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
}
