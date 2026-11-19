// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-tenant compliance configuration.
//!
//! Empty map keeps all compliance behavior disabled (robots-only admission,
//! no PII minimization, no retention checks). GDPR implies minimization even
//! when the explicit flag is off.

use serde::{Deserialize, Serialize};

/// Compliance settings for one tenant.
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__COMPLIANCE__")]
pub struct TenantComplianceConfig {
    /// Apply the GDPR compliance profile.
    #[serde(default)]
    pub gdpr: bool,
    /// Respect `ai.txt` during admission.
    #[serde(default)]
    pub respect_ai_txt: bool,
    /// Minimize detected personal data in extracted content.
    #[serde(default)]
    pub minimize_personal_data: bool,
    /// Data retention window in days.
    #[serde(default)]
    pub retention_days: Option<u32>,
    /// Reference to a DPIA document.
    #[serde(default)]
    pub dpia_reference: Option<String>,
}

/// Whether personal-data minimization is enforced for a tenant.
///
/// GDPR forces minimization regardless of the explicit flag.
pub fn enforced_minimize(cfg: &TenantComplianceConfig) -> bool {
    cfg.gdpr || cfg.minimize_personal_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdpr_forces_minimize() {
        let cfg = TenantComplianceConfig {
            gdpr: true,
            minimize_personal_data: false,
            ..Default::default()
        };
        assert!(enforced_minimize(&cfg));
    }

    #[test]
    fn minimize_flag_without_gdpr() {
        let cfg = TenantComplianceConfig {
            minimize_personal_data: true,
            ..Default::default()
        };
        assert!(enforced_minimize(&cfg));
    }

    #[test]
    fn no_flags_means_no_minimize() {
        assert!(!enforced_minimize(&TenantComplianceConfig::default()));
    }

    #[test]
    fn toml_defaults_all_optional_fields() {
        let cfg: TenantComplianceConfig = toml::from_str("respect_ai_txt = true").unwrap();
        assert!(cfg.respect_ai_txt);
        assert!(!cfg.gdpr);
        assert!(cfg.retention_days.is_none());
        assert!(cfg.dpia_reference.is_none());
    }
}
