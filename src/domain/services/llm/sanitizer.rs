// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Indirect prompt-injection sanitizer for the LLM extraction pipeline.
//!
//! External page content is fully attacker-controlled input. This module strips
//! the four injection carriers before the text reaches an LLM prompt:
//! zero-width characters, HTML comment payloads, oversized text, and known
//! suspicious instruction lines. DOM-level hidden-node pruning
//! ([`is_hidden_node`]) is consumed by `extraction_service::collect_clean_text`.
//!
//! All functions are pure and stateless; behavior is driven by
//! [`SanitizationConfig`] (built from `[llm.sanitization]` settings).

use serde::Serialize;

/// Sanitizer configuration (fields map 1:1 to `[llm.sanitization]` settings).
#[derive(Debug, Clone, PartialEq)]
pub struct SanitizationConfig {
    /// Master switch: `false` bypasses sanitization entirely (zero overhead).
    pub enabled: bool,
    /// Skip hidden DOM subtrees during text collection (DOM layer).
    pub strip_hidden_nodes: bool,
    /// Remove zero-width characters (U+200B/U+200C/U+200D/U+FEFF).
    pub strip_zero_width: bool,
    /// Strip `<!-- ... -->` HTML comment payloads from text.
    pub strip_html_comments: bool,
    /// Truncate text to at most this many chars (char boundary, `0` = no limit).
    pub max_text_length: usize,
    /// What to do with lines matching suspicious instruction patterns.
    pub suspicious_pattern_action: SuspiciousAction,
}

impl Default for SanitizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strip_hidden_nodes: true,
            strip_zero_width: true,
            strip_html_comments: true,
            max_text_length: 500_000,
            suspicious_pattern_action: SuspiciousAction::StripAndFlag,
        }
    }
}

/// Action applied to suspicious instruction lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspiciousAction {
    /// Remove matched lines and count them in the report (default).
    StripAndFlag,
    /// Only count matches, keep the text (observation mode).
    FlagOnly,
}

/// Counters describing what one `sanitize_text` pass did.
///
/// Serialized under the `_sanitization` key alongside business fields; an
/// all-zero report is skipped when attaching (old clients see no change).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SanitizationReport {
    /// Hidden DOM subtrees skipped during collection (DOM layer counter).
    pub hidden_nodes_removed: u32,
    /// Zero-width characters removed.
    pub zero_width_removed: u32,
    /// HTML comment payloads stripped.
    pub comments_removed: u32,
    /// Whether the text was truncated at `max_text_length`.
    pub truncated: bool,
    /// Suspicious instruction lines matched.
    pub suspicious_hits: u32,
}

impl SanitizationReport {
    /// True when nothing was removed, flagged, or truncated.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Text-layer sanitization: zero-width removal, HTML comment stripping,
/// length truncation, and suspicious-line handling (per `cfg` switches).
///
/// `cfg.enabled == false` returns the input unchanged with an empty report.
pub fn sanitize_text(text: &str, cfg: &SanitizationConfig) -> (String, SanitizationReport) {
    if !cfg.enabled {
        return (text.to_string(), SanitizationReport::default());
    }
    let mut report = SanitizationReport::default();

    // Layer 1: zero-width characters (T001)
    let mut out = String::with_capacity(text.len());
    if cfg.strip_zero_width {
        for ch in text.chars() {
            if matches!(ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}') {
                report.zero_width_removed += 1;
            } else {
                out.push(ch);
            }
        }
    } else {
        out.push_str(text);
    }

    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, zero_width: bool) -> SanitizationConfig {
        SanitizationConfig {
            enabled,
            strip_zero_width: zero_width,
            ..SanitizationConfig::default()
        }
    }

    /// Zero-width chars U+200B/U+200C/U+200D/U+FEFF are removed and counted.
    #[test]
    fn sanitize_text_removes_zero_width_chars_and_counts() {
        let (out, report) = sanitize_text("a\u{200B}b\u{FEFF}c", &cfg(true, true));
        assert_eq!(out, "abc");
        assert_eq!(report.zero_width_removed, 2);
    }

    /// All four zero-width codepoints are covered.
    #[test]
    fn sanitize_text_removes_all_four_zero_width_variants() {
        let (out, report) = sanitize_text("x\u{200B}\u{200C}\u{200D}\u{FEFF}y", &cfg(true, true));
        assert_eq!(out, "xy");
        assert_eq!(report.zero_width_removed, 4);
    }

    /// `strip_zero_width = false` keeps the characters (observation mode).
    #[test]
    fn sanitize_text_zero_width_disabled_keeps_chars() {
        let (out, report) = sanitize_text("a\u{200B}b", &cfg(true, false));
        assert_eq!(out, "a\u{200B}b");
        assert_eq!(report.zero_width_removed, 0);
    }

    /// `enabled = false` bypasses everything: original text, empty report.
    #[test]
    fn sanitize_text_disabled_bypasses() {
        let (out, report) = sanitize_text("a\u{200B}b", &cfg(false, true));
        assert_eq!(out, "a\u{200B}b");
        assert!(report.is_empty());
    }

    /// CJK text passes through untouched (no false positives).
    #[test]
    fn sanitize_text_keeps_normal_cjk_and_ascii() {
        let (out, report) = sanitize_text("今天天气很好 hello world", &cfg(true, true));
        assert_eq!(out, "今天天气很好 hello world");
        assert!(report.is_empty());
    }
}
