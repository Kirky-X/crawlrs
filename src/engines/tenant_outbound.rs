// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-tenant outbound resource isolation.
//!
//! All containers here are empty fast paths until a tenant is configured, so
//! the unconfigured deployment keeps the exact prior behavior (shared pool,
//! unlimited budgets). This module compiles unconditionally in the lib and
//! adds no new dependencies.

use std::sync::Arc;

use dashmap::DashMap;
use thiserror::Error;

use crate::engines::proxy_pool::ProxyPool;

/// Quota overflow for a tenant's outbound resources.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OutboundQuotaError {
    /// Browser-context concurrency budget exhausted.
    #[error("tenant {tenant} browser context quota exceeded")]
    BrowserQuotaExceeded {
        /// Tenant that exhausted its budget.
        tenant: String,
    },
    /// DNS query rate budget exhausted.
    #[error("tenant {tenant} dns quota exceeded")]
    DnsQuotaExceeded {
        /// Tenant that exhausted its budget.
        tenant: String,
    },
    /// Queue-with-timeout wait elapsed.
    #[error("queue timeout for tenant {tenant}")]
    QueueTimeout {
        /// Tenant whose wait elapsed.
        tenant: String,
    },
}

/// Routes each tenant to a dedicated or shared proxy pool.
pub struct TenantProxyRegistry {
    shared: Arc<ProxyPool>,
    dedicated: DashMap<String, Arc<ProxyPool>>,
    tenant_pool: DashMap<String, String>,
}

impl TenantProxyRegistry {
    /// Build a registry around the shared pool.
    pub fn new(shared: Arc<ProxyPool>) -> Self {
        Self {
            shared,
            dedicated: DashMap::new(),
            tenant_pool: DashMap::new(),
        }
    }

    /// Register a named dedicated pool.
    pub fn register_pool(&self, pool_id: impl Into<String>, pool: Arc<ProxyPool>) {
        self.dedicated.insert(pool_id.into(), pool);
    }

    /// Map a tenant to a named pool. Unknown pool ids fall back to shared at
    /// selection time.
    pub fn map_tenant(&self, tenant: impl Into<String>, pool_id: impl Into<String>) {
        self.tenant_pool.insert(tenant.into(), pool_id.into());
    }

    /// Select the proxy pool for a tenant. `None` (unauthenticated / embedded)
    /// and unmapped tenants resolve to the shared pool.
    pub fn select(&self, tenant: Option<&str>) -> Arc<ProxyPool> {
        let tenant = match tenant {
            Some(t) => t,
            None => return Arc::clone(&self.shared),
        };
        let pool_id = match self.tenant_pool.get(tenant) {
            Some(id) => id.clone(),
            None => return Arc::clone(&self.shared),
        };
        match self.dedicated.get(&pool_id) {
            Some(pool) => Arc::clone(pool.value()),
            None => Arc::clone(&self.shared),
        }
    }

    /// Number of configured dedicated pools.
    pub fn pool_count(&self) -> usize {
        self.dedicated.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pool(url: &str) -> Arc<ProxyPool> {
        Arc::new(ProxyPool::from_urls(
            vec![url.to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ))
    }

    #[test]
    fn select_none_returns_shared_pool() {
        let shared = pool("http://shared:8080");
        let registry = TenantProxyRegistry::new(Arc::clone(&shared));
        assert!(Arc::ptr_eq(&registry.select(None), &shared));
    }

    #[test]
    fn select_unmapped_tenant_returns_shared_pool() {
        let shared = pool("http://shared:8080");
        let registry = TenantProxyRegistry::new(Arc::clone(&shared));
        assert!(Arc::ptr_eq(&registry.select(Some("unknown")), &shared));
    }

    #[test]
    fn select_configured_tenant_returns_dedicated_pool() {
        let shared = pool("http://shared:8080");
        let dedicated = pool("http://acme:8080");
        let registry = TenantProxyRegistry::new(Arc::clone(&shared));
        registry.register_pool("acme-pool", Arc::clone(&dedicated));
        registry.map_tenant("acme", "acme-pool");
        assert!(Arc::ptr_eq(&registry.select(Some("acme")), &dedicated));
        assert_eq!(registry.pool_count(), 1);
    }

    #[test]
    fn two_tenants_get_distinct_dedicated_pools() {
        let shared = pool("http://shared:8080");
        let a = pool("http://a:8080");
        let b = pool("http://b:8080");
        let registry = TenantProxyRegistry::new(Arc::clone(&shared));
        registry.register_pool("a-pool", Arc::clone(&a));
        registry.register_pool("b-pool", Arc::clone(&b));
        registry.map_tenant("tenant-a", "a-pool");
        registry.map_tenant("tenant-b", "b-pool");
        assert!(Arc::ptr_eq(&registry.select(Some("tenant-a")), &a));
        assert!(Arc::ptr_eq(&registry.select(Some("tenant-b")), &b));
    }

    #[test]
    fn unknown_pool_id_falls_back_to_shared() {
        let shared = pool("http://shared:8080");
        let registry = TenantProxyRegistry::new(Arc::clone(&shared));
        registry.map_tenant("acme", "does-not-exist");
        assert!(Arc::ptr_eq(&registry.select(Some("acme")), &shared));
    }

    #[test]
    fn quota_error_messages_include_tenant() {
        let err = OutboundQuotaError::BrowserQuotaExceeded {
            tenant: "acme".to_string(),
        };
        assert!(format!("{err}").contains("acme"));
    }
}
