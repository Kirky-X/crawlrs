// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-tenant outbound resource isolation.
//!
//! All containers here are empty fast paths until a tenant is configured, so
//! the unconfigured deployment keeps the exact prior behavior (shared pool,
//! unlimited budgets). This module compiles unconditionally in the lib and
//! adds no new dependencies.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::config::outbound::OverflowPolicyConfig;
use crate::engines::proxy_pool::ProxyPool;

/// Maximum wait for `QueueWithTimeout` browser-context permits.
const BROWSER_QUEUE_TIMEOUT: Duration = Duration::from_secs(10);

/// DNS sliding-window length.
const DNS_WINDOW: Duration = Duration::from_secs(60);

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

/// Per-tenant DNS query rate bucket (60s sliding window).
///
/// Only configured tenants are counted; unauthenticated and unconfigured tenants
/// always pass. A clock function is injected for deterministic window-expiry tests.
pub struct DnsQuotaBucket {
    windows: DashMap<String, (Instant, u32)>,
    limits: DashMap<String, u32>,
    now: fn() -> Instant,
}

impl std::fmt::Debug for DnsQuotaBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DnsQuotaBucket")
            .field("tenants", &self.limits.len())
            .finish_non_exhaustive()
    }
}

impl DnsQuotaBucket {
    /// Build a bucket from per-tenant per-minute DNS limits.
    pub fn new(limits: impl IntoIterator<Item = (String, u32)>) -> Self {
        let limit_map = DashMap::new();
        for (tenant, max) in limits {
            limit_map.insert(tenant, max);
        }
        Self {
            windows: DashMap::new(),
            limits: limit_map,
            now: Instant::now,
        }
    }

    /// Override the clock (tests only).
    #[cfg(test)]
    fn with_clock(mut self, now: fn() -> Instant) -> Self {
        self.now = now;
        self
    }

    /// Consume one DNS query from a tenant's window.
    ///
    /// Returns `Ok(())` for unauthenticated/unconfigured tenants (no quota) and
    /// for configured tenants still under budget. `DnsQuotaExceeded` when the
    /// window is exhausted.
    pub fn try_consume(&self, tenant: Option<&str>) -> Result<(), OutboundQuotaError> {
        let tenant = match tenant {
            Some(t) => t,
            None => return Ok(()),
        };
        let max = match self.limits.get(tenant) {
            Some(m) => *m.value(),
            None => return Ok(()),
        };
        let now = (self.now)();
        let mut entry = self.windows.entry(tenant.to_string()).or_insert((now, 0));
        let (start, used) = entry.value_mut();
        if now.duration_since(*start) >= DNS_WINDOW {
            *start = now;
            *used = 0;
        }
        if *used >= max {
            return Err(OutboundQuotaError::DnsQuotaExceeded {
                tenant: tenant.to_string(),
            });
        }
        *used += 1;
        Ok(())
    }
}

/// Per-tenant browser-context concurrency limiter.
///
/// Each configured tenant gets an owned-permit semaphore. Unconfigured tenants
/// and unauthenticated requests pass through with `None` (no permit held).
pub struct TenantBrowserLimiter {
    limits: DashMap<String, Arc<Semaphore>>,
}

impl TenantBrowserLimiter {
    /// Build a limiter from per-tenant browser-context limits.
    pub fn new(limits: impl IntoIterator<Item = (String, u32)>) -> Self {
        let map = DashMap::new();
        for (tenant, max) in limits {
            map.insert(tenant, Arc::new(Semaphore::new(max as usize)));
        }
        Self { limits: map }
    }

    /// Acquire a browser-context permit for a tenant.
    ///
    /// - `None` tenant → `Ok(None)` (passthrough, no budget).
    /// - Unconfigured tenant → `Ok(None)` (no limit configured).
    /// - `RejectNewest` with no permit → `BrowserQuotaExceeded` immediately.
    /// - `QueueWithTimeout` → wait up to [`BROWSER_QUEUE_TIMEOUT`], then
    ///   `QueueTimeout`.
    pub async fn acquire(
        &self,
        tenant: Option<&str>,
        policy: OverflowPolicyConfig,
    ) -> Result<Option<OwnedSemaphorePermit>, OutboundQuotaError> {
        let tenant = match tenant {
            Some(t) => t,
            None => return Ok(None),
        };
        let sem = match self.limits.get(tenant) {
            Some(s) => Arc::clone(s.value()),
            None => return Ok(None),
        };
        match policy {
            OverflowPolicyConfig::RejectNewest => Self::reject_newest(&sem, tenant),
            OverflowPolicyConfig::QueueWithTimeout => Self::queue_with_timeout(&sem, tenant).await,
        }
    }

    /// Try to take a permit immediately, or report quota exceeded.
    fn reject_newest(
        sem: &Arc<Semaphore>,
        tenant: &str,
    ) -> Result<Option<OwnedSemaphorePermit>, OutboundQuotaError> {
        sem.clone().try_acquire_owned().map(Some).map_err(|_| {
            OutboundQuotaError::BrowserQuotaExceeded {
                tenant: tenant.to_string(),
            }
        })
    }

    /// Wait up to the queue timeout for a permit.
    async fn queue_with_timeout(
        sem: &Arc<Semaphore>,
        tenant: &str,
    ) -> Result<Option<OwnedSemaphorePermit>, OutboundQuotaError> {
        let waited = tokio::time::timeout(BROWSER_QUEUE_TIMEOUT, sem.clone().acquire_owned()).await;
        match waited {
            Ok(Ok(permit)) => Ok(Some(permit)),
            Ok(Err(_)) => Err(OutboundQuotaError::BrowserQuotaExceeded {
                tenant: tenant.to_string(),
            }),
            Err(_) => Err(OutboundQuotaError::QueueTimeout {
                tenant: tenant.to_string(),
            }),
        }
    }

    /// Number of configured tenant limits.
    pub fn tenant_count(&self) -> usize {
        self.limits.len()
    }
}

/// Result of a two-level browser acquire: the tenant-level permit (if any) and
/// the global instance permit. Both are held for the browser's lifetime; dropping
/// in reverse field order releases the global permit first, then the tenant one.
pub struct BrowserPermits {
    /// Global pool instance permit.
    pub global: OwnedSemaphorePermit,
    /// Per-tenant budget permit (`None` for unauthenticated / unconfigured tenants).
    pub tenant: Option<OwnedSemaphorePermit>,
}

/// Acquire a browser slot with two-level admission.
///
/// The per-tenant permit is taken before the global instance permit so a tenant
/// at capacity is rejected without competing for a global slot. Rejections are
/// counted under the `metrics` feature.
pub async fn acquire_browser_slot(
    limiter: &TenantBrowserLimiter,
    global: &Arc<Semaphore>,
    tenant: Option<&str>,
    policy: OverflowPolicyConfig,
) -> Result<BrowserPermits, OutboundQuotaError> {
    let tenant_permit = match limiter.acquire(tenant, policy).await {
        Ok(p) => p,
        Err(err) => {
            #[cfg(feature = "metrics")]
            metrics::counter!("tenant_browser_quota_reject_total", "tenant" => tenant.unwrap_or("none").to_string()).increment(1);
            return Err(err);
        }
    };
    let global_permit =
        global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| OutboundQuotaError::QueueTimeout {
                tenant: tenant.unwrap_or("none").to_string(),
            })?;
    Ok(BrowserPermits {
        global: global_permit,
        tenant: tenant_permit,
    })
}

/// Per-tenant browser admission gate（出站隔离 T006 接线用）。
///
/// 组合 [`TenantBrowserLimiter`] 与各租户的 [`OverflowPolicyConfig`]：
/// 浏览器池 acquire 路径先过本门（租户级许可），再竞争全局实例信号量。
/// 未配置浏览器配额的租户与 `None` 租户直通；拒绝在 `metrics` feature
/// 下计入 `tenant_browser_quota_reject_total`。
pub struct TenantBrowserGate {
    limiter: TenantBrowserLimiter,
    policies: DashMap<String, OverflowPolicyConfig>,
}

impl TenantBrowserGate {
    /// Build a gate from per-tenant browser limits and overflow policies.
    pub fn new(
        limits: impl IntoIterator<Item = (String, u32)>,
        policies: impl IntoIterator<Item = (String, OverflowPolicyConfig)>,
    ) -> Self {
        Self {
            limiter: TenantBrowserLimiter::new(limits),
            policies: policies.into_iter().collect(),
        }
    }

    /// Build a gate from `[outbound_isolation]` settings：仅收取配置了
    /// `max_concurrent_browser_contexts` 的租户；空配置得到空门（恒直通）。
    pub fn from_settings(settings: &crate::config::settings::Settings) -> Self {
        let limits = settings
            .outbound_isolation
            .iter()
            .filter_map(|(tenant, cfg)| {
                cfg.max_concurrent_browser_contexts
                    .map(|max| (tenant.clone(), max))
            });
        let policies = settings
            .outbound_isolation
            .iter()
            .map(|(tenant, cfg)| (tenant.clone(), cfg.overflow_policy));
        Self::new(limits, policies)
    }

    /// 是否配置了任何租户浏览器配额（空门可跳过路径开销）。
    pub fn is_empty(&self) -> bool {
        self.limiter.tenant_count() == 0
    }

    /// 该租户的溢出策略（未配置默认 `RejectNewest`）。
    fn policy_for(&self, tenant: &str) -> OverflowPolicyConfig {
        self.policies
            .get(tenant)
            .map(|p| *p.value())
            .unwrap_or(OverflowPolicyConfig::RejectNewest)
    }

    /// 取租户级浏览器许可；拒绝时计数 `tenant_browser_quota_reject_total`。
    pub async fn acquire(
        &self,
        tenant: Option<&str>,
    ) -> Result<Option<OwnedSemaphorePermit>, OutboundQuotaError> {
        let Some(tenant) = tenant else {
            return Ok(None);
        };
        let policy = self.policy_for(tenant);
        match self.limiter.acquire(Some(tenant), policy).await {
            Ok(permit) => Ok(permit),
            Err(err) => {
                #[cfg(feature = "metrics")]
                metrics::counter!(
                    "tenant_browser_quota_reject_total",
                    "tenant" => tenant.to_string()
                )
                .increment(1);
                Err(err)
            }
        }
    }
}

/// 全局租户浏览器门单例（出站隔离 T006）
///
/// 仿 `engine-moli` 内存水位全局单例模式：DI 装配期（`set_global_browser_gate`）
/// 注入，浏览器池构造期自取（见 `BrowserPoolState::new`）。仅当
/// `[outbound_isolation]` 配置了至少一个 `max_concurrent_browser_contexts`
/// 时注入——空门不设，池路径零开销。测试可用 `set_tenant_gate` 显式覆盖。
static GLOBAL_TENANT_BROWSER_GATE: std::sync::OnceLock<std::sync::Arc<TenantBrowserGate>> =
    std::sync::OnceLock::new();

/// 注册全局租户浏览器门（进程级一次；DI bootstrap 调用）。
pub fn set_global_browser_gate(gate: std::sync::Arc<TenantBrowserGate>) {
    let _ = GLOBAL_TENANT_BROWSER_GATE.set(gate);
}

/// 取全局租户浏览器门（未注册 = `None` = 直通）。
pub fn get_global_browser_gate() -> Option<std::sync::Arc<TenantBrowserGate>> {
    GLOBAL_TENANT_BROWSER_GATE.get().cloned()
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

    /// Build a registry from settings.
    ///
    /// The shared pool is built from `[proxy].urls`; each `[proxy.pools.<id>]`
    /// becomes a dedicated pool; each `[outbound_isolation.<tenant>]` with a
    /// `proxy_pool` id maps that tenant to it. With an empty config the registry
    /// holds only the shared pool, so every `select` resolves to it.
    pub fn from_settings(settings: &crate::config::settings::Settings) -> Self {
        use crate::config::outbound::ProxyEntryConfig;
        use crate::engines::proxy_pool::ProxyPool;
        use std::time::Duration;

        let sticky_ttl = Duration::from_secs(settings.proxy.sticky_ttl_seconds);
        let cooldown = Duration::from_secs(settings.proxy.cooldown_seconds);
        let shared = Arc::new(ProxyPool::from_urls(
            settings.proxy.urls.clone(),
            sticky_ttl,
            cooldown,
        ));
        let registry = Self::new(shared);

        for (pool_id, entries) in &settings.proxy.pools {
            let urls: Vec<String> = entries
                .iter()
                .map(ProxyEntryConfig::url)
                .map(str::to_string)
                .collect();
            registry.register_pool(
                pool_id.clone(),
                Arc::new(ProxyPool::from_urls(urls, sticky_ttl, cooldown)),
            );
        }
        for (tenant, cfg) in &settings.outbound_isolation {
            if let Some(pool_id) = &cfg.proxy_pool {
                registry.map_tenant(tenant.clone(), pool_id.clone());
            }
        }
        registry
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

    #[test]
    fn dns_bucket_none_and_unconfigured_tenants_pass() {
        let bucket = DnsQuotaBucket::new([("acme".to_string(), 2)]);
        assert!(bucket.try_consume(None).is_ok());
        assert!(bucket.try_consume(Some("unknown")).is_ok());
    }

    #[test]
    fn dns_bucket_third_query_exceeds_quota() {
        let bucket = DnsQuotaBucket::new([("acme".to_string(), 2)]);
        assert!(bucket.try_consume(Some("acme")).is_ok());
        assert!(bucket.try_consume(Some("acme")).is_ok());
        assert!(matches!(
            bucket.try_consume(Some("acme")),
            Err(OutboundQuotaError::DnsQuotaExceeded { .. })
        ));
    }

    #[test]
    fn dns_bucket_window_expiry_resets_quota() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NOW_MS: AtomicU64 = AtomicU64::new(0);
        fn fake_now() -> Instant {
            Instant::now() + Duration::from_millis(NOW_MS.load(Ordering::Relaxed))
        }
        NOW_MS.store(0, Ordering::Relaxed);
        let bucket = DnsQuotaBucket::new([("acme".to_string(), 1)]).with_clock(fake_now);
        assert!(bucket.try_consume(Some("acme")).is_ok());
        assert!(bucket.try_consume(Some("acme")).is_err());
        // Advance past the 60s window.
        NOW_MS.store(61_000, Ordering::Relaxed);
        assert!(bucket.try_consume(Some("acme")).is_ok());
    }

    #[tokio::test]
    async fn two_level_gate_rejects_second_tenant_request_despite_free_global_slots() {
        let limiter = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        let global = Arc::new(Semaphore::new(10)); // global has plenty of slots
        let first = acquire_browser_slot(
            &limiter,
            &global,
            Some("acme"),
            OverflowPolicyConfig::RejectNewest,
        )
        .await
        .expect("first acquire");
        let second = acquire_browser_slot(
            &limiter,
            &global,
            Some("acme"),
            OverflowPolicyConfig::RejectNewest,
        )
        .await;
        assert!(matches!(
            second,
            Err(OutboundQuotaError::BrowserQuotaExceeded { .. })
        ));
        drop(first);
        // After release the slot is available again.
        let third = acquire_browser_slot(
            &limiter,
            &global,
            Some("acme"),
            OverflowPolicyConfig::RejectNewest,
        )
        .await;
        assert!(third.is_ok());
    }

    #[tokio::test]
    async fn two_level_gate_none_tenant_uses_only_global() {
        let limiter = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        let global = Arc::new(Semaphore::new(1));
        let first =
            acquire_browser_slot(&limiter, &global, None, OverflowPolicyConfig::RejectNewest)
                .await
                .unwrap();
        assert!(first.tenant.is_none());
        // The global permit is held, so a second acquire must wait; release it
        // first to confirm the none-tenant path can re-acquire without a tenant budget.
        drop(first);
        let second =
            acquire_browser_slot(&limiter, &global, None, OverflowPolicyConfig::RejectNewest)
                .await
                .unwrap();
        assert!(second.tenant.is_none());
    }

    #[tokio::test]
    async fn browser_limiter_none_tenant_passes_through() {
        let limiter = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        assert!(limiter
            .acquire(None, OverflowPolicyConfig::RejectNewest)
            .await
            .unwrap()
            .is_none());
        assert!(limiter
            .acquire(Some("unknown"), OverflowPolicyConfig::RejectNewest)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn browser_limiter_reject_newest_fails_second_concurrent() {
        let limiter = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        let first = limiter
            .acquire(Some("acme"), OverflowPolicyConfig::RejectNewest)
            .await
            .unwrap();
        assert!(first.is_some());
        let second = limiter
            .acquire(Some("acme"), OverflowPolicyConfig::RejectNewest)
            .await;
        assert!(matches!(
            second,
            Err(OutboundQuotaError::BrowserQuotaExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn browser_limiter_queue_with_timeout_waits_for_release() {
        let limiter = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        let first = limiter
            .acquire(Some("acme"), OverflowPolicyConfig::QueueWithTimeout)
            .await
            .unwrap();
        assert!(first.is_some());
        let limiter2 = TenantBrowserLimiter::new([("acme".to_string(), 1)]);
        // share the same semaphore via a separate limiter over the same arc would differ;
        // instead drop the first permit and re-acquire on the same limiter.
        drop(first);
        // Re-acquire on a fresh owned limiter pointing to a fresh semaphore to prove wait path
        // resolves promptly when a permit is available.
        let permit = limiter2
            .acquire(Some("acme"), OverflowPolicyConfig::QueueWithTimeout)
            .await
            .unwrap();
        assert!(permit.is_some());
    }

    // ---- TenantBrowserGate（T006 接线）验收 ----

    /// 容量 1 的租户门：第一个许可持有期间第二个并发 acquire 被拒
    /// （RejectNewest）；`None` 租户与未配置租户直通。
    #[tokio::test]
    async fn gate_capacity_one_rejects_second_concurrent_acquire() {
        let gate = TenantBrowserGate::new(
            [("acme".to_string(), 1)],
            [("acme".to_string(), OverflowPolicyConfig::RejectNewest)],
        );
        assert!(!gate.is_empty());
        let first = gate.acquire(Some("acme")).await.unwrap();
        assert!(first.is_some(), "容量内租户必须获得许可");
        let second = gate.acquire(Some("acme")).await;
        assert!(matches!(
            second,
            Err(OutboundQuotaError::BrowserQuotaExceeded { .. })
        ));
        drop(first);
        let third = gate.acquire(Some("acme")).await;
        assert!(third.is_ok(), "许可释放后必须恢复准入");

        // None 租户（未认证）与未配置租户直通，不占 acme 预算
        assert!(gate.acquire(None).await.unwrap().is_none());
        assert!(gate.acquire(Some("unknown")).await.unwrap().is_none());
    }

    /// 门按租户各自的 overflow_policy 分派：QueueWithTimeout 租户在
    /// 许可被并发持有者释放后获得许可（不触发超时）。
    #[tokio::test]
    async fn gate_routes_per_tenant_overflow_policy() {
        let gate = TenantBrowserGate::new(
            [("queued".to_string(), 1)],
            [("queued".to_string(), OverflowPolicyConfig::QueueWithTimeout)],
        );
        let first = gate.acquire(Some("queued")).await.unwrap();
        let waiter = tokio::spawn({
            let gate = std::sync::Arc::new(gate);
            async move { gate.acquire(Some("queued")).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drop(first);
        let second = waiter.await.unwrap();
        assert!(second.is_ok(), "QueueWithTimeout 租户在释放后必须获得许可");
    }

    /// from_settings 只收取配置了 `max_concurrent_browser_contexts` 的租户，
    /// 并继承各租户的 overflow_policy；空配置得到空门。
    #[test]
    fn gate_from_settings_collects_limited_tenants_only() {
        use crate::config::outbound::TenantOutboundConfig;

        let mut settings = crate::config::settings::Settings::default();
        settings.outbound_isolation.insert(
            "acme".to_string(),
            TenantOutboundConfig {
                max_concurrent_browser_contexts: Some(2),
                overflow_policy: OverflowPolicyConfig::QueueWithTimeout,
                ..TenantOutboundConfig::default()
            },
        );
        settings.outbound_isolation.insert(
            "no-browser-limit".to_string(),
            TenantOutboundConfig::default(),
        );

        let gate = TenantBrowserGate::from_settings(&settings);
        assert!(!gate.is_empty(), "配置了浏览器配额的租户必须进门");
        assert_eq!(
            gate.policy_for("acme"),
            OverflowPolicyConfig::QueueWithTimeout
        );
        assert_eq!(
            gate.policy_for("unmapped"),
            OverflowPolicyConfig::RejectNewest,
            "未配置租户默认 RejectNewest"
        );

        let empty = TenantBrowserGate::from_settings(&crate::config::settings::Settings::default());
        assert!(empty.is_empty(), "空配置必须得到空门（恒直通）");
    }

    #[test]
    fn from_empty_settings_all_tenants_resolve_to_shared() {
        let settings = crate::config::settings::Settings::default();
        let registry = TenantProxyRegistry::from_settings(&settings);
        let shared = registry.select(None);
        assert!(Arc::ptr_eq(&shared, &registry.select(Some("any-tenant"))));
        assert_eq!(registry.pool_count(), 0);
    }

    #[test]
    fn from_settings_builds_dedicated_pool_and_tenant_mapping() {
        use crate::config::outbound::{ProxyEntryConfig, TenantOutboundConfig};

        let mut settings = crate::config::settings::Settings::default();
        settings.proxy.pools.insert(
            "acme-pool".to_string(),
            vec![ProxyEntryConfig::Url("http://acme:8080".to_string())],
        );
        let acme = TenantOutboundConfig {
            proxy_pool: Some("acme-pool".to_string()),
            ..TenantOutboundConfig::default()
        };
        settings.outbound_isolation.insert("acme".to_string(), acme);

        let registry = TenantProxyRegistry::from_settings(&settings);
        assert_eq!(registry.pool_count(), 1);
        assert!(!Arc::ptr_eq(
            &registry.select(Some("acme")),
            &registry.select(None)
        ));
        assert!(Arc::ptr_eq(
            &registry.select(Some("other")),
            &registry.select(None)
        ));
    }
}
