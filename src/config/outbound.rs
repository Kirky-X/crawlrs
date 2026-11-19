// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-tenant outbound isolation configuration.
//!
//! Each tenant may declare a dedicated proxy pool and concurrency budgets for
//! browser contexts and DNS queries. Absent configuration means the shared
//! global pool and unlimited budgets (byte-for-byte current behavior).

use serde::{Deserialize, Serialize};

/// Overflow handling when a tenant-specific resource budget is exhausted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicyConfig {
    /// Reject the incoming request immediately.
    #[default]
    RejectNewest,
    /// Wait briefly, then degrade to the shared pool.
    QueueWithTimeout,
}

/// Outbound isolation settings for one tenant.
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__OUTBOUND_ISOLATION__")]
pub struct TenantOutboundConfig {
    /// Identifier of a dedicated proxy pool; `None` uses the shared pool.
    #[serde(default)]
    pub proxy_pool: Option<String>,

    /// Max concurrent browser contexts for this tenant.
    #[serde(default)]
    pub max_concurrent_browser_contexts: Option<u32>,

    /// Max DNS queries per minute for this tenant.
    #[serde(default)]
    pub max_dns_queries_per_minute: Option<u32>,

    /// Overflow policy; defaults to `RejectNewest` when the key is absent.
    #[serde(default)]
    pub overflow_policy: OverflowPolicyConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_policy_defaults_to_reject_newest() {
        let cfg: TenantOutboundConfig = toml::from_str("").expect("empty toml");
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::RejectNewest);
    }

    #[test]
    fn overflow_policy_deserializes_snake_case() {
        let cfg: TenantOutboundConfig =
            toml::from_str("overflow_policy = \"queue_with_timeout\"").expect("toml");
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::QueueWithTimeout);
    }

    #[test]
    fn tenant_config_parses_browser_context_limit() {
        let cfg: TenantOutboundConfig =
            toml::from_str("max_concurrent_browser_contexts = 8").expect("toml");
        assert_eq!(cfg.max_concurrent_browser_contexts, Some(8));
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::RejectNewest);
    }

    #[test]
    fn tenant_config_all_fields_optional() {
        let cfg: TenantOutboundConfig = toml::from_str("").expect("toml");
        assert!(cfg.proxy_pool.is_none());
        assert!(cfg.max_concurrent_browser_contexts.is_none());
        assert!(cfg.max_dns_queries_per_minute.is_none());
    }
}
