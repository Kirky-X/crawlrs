// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Lightweight PII detection and minimization for extracted JSON content.
//!
//! Detects email addresses, E.164 phone numbers, IPv4 addresses and
//! ID-card-like digit strings, then masks them in place with conservative
//! policies that keep enough structure for debugging while removing identifiers.
//! The aho-corasick anchor prefilter is compiled under `content` (where the
//! dependency lives); a regex-only path keeps this module compilable in minimal
//! builds.

use std::collections::HashMap;

use regex::Regex;
use serde::Serialize;

/// Counts of PII masking operations.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PiiStats {
    /// Number of string fields changed.
    pub fields_masked: u32,
    /// Hits by PII kind (`email`, `phone`, `ipv4`, `id_card`).
    pub hits_by_kind: HashMap<&'static str, u32>,
}

impl PiiStats {
    fn bump(&mut self, kind: &'static str) {
        *self.hits_by_kind.entry(kind).or_insert(0) += 1;
    }
}

/// Recursive JSON PII minimizer.
pub struct PiiMinimizer {
    email: Regex,
    phone: Regex,
    ipv4: Regex,
    id_card: Regex,
    #[cfg(feature = "content")]
    anchors: aho_corasick::AhoCorasick,
}

impl Default for PiiMinimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl PiiMinimizer {
    /// Build the detector set (regexes are precompiled once).
    pub fn new() -> Self {
        Self {
            email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
                .expect("email regex"),
            phone: Regex::new(r"\+\d{7,15}").expect("phone regex"),
            ipv4: Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").expect("ipv4 regex"),
            id_card: Regex::new(r"\b\d{18}\b|\b\d{15}\b").expect("id card regex"),
            #[cfg(feature = "content")]
            anchors: aho_corasick::AhoCorasick::new(["@", "+", "."]).expect("anchor automaton"),
        }
    }

    /// Quick pre-check: whether a string can contain any PII anchor.
    /// ID cards are pure digit runs, so a long-digit signal is also an anchor.
    fn has_long_digits(s: &str) -> bool {
        let mut run = 0usize;
        for ch in s.chars() {
            if ch.is_ascii_digit() {
                run += 1;
                if run >= 15 {
                    return true;
                }
            } else {
                run = 0;
            }
        }
        false
    }

    #[cfg(feature = "content")]
    fn has_anchor(&self, s: &str) -> bool {
        self.anchors.is_match(s) || Self::has_long_digits(s)
    }

    #[cfg(not(feature = "content"))]
    fn has_anchor(&self, s: &str) -> bool {
        s.contains('@') || s.contains('+') || s.contains('.') || Self::has_long_digits(s)
    }

    /// Recursively mask PII in a JSON value, returning aggregate stats.
    pub fn minimize_value(&self, value: &mut serde_json::Value) -> PiiStats {
        let mut stats = PiiStats::default();
        self.mask(value, &mut stats);
        stats
    }

    fn mask(&self, value: &mut serde_json::Value, stats: &mut PiiStats) {
        match value {
            serde_json::Value::Object(map) => {
                for v in map.values_mut() {
                    self.mask(v, stats);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr.iter_mut() {
                    self.mask(v, stats);
                }
            }
            serde_json::Value::String(s) => {
                if !self.has_anchor(s) {
                    return;
                }
                let original = s.clone();
                let masked = self.mask_string(s, stats);
                if masked != original {
                    *s = masked;
                    stats.fields_masked += 1;
                }
            }
            _ => {}
        }
    }

    fn mask_string(&self, s: &str, stats: &mut PiiStats) -> String {
        let mut out = self
            .email
            .replace_all(s, |caps: &regex::Captures| {
                stats.bump("email");
                let whole = &caps[0];
                let domain = whole.split('@').nth(1).unwrap_or("");
                format!("u***@{domain}")
            })
            .to_string();
        out = self
            .phone
            .replace_all(&out, |caps: &regex::Captures| {
                stats.bump("phone");
                let whole = &caps[0];
                let keep = &whole[..whole.len().min(4)];
                format!("{keep}***")
            })
            .to_string();
        out = self
            .ipv4
            .replace_all(&out, |caps: &regex::Captures| {
                stats.bump("ipv4");
                let parts: Vec<&str> = caps[0].split('.').collect();
                if parts.len() == 4 {
                    format!("{}.{}.***.***", parts[0], parts[1])
                } else {
                    "***.***.***.***".to_string()
                }
            })
            .to_string();
        out = self
            .id_card
            .replace_all(&out, |caps: &regex::Captures| {
                stats.bump("id_card");
                let whole = &caps[0];
                if whole.len() >= 8 {
                    format!("{}****{}", &whole[..4], &whole[whole.len() - 4..])
                } else {
                    "****".to_string()
                }
            })
            .to_string();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_email_phone_ipv4_in_nested_json() {
        let minimizer = PiiMinimizer::new();
        let mut value = json!({
            "user": {
                "email": "alice@example.com",
                "phone": "+8613800138000",
                "ip": "192.168.1.42",
                "note": "hello world"
            },
            "tags": ["plain", "bob@test.org"]
        });
        let stats = minimizer.minimize_value(&mut value);
        assert_eq!(value["user"]["email"], "u***@example.com");
        assert!(value["user"]["phone"].as_str().unwrap().ends_with("***"));
        assert_eq!(value["user"]["ip"], "192.168.***.***");
        assert_eq!(value["user"]["note"], "hello world");
        assert_eq!(value["tags"][0], "plain");
        assert_eq!(value["tags"][1], "u***@test.org");
        assert_eq!(stats.fields_masked, 4);
        assert_eq!(stats.hits_by_kind["email"], 2);
    }

    #[test]
    fn masks_id_card_keeping_edges() {
        let minimizer = PiiMinimizer::new();
        let mut value = json!({"id": "110101199003074512"});
        let stats = minimizer.minimize_value(&mut value);
        let masked = value["id"].as_str().unwrap();
        assert!(masked.starts_with("1101"));
        assert!(masked.ends_with("4512"));
        assert_eq!(stats.hits_by_kind["id_card"], 1);
    }

    #[test]
    fn non_matching_strings_untouched() {
        let minimizer = PiiMinimizer::new();
        let mut value = json!({"text": "no personal data here"});
        let stats = minimizer.minimize_value(&mut value);
        assert_eq!(value["text"], "no personal data here");
        assert_eq!(stats.fields_masked, 0);
    }
}
