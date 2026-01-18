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
