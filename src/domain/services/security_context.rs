// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Response `security_context` contract.
//!
//! A "security passport" attached to scrape responses. Every field is optional
//! so the whole object is omitted when empty, keeping the 0.2.0 response schema
//! backward-compatible. The type lives in the domain layer so agent-lib builds
//! can re-export it without compiling the application/presentation layers.

use serde::{Deserialize, Serialize};

/// Summary of injection-sanitization counters (sourced from the
/// llm-injection-sanitizer change's `SanitizationReport`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SanitizationSummary {
    /// Hidden DOM subtrees removed.
    pub hidden_nodes_removed: u32,
    /// Suspicious instruction lines flagged.
    pub suspicious_hits: u32,
}

/// Security/compliance context attached to a scrape response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SecurityContext {
    /// Authenticated tenant identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    /// Engine chain actually used (initial → escalation targets).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub engine_path: Vec<String>,
    /// robots.txt verdict (`allowed` / `disallowed` / `not_fetched`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots_txt: Option<String>,
    /// ai.txt verdict (when `respect_ai_txt` is enabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_txt: Option<String>,
    /// Injection-sanitization summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sanitization: Option<SanitizationSummary>,
    /// Extraction quality confidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_confidence: Option<f64>,
    /// Active compliance profile (e.g. `gdpr`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_profile: Option<String>,
    /// Whether PII minimization was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pii_minimized: Option<bool>,
}

impl SecurityContext {
    /// True when no field carries a value.
    pub fn is_empty(&self) -> bool {
        self.tenant.is_none()
            && self.engine_path.is_empty()
            && self.robots_txt.is_none()
            && self.ai_txt.is_none()
            && self.sanitization.is_none()
            && self.extraction_confidence.is_none()
            && self.compliance_profile.is_none()
            && self.pii_minimized.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_serializes_to_empty_object() {
        let ctx = SecurityContext::default();
        assert!(ctx.is_empty());
        assert_eq!(serde_json::to_string(&ctx).unwrap(), "{}");
    }

    #[test]
    fn full_context_serializes_all_eight_keys_snake_case() {
        let ctx = SecurityContext {
            tenant: Some("acme".to_string()),
            engine_path: vec!["http".to_string(), "playwright".to_string()],
            robots_txt: Some("allowed".to_string()),
            ai_txt: Some("not_fetched".to_string()),
            sanitization: Some(SanitizationSummary {
                hidden_nodes_removed: 3,
                suspicious_hits: 1,
            }),
            extraction_confidence: Some(0.9),
            compliance_profile: Some("gdpr".to_string()),
            pii_minimized: Some(true),
        };
        assert!(!ctx.is_empty());
        let json = serde_json::to_value(&ctx).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 8);
        for key in [
            "tenant",
            "engine_path",
            "robots_txt",
            "ai_txt",
            "sanitization",
            "extraction_confidence",
            "compliance_profile",
            "pii_minimized",
        ] {
            assert!(obj.contains_key(key), "missing snake_case key: {key}");
        }
    }
}
