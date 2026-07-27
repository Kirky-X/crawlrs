// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理轮换池（design.md §12，T054/R-identity-003）
//!
//! 提供代理轮换池：RoundRobin + 粘性会话 + 健康检查冷却。
//! 支持 `ProxyCategory` 路由（HTML/Media 走不同子池）。
//!
//! ## 关键设计
//!
//! - **RR 跳过冷却**：`next(category)` 遍历 entries，跳过 `cooldown_until > now` 的代理
//! - **粘性会话**：`sticky(session_id)` TTL 内返回同一代理；TTL 过期或代理冷却中时重选
//! - **失败冷却**：`mark_failure(url)` 设置 `cooldown_until = now + default_cooldown`
//! - **空池行为**：所有代理冷却中或池为空 → 返回 `None`（不阻塞不 panic）
//! - **脱敏**：日志输出代理 URL 时调用 `redact_proxy_url` 屏蔽 userinfo
//!
//! ## 线程安全
//!
//! `ProxyPool` 内部所有状态用 `Atomic*` / `DashMap` 共享，`&self` 即可并发调用
//! `next` / `sticky` / `mark_failure` / `mark_success`。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;

use crate::engines::provider::{ProxyCategory, ProxyProvider};
use crate::utils::proxy::redact_proxy_url;

/// 粘性会话绑定表最大容量（T056 安全审查 LOW-1 修复）
///
/// 防止恶意或异常数量的 session_id 耗尽内存。session_id 本身已有长度上限
/// （[`crate::engines::engine_client::MAX_SESSION_ID_LEN`] = 128 字节），
/// 但攻击者可发送大量不同 session_id 填充 sticky 表。
///
/// 超过容量时优先清理过期绑定；仍超容量时拒绝新绑定（返回代理 URL 但不绑定，
/// 调用方仍获得可用代理，仅失去粘性语义——降级而非拒绝服务）。
const MAX_STICKY_BINDINGS: usize = 10_000;

// 注：ProxyStrategy 定义在 `crate::config::settings`（配置项类型，由 ProxySettings 持有）。
// ProxyPool 内部不存储 strategy（由调用方按 strategy 选择 next / sticky 方法）。

/// 代理条目
///
/// 注意：不派生 `#[derive(Debug)]`，手动实现 Debug 对 `url` 脱敏，
/// 防止 `format!("{:?}", entry)` 泄露 `user:pass@host` 凭据（CWE-532 防护，
/// T056 安全审查 MEDIUM-1 修复）。
pub struct ProxyEntry {
    /// 代理 URL（含 userinfo，仅内部使用；日志输出必须脱敏）
    pub url: String,
    /// 健康状态（`true` = 可用，`false` = 冷却中）
    healthy: AtomicBool,
    /// 冷却截止时间戳（epoch_ms）；`0` 表示从未冷却
    cooldown_until: AtomicU64,
    /// 代理类别（HTML/Media）
    pub category: ProxyCategory,
}

impl std::fmt::Debug for ProxyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 安全：url 可能含 user:pass@host 凭据，Debug 输出必须脱敏（CWE-532 防护）
        f.debug_struct("ProxyEntry")
            .field("url", &redact_proxy_url(&self.url))
            .field("healthy", &self.healthy.load(Ordering::Acquire))
            .field("cooldown_until", &self.cooldown_until.load(Ordering::Acquire))
            .field("category", &self.category)
            .finish()
    }
}

impl ProxyEntry {
    /// 创建新条目，默认健康
    #[must_use]
    pub fn new(url: String, category: ProxyCategory) -> Self {
        Self {
            url,
            healthy: AtomicBool::new(true),
            cooldown_until: AtomicU64::new(0),
            category,
        }
    }

    /// 是否可用（健康 + 未在冷却中）
    pub fn is_available(&self) -> bool {
        let until = self.cooldown_until.load(Ordering::Acquire);
        if until == 0 {
            return self.healthy.load(Ordering::Acquire);
        }
        // 冷却已过 → 可用
        now_ms() >= until
    }

    /// 标记失败，进入冷却
    pub fn mark_failure(&self, cooldown: Duration) {
        let until = now_ms().saturating_add(cooldown.as_millis() as u64);
        self.cooldown_until.store(until, Ordering::Release);
        self.healthy.store(false, Ordering::Release);
    }

    /// 标记成功，恢复健康
    pub fn mark_success(&self) {
        self.cooldown_until.store(0, Ordering::Release);
        self.healthy.store(true, Ordering::Release);
    }
}

/// 粘性会话绑定记录
struct StickyBinding {
    /// 绑定的代理在 `entries` 中的索引
    entry_idx: usize,
    /// 绑定过期时间
    expires_at: Instant,
}

/// 代理轮换池
///
/// 线程安全：所有内部状态用原子操作 + `DashMap`，`&self` 可并发调用所有方法。
pub struct ProxyPool {
    /// 代理条目列表（按插入顺序保留，category 可混合）
    entries: Vec<Arc<ProxyEntry>>,
    /// 全局 RoundRobin 计数器（仅用于 `next` / `sticky` 重选时推进）
    rr: AtomicUsize,
    /// 粘性会话绑定表：`session_id -> (entry_idx, expires_at)`
    sticky: DashMap<String, StickyBinding>,
    /// 粘性会话 TTL
    sticky_ttl: Duration,
    /// 失败默认冷却时长
    default_cooldown: Duration,
    /// 粘性会话绑定表最大容量（T056 安全审查 LOW-1 修复）
    ///
    /// 超限时清理过期绑定；仍超限则拒绝新绑定（降级：返回代理 URL 但不绑定）。
    /// 默认 [`MAX_STICKY_BINDINGS`]，可通过 [`Self::with_sticky_max_capacity`] 调整。
    sticky_max_capacity: usize,
}

impl ProxyPool {
    /// 创建代理池
    ///
    /// # 参数
    ///
    /// - `entries`: 代理条目列表（可包含 Html/Media 混合）
    /// - `sticky_ttl`: 粘性会话 TTL（`sticky` 命中后在此时间内返回同一代理）
    /// - `default_cooldown`: `mark_failure` 默认冷却时长
    ///
    /// sticky 表容量上限默认为 [`MAX_STICKY_BINDINGS`]，可通过
    /// [`with_sticky_max_capacity`](Self::with_sticky_max_capacity) 调整。
    ///
    /// 性能审查 MEDIUM-2 修复：sticky 表预分配容量，避免高并发下大量 session_id
    /// 触发 DashMap 反复 rehash（默认 [`MAX_STICKY_BINDINGS`] = 10_000）。
    #[must_use]
    pub fn new(entries: Vec<ProxyEntry>, sticky_ttl: Duration, default_cooldown: Duration) -> Self {
        let entries: Vec<Arc<ProxyEntry>> = entries.into_iter().map(Arc::new).collect();
        let sticky_max_capacity = MAX_STICKY_BINDINGS;
        Self {
            entries,
            rr: AtomicUsize::new(0),
            // 预分配 sticky 表容量，避免运行时 rehash
            sticky: DashMap::with_capacity(sticky_max_capacity),
            sticky_ttl,
            default_cooldown,
            sticky_max_capacity,
        }
    }

    /// 设置 sticky 表最大容量（T056 安全审查 LOW-1 修复）
    ///
    /// 用于需要调整默认 [`MAX_STICKY_BINDINGS`] 上限的场景。
    /// 返回 `self` 以支持链式调用。
    #[must_use]
    pub fn with_sticky_max_capacity(mut self, capacity: usize) -> Self {
        self.sticky_max_capacity = capacity.max(1);
        self
    }

    /// 从 URL 字符串列表构造代理池（默认全部 `Html` category）
    ///
    /// 用于 `ProxySettings.urls` 直接构造池的便捷路径。
    #[must_use]
    pub fn from_urls(urls: Vec<String>, sticky_ttl: Duration, default_cooldown: Duration) -> Self {
        let entries: Vec<ProxyEntry> = urls
            .into_iter()
            .map(|url| ProxyEntry::new(url, ProxyCategory::Html))
            .collect();
        Self::new(entries, sticky_ttl, default_cooldown)
    }

    /// RoundRobin 取下一个可用代理（跳过冷却中），按 `category` 过滤
    ///
    /// 全部冷却或 category 无匹配时返回 `None`（不阻塞不 panic）。
    pub fn next(&self, category: ProxyCategory) -> Option<String> {
        let idx = self.rr_pick(category)?;
        Some(self.entries[idx].url.clone())
    }

    /// 粘性会话：TTL 内返回同一代理；TTL 过期或代理冷却中时重选
    ///
    /// 默认从 `Html` 子池选（sticky 主要用于 HTML 反爬场景）。
    /// 空池或全部冷却时返回 `None`。
    ///
    /// # 并发安全（T056 安全审查 LOW-2 修复）
    ///
    /// 使用 DashMap `entry` API 原子地 check-and-update，避免 TOCTOU 竞态：
    /// 多线程同时发现绑定过期时，只有一个线程执行重选并更新绑定，
    /// 其余线程在 `entry` 锁内重新检查到新绑定后复用，保证同一 session_id
    /// 在同一时刻返回同一代理。
    ///
    /// # 容量限制（T056 安全审查 LOW-1 修复）
    ///
    /// sticky 表上限 [`MAX_STICKY_BINDINGS`]。超限时先清理过期绑定；
    /// 清理后仍超限则返回代理 URL 但不绑定（降级，不拒绝服务）。
    pub fn sticky(&self, session_id: &str) -> Option<String> {
        use dashmap::mapref::entry::Entry;

        // 1. 快速路径：只读检查现有绑定是否有效
        if let Some(binding) = self.sticky.get(session_id) {
            if binding.expires_at > Instant::now()
                && self.entries[binding.entry_idx].is_available()
            {
                return Some(self.entries[binding.entry_idx].url.clone());
            }
            drop(binding);
        }

        // 2. 慢路径：重选
        let idx = self.rr_pick(ProxyCategory::Html)?;

        // LOW-1 修复：容量限制 — 超限时清理过期绑定
        if self.sticky.len() >= self.sticky_max_capacity {
            self.evict_expired_sticky();
        }

        // 在 entry() 之前检查容量，避免在 entry 锁内调用 len() 导致死锁
        // （DashMap entry 持有 shard 写锁，len() 需读取所有 shard 的读锁 → 死锁）
        // 这是一个软限制：检查后到 entry() 之间可能有其他线程插入，偶尔超出几个条目可接受
        let at_capacity = self.sticky.len() >= self.sticky_max_capacity;

        // 3. LOW-2 修复：使用 entry API 原子 check-and-update
        //    在 entry 锁内重新检查，避免 TOCTOU 竞态：
        //    另一线程可能在我们调用 rr_pick 期间已更新绑定
        match self.sticky.entry(session_id.to_string()) {
            Entry::Occupied(mut occ) => {
                let binding = occ.get();
                if binding.expires_at > Instant::now()
                    && self.entries[binding.entry_idx].is_available()
                {
                    return Some(self.entries[binding.entry_idx].url.clone());
                }
                // 绑定仍过期/冷却中 → 用新选的 idx 更新
                occ.insert(StickyBinding {
                    entry_idx: idx,
                    expires_at: Instant::now() + self.sticky_ttl,
                });
                Some(self.entries[idx].url.clone())
            }
            Entry::Vacant(vac) => {
                // LOW-1 修复：超容量时不绑定（降级：返回代理 URL，仅失去粘性语义）
                if !at_capacity {
                    vac.insert(StickyBinding {
                        entry_idx: idx,
                        expires_at: Instant::now() + self.sticky_ttl,
                    });
                }
                Some(self.entries[idx].url.clone())
            }
        }
    }

    /// 标记失败，代理进入冷却
    ///
    /// `url` 不存在时记录 warn 日志（不 panic）。
    pub fn mark_failure(&self, url: &str) {
        for entry in &self.entries {
            if entry.url == url {
                entry.mark_failure(self.default_cooldown);
                log::warn!(
                    "proxy_pool: marked failure (cooldown={}s): {}",
                    self.default_cooldown.as_secs(),
                    redact_proxy_url(url)
                );
                return;
            }
        }
        log::warn!(
            "proxy_pool: mark_failure url not found in pool: {}",
            redact_proxy_url(url)
        );
    }

    /// 标记成功，恢复代理健康
    pub fn mark_success(&self, url: &str) {
        for entry in &self.entries {
            if entry.url == url {
                entry.mark_success();
                return;
            }
        }
        // url 不存在时不报错（与 mark_failure 一致：调用方传错 url 不应阻塞流程）
        log::debug!(
            "proxy_pool: mark_success url not found in pool: {}",
            redact_proxy_url(url)
        );
    }

    /// 池是否为空
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 池大小
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// RR 选下一个可用代理的 entry_idx（推进 rr 计数器）
    ///
    /// 过滤 `category` + 未冷却，按 `rr` 计数器取候选列表中下一个。
    ///
    /// 性能审查 MEDIUM-1 修复：避免每次调用都分配 `Vec<usize>`。
    /// 原实现用 `filter().map().collect()` 收集候选索引到 Vec，
    /// 在高频调用（每次请求都走 `next` / `sticky` 重选）下产生大量短命分配。
    /// 修复后用 `count()` + `nth()` 替代 `collect()`，零堆分配：
    /// - 第一遍 `count()` 仅计数（迭代器内部不分配）
    /// - 第二遍 `nth(target)` 直接跳到第 `target` 个候选
    /// entries 通常 < 100，两遍扫描的成本可接受。
    fn rr_pick(&self, category: ProxyCategory) -> Option<usize> {
        // 第一遍：统计候选数（category 匹配 + 未冷却）
        let total = self
            .entries
            .iter()
            .filter(|e| e.category == category && e.is_available())
            .count();
        if total == 0 {
            return None;
        }
        // RR：取计数器值 mod 候选数，然后推进计数器
        let rr_idx = self.rr.fetch_add(1, Ordering::AcqRel);
        let target = rr_idx % total;
        // 第二遍：找第 target 个候选（零分配，直接迭代到目标位置）
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.category == category && e.is_available())
            .nth(target)
            .map(|(i, _)| i)
    }

    /// 清理过期的粘性会话绑定（T056 安全审查 LOW-1 修复）
    ///
    /// 移除所有 `expires_at <= now` 的绑定。在 sticky 表达到
    /// [`MAX_STICKY_BINDINGS`] 上限时调用，防止内存耗尽。
    fn evict_expired_sticky(&self) {
        let now = Instant::now();
        self.sticky.retain(|_, binding| binding.expires_at > now);
    }
}

/// 当前时间戳（epoch 毫秒）
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `ProxyPool` 自动实现 `ProxyProvider`（MEDIUM-1 修复：impl block 从 provider.rs 移至此处）
///
/// 方法签名与 `ProxyPool` 的固有方法一一对应，零成本桥接。
/// 显式实现覆盖 trait 的默认空实现（`mark_failure` / `mark_success`）。
impl ProxyProvider for ProxyPool {
    #[inline]
    fn next(&self, category: ProxyCategory) -> Option<String> {
        ProxyPool::next(self, category)
    }

    #[inline]
    fn sticky(&self, session_id: &str) -> Option<String> {
        ProxyPool::sticky(self, session_id)
    }

    #[inline]
    fn mark_failure(&self, url: &str) {
        ProxyPool::mark_failure(self, url);
    }

    #[inline]
    fn mark_success(&self, url: &str) {
        ProxyPool::mark_success(self, url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// 构造测试用代理池：所有 URL 默认 Html category
    fn make_pool(urls: Vec<&str>) -> ProxyPool {
        let entries: Vec<ProxyEntry> = urls
            .into_iter()
            .map(|u| ProxyEntry::new(u.to_string(), ProxyCategory::Html))
            .collect();
        ProxyPool::new(entries, Duration::from_secs(60), Duration::from_secs(30))
    }

    /// 构造测试用代理池（自定义 sticky_ttl）
    fn make_pool_with_ttl(urls: Vec<&str>, sticky_ttl: Duration) -> ProxyPool {
        let entries: Vec<ProxyEntry> = urls
            .into_iter()
            .map(|u| ProxyEntry::new(u.to_string(), ProxyCategory::Html))
            .collect();
        ProxyPool::new(entries, sticky_ttl, Duration::from_secs(30))
    }

    // =========================================================================
    // next() RoundRobin
    // =========================================================================

    #[test]
    fn next_round_robin_cycles_through_all_proxies() {
        let pool = make_pool(vec!["http://a:8080", "http://b:8080", "http://c:8080"]);
        let first = pool.next(ProxyCategory::Html).unwrap();
        let second = pool.next(ProxyCategory::Html).unwrap();
        let third = pool.next(ProxyCategory::Html).unwrap();
        let fourth = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(first, "http://a:8080");
        assert_eq!(second, "http://b:8080");
        assert_eq!(third, "http://c:8080");
        // RR 循环：第 4 次回到 a
        assert_eq!(fourth, "http://a:8080");
    }

    #[test]
    fn next_skips_proxies_in_cooldown() {
        let pool = make_pool(vec!["http://a:8080", "http://b:8080"]);
        pool.mark_failure("http://a:8080");
        // 现在 a 在冷却中，next 应该跳过 a 直接选 b
        let url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url, "http://b:8080");
        // 再次 next 仍然只能选 b（a 还在冷却）
        let url2 = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url2, "http://b:8080");
    }

    #[test]
    fn next_returns_none_when_all_proxies_in_cooldown() {
        let pool = make_pool(vec!["http://a:8080"]);
        pool.mark_failure("http://a:8080");
        assert_eq!(pool.next(ProxyCategory::Html), None);
    }

    #[test]
    fn next_returns_none_for_empty_pool() {
        let pool = make_pool(vec![]);
        assert_eq!(pool.next(ProxyCategory::Html), None);
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn next_filters_by_category() {
        let entries = vec![
            ProxyEntry::new("http://html:8080".to_string(), ProxyCategory::Html),
            ProxyEntry::new("http://media:8080".to_string(), ProxyCategory::Media),
        ];
        let pool = ProxyPool::new(entries, Duration::from_secs(60), Duration::from_secs(30));
        // Html category 只能选 html 代理
        let html_url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(html_url, "http://html:8080");
        // Media category 只能选 media 代理
        let media_url = pool.next(ProxyCategory::Media).unwrap();
        assert_eq!(media_url, "http://media:8080");
    }

    #[test]
    fn next_returns_none_when_category_has_no_proxies() {
        let entries = vec![ProxyEntry::new(
            "http://html:8080".to_string(),
            ProxyCategory::Html,
        )];
        let pool = ProxyPool::new(entries, Duration::from_secs(60), Duration::from_secs(30));
        // 池中只有 Html 代理，请求 Media 应返回 None
        assert_eq!(pool.next(ProxyCategory::Media), None);
    }

    // =========================================================================
    // sticky() 粘性会话
    // =========================================================================

    #[test]
    fn sticky_returns_same_url_within_ttl() {
        let pool = make_pool(vec!["http://a:8080", "http://b:8080"]);
        let url1 = pool.sticky("session-1").unwrap();
        // L5 修复：多次调用都应返回同一代理（粘性），不只是两次
        let url2 = pool.sticky("session-1").unwrap();
        let url3 = pool.sticky("session-1").unwrap();
        let url4 = pool.sticky("session-1").unwrap();
        assert_eq!(url1, url2, "same session_id should return same url within TTL");
        assert_eq!(url2, url3, "sticky must be stable across multiple calls");
        assert_eq!(url3, url4, "sticky must be stable across multiple calls");
        // 验证返回的 URL 确实在池中（不是凭空生成）
        assert!(
            url1 == "http://a:8080" || url1 == "http://b:8080",
            "sticky url must be one of the pool entries"
        );
    }

    #[test]
    fn sticky_different_sessions_get_different_urls() {
        let pool = make_pool(vec!["http://a:8080", "http://b:8080"]);
        let url1 = pool.sticky("session-1").unwrap();
        let url2 = pool.sticky("session-2").unwrap();
        assert_ne!(url1, url2, "different sessions should get different urls");
    }

    #[test]
    fn sticky_reselects_after_ttl_expiry() {
        // TTL = 1ms：立即过期
        let pool =
            make_pool_with_ttl(vec!["http://a:8080", "http://b:8080"], Duration::from_millis(1));
        let _url1 = pool.sticky("session-1").unwrap();
        // TTL 已过期，重选应该返回有效 URL（不 panic，不 None）
        thread::sleep(Duration::from_millis(5));
        let url2 = pool.sticky("session-1").unwrap();
        // L5 修复：验证重选后的 URL 在池中
        assert!(
            url2 == "http://a:8080" || url2 == "http://b:8080",
            "reselected url should be one of the pool entries"
        );
        // L5 修复：验证绑定已刷新——后续调用应返回 url2（新 TTL 内稳定）
        let url3 = pool.sticky("session-1").unwrap();
        assert_eq!(
            url2, url3,
            "after reselection, sticky should be stable again within new TTL"
        );
    }

    #[test]
    fn sticky_reselects_when_binding_in_cooldown() {
        let pool = make_pool(vec!["http://a:8080", "http://b:8080"]);
        let url1 = pool.sticky("session-1").unwrap();
        pool.mark_failure(&url1);
        // url1 在冷却中，sticky 应该重选另一个
        let url2 = pool.sticky("session-1").unwrap();
        assert_ne!(url1, url2, "sticky should reselect when binding is in cooldown");
        // L5 修复：验证重选的代理未在冷却中（即与 mark_failure 的代理不同）
        assert!(
            url2 == "http://a:8080" || url2 == "http://b:8080",
            "reselected url must be a valid pool entry"
        );
        // L5 修复：验证新绑定稳定——后续调用仍返回 url2
        let url3 = pool.sticky("session-1").unwrap();
        assert_eq!(
            url2, url3,
            "sticky should be stable after reselecting from cooldown"
        );
    }

    #[test]
    fn sticky_returns_none_when_all_in_cooldown() {
        let pool = make_pool(vec!["http://a:8080"]);
        pool.mark_failure("http://a:8080");
        assert_eq!(pool.sticky("session-1"), None);
    }

    #[test]
    fn sticky_returns_none_for_empty_pool() {
        let pool = make_pool(vec![]);
        assert_eq!(pool.sticky("session-1"), None);
    }

    // =========================================================================
    // mark_failure / mark_success
    // =========================================================================

    #[test]
    fn mark_failure_sets_cooldown() {
        let pool = make_pool(vec!["http://a:8080"]);
        pool.mark_failure("http://a:8080");
        // 验证 a 进入冷却（next 返回 None）
        assert_eq!(pool.next(ProxyCategory::Html), None);
    }

    #[test]
    fn mark_success_restores_health() {
        let pool = make_pool(vec!["http://a:8080"]);
        pool.mark_failure("http://a:8080");
        assert_eq!(pool.next(ProxyCategory::Html), None);
        pool.mark_success("http://a:8080");
        // 恢复健康后可重新使用
        let url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url, "http://a:8080");
    }

    #[test]
    fn mark_failure_unknown_url_does_not_panic() {
        let pool = make_pool(vec!["http://a:8080"]);
        // 不存在 url 不 panic
        pool.mark_failure("http://unknown:8080");
        // a 仍然可用
        let url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url, "http://a:8080");
    }

    #[test]
    fn mark_success_unknown_url_does_not_panic() {
        let pool = make_pool(vec!["http://a:8080"]);
        pool.mark_success("http://unknown:8080");
        // a 仍然可用
        let url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url, "http://a:8080");
    }

    // =========================================================================
    // from_urls / len / is_empty
    // =========================================================================

    #[test]
    fn from_urls_creates_pool_with_html_category() {
        let pool = ProxyPool::from_urls(
            vec!["http://a:8080".to_string(), "http://b:8080".to_string()],
            Duration::from_secs(60),
            Duration::from_secs(30),
        );
        assert_eq!(pool.len(), 2);
        assert!(!pool.is_empty());
        // from_urls 默认 Html category，应能取到
        let url = pool.next(ProxyCategory::Html).unwrap();
        assert_eq!(url, "http://a:8080");
    }

    #[test]
    fn len_and_is_empty_reflect_pool_state() {
        let empty_pool = make_pool(vec![]);
        assert!(empty_pool.is_empty());
        assert_eq!(empty_pool.len(), 0);

        let pool = make_pool(vec!["http://a:8080", "http://b:8080"]);
        assert!(!pool.is_empty());
        assert_eq!(pool.len(), 2);
    }

    // =========================================================================
    // 并发测试（L4 修复：验证 ProxyPool 在多线程并发下不 panic / 不 data race）
    // =========================================================================

    #[test]
    fn concurrent_next_does_not_panic_and_returns_valid_urls() {
        // L4 修复：多线程并发调用 next() 验证线程安全
        // ProxyPool 内部用 Atomic* + DashMap，&self 可并发调用
        let pool = std::sync::Arc::new(make_pool(vec![
            "http://a:8080",
            "http://b:8080",
            "http://c:8080",
            "http://d:8080",
        ]));
        let threads = 8;
        let calls_per_thread = 100;
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let pool_clone = std::sync::Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                let mut urls = Vec::with_capacity(calls_per_thread);
                for _ in 0..calls_per_thread {
                    if let Some(url) = pool_clone.next(ProxyCategory::Html) {
                        urls.push(url);
                    }
                }
                urls
            }));
        }
        // 所有线程都应成功完成（不 panic）
        let mut total_urls = 0usize;
        for handle in handles {
            let urls = handle.join().expect("thread should not panic");
            // 每次返回的 URL 必须在池中（不是 data race 导致的乱码）
            for url in &urls {
                assert!(
                    url == "http://a:8080"
                        || url == "http://b:8080"
                        || url == "http://c:8080"
                        || url == "http://d:8080",
                    "concurrent next() returned invalid url: {}",
                    url
                );
            }
            total_urls += urls.len();
        }
        // 验证总调用数 = threads * calls_per_thread（无代理冷却，全部应返回 Some）
        assert_eq!(
            total_urls,
            threads * calls_per_thread,
            "all concurrent next() calls should return Some (no cooldown)"
        );
    }

    #[test]
    fn concurrent_mark_failure_and_next_does_not_data_race() {
        // L4 修复：多线程并发调用 mark_failure + next 验证无 data race
        // 场景：一些线程标记失败（让代理进入冷却），另一些线程取代理
        // 预期：不 panic，所有返回的 URL 都在池中
        let pool = std::sync::Arc::new(make_pool(vec![
            "http://a:8080",
            "http://b:8080",
            "http://c:8080",
            "http://d:8080",
        ]));
        let valid_urls = std::sync::Arc::new(vec![
            "http://a:8080".to_string(),
            "http://b:8080".to_string(),
            "http://c:8080".to_string(),
            "http://d:8080".to_string(),
        ]);
        let threads = 6;
        let mut handles = Vec::with_capacity(threads);
        // 一半线程做 mark_failure + mark_success 循环
        for t in 0..threads {
            let pool_clone = std::sync::Arc::clone(&pool);
            let valid_urls_clone = std::sync::Arc::clone(&valid_urls);
            handles.push(thread::spawn(move || {
                for i in 0..50 {
                    let url = &valid_urls_clone[(t + i) % valid_urls_clone.len()];
                    if i % 2 == 0 {
                        pool_clone.mark_failure(url);
                    } else {
                        pool_clone.mark_success(url);
                    }
                    // 同时调用 next 验证不 panic
                    if let Some(returned) = pool_clone.next(ProxyCategory::Html) {
                        // 返回的 URL 必须在池中
                        assert!(
                            valid_urls_clone.contains(&returned),
                            "concurrent next() returned url not in pool: {}",
                            returned
                        );
                    }
                }
            }));
        }
        // 所有线程应成功完成（不 panic，无 data race）
        for handle in handles {
            handle.join().expect("thread should not panic under concurrent access");
        }
    }

    #[test]
    fn concurrent_sticky_same_session_returns_consistent_url() {
        // T056 LOW-2 修复验证：多线程同 session_id 并发调用 sticky 必须返回同一 URL
        // 修复前：sticky 存在 TOCTOU 竞态，多线程同时发现绑定过期时可能各自重选
        //         并覆盖彼此的绑定，导致同 session_id 返回不同 URL。
        // 修复后：使用 DashMap entry API 原子 check-and-update，第一个线程插入绑定后，
        //         其余线程在 entry 锁内重新检查到有效绑定并复用，保证一致性。
        let pool = std::sync::Arc::new(make_pool(vec![
            "http://a:8080",
            "http://b:8080",
            "http://c:8080",
        ]));
        let threads = 4;
        let calls_per_thread = 20;
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let pool_clone = std::sync::Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                let mut urls = Vec::with_capacity(calls_per_thread);
                for _ in 0..calls_per_thread {
                    if let Some(url) = pool_clone.sticky("shared-session") {
                        urls.push(url);
                    }
                }
                urls
            }));
        }
        // 收集所有线程返回的 URL
        let mut all_urls: Vec<String> = Vec::new();
        for handle in handles {
            let urls = handle.join().expect("sticky thread should not panic");
            // 每个线程返回的 URL 都应在池中
            for url in &urls {
                assert!(
                    url == "http://a:8080" || url == "http://b:8080" || url == "http://c:8080",
                    "concurrent sticky() returned invalid url: {}",
                    url
                );
            }
            all_urls.extend(urls);
        }
        // LOW-2 修复核心断言：所有调用必须返回同一 URL（无 TOCTOU 竞态）
        let first_url = &all_urls[0];
        for url in &all_urls {
            assert_eq!(
                url, first_url,
                "all concurrent sticky() calls with same session_id must return the same URL"
            );
        }
    }

    // =========================================================================
    // T056 LOW-1: sticky 表容量限制
    // =========================================================================

    #[test]
    fn sticky_capacity_limit_evicts_expired_entries() {
        // LOW-1 修复：sticky 表超容量时清理过期绑定
        let pool = std::sync::Arc::new(
            make_pool(vec!["http://a:8080", "http://b:8080"])
                .with_sticky_max_capacity(3),
        );
        // 插入 3 个绑定（达到容量上限）
        pool.sticky("session-1").unwrap();
        pool.sticky("session-2").unwrap();
        pool.sticky("session-3").unwrap();
        assert_eq!(pool.sticky.len(), 3);

        // 等待 TTL 过期（测试用 TTL = 60s，这里手动修改 expires_at 模拟过期）
        // 直接操作内部 sticky 表，将所有绑定标记为已过期
        {
            let now = Instant::now();
            for mut entry in pool.sticky.iter_mut() {
                entry.expires_at = now - Duration::from_secs(1);
            }
        }

        // 插入第 4 个绑定：应触发 evict_expired_sticky，清理过期绑定后插入新的
        let url = pool.sticky("session-4");
        assert!(url.is_some(), "sticky should return a URL even at capacity");
        // 过期绑定被清理，新绑定被插入
        assert!(
            pool.sticky.len() <= 3,
            "sticky table should not exceed capacity after eviction: len={}",
            pool.sticky.len()
        );
        // session-4 的绑定应该存在
        assert!(
            pool.sticky.contains_key("session-4"),
            "session-4 binding should be inserted after eviction"
        );
    }

    #[test]
    fn sticky_capacity_limit_degrades_gracefully_when_all_valid() {
        // LOW-1 修复：超容量且所有绑定都有效（未过期）时，降级返回代理 URL 但不绑定
        let pool = std::sync::Arc::new(
            make_pool(vec!["http://a:8080", "http://b:8080"])
                .with_sticky_max_capacity(2),
        );
        // 插入 2 个绑定（达到容量上限）
        pool.sticky("session-1").unwrap();
        pool.sticky("session-2").unwrap();
        assert_eq!(pool.sticky.len(), 2);

        // 插入第 3 个绑定：所有绑定都在 TTL 内（未过期），无法清理
        // 应降级：返回代理 URL 但不绑定
        let url = pool.sticky("session-3");
        assert!(
            url.is_some(),
            "sticky should still return a proxy URL when at capacity (degraded)"
        );
        assert!(
            url.as_ref() == Some(&"http://a:8080".to_string())
                || url.as_ref() == Some(&"http://b:8080".to_string()),
            "degraded sticky should return a valid pool URL"
        );
        // session-3 未被绑定（降级）
        assert!(
            !pool.sticky.contains_key("session-3"),
            "session-3 should NOT be bound when at capacity with all valid entries"
        );
        // sticky 表未超容量
        assert!(
            pool.sticky.len() <= 2,
            "sticky table should not exceed capacity: len={}",
            pool.sticky.len()
        );
    }

    #[test]
    fn sticky_capacity_limit_still_returns_valid_binding_for_existing_session() {
        // LOW-1 修复：超容量时，已有有效绑定的 session 仍应正常命中（不受容量限制影响）
        let pool = std::sync::Arc::new(
            make_pool(vec!["http://a:8080", "http://b:8080"])
                .with_sticky_max_capacity(1),
        );
        // 插入 1 个绑定（达到容量上限）
        let url1 = pool.sticky("session-1").unwrap();
        assert_eq!(pool.sticky.len(), 1);

        // 尝试插入第 2 个绑定（应降级，不绑定）
        let url2 = pool.sticky("session-2").unwrap();
        assert!(
            url2 == "http://a:8080" || url2 == "http://b:8080",
            "degraded sticky should return a valid pool URL"
        );
        assert!(
            !pool.sticky.contains_key("session-2"),
            "session-2 should not be bound (at capacity)"
        );

        // session-1 的绑定仍有效，应返回同一 URL
        let url1_again = pool.sticky("session-1").unwrap();
        assert_eq!(
            url1, url1_again,
            "existing valid binding should still be honored despite capacity limit"
        );
    }

    // =========================================================================
    // T056 LOW-2: sticky TOCTOU 竞态修复（entry API 原子 check-and-update）
    // =========================================================================

    #[test]
    fn concurrent_sticky_touctou_race_all_threads_get_same_url() {
        // LOW-2 修复核心测试：多线程并发 sticky 同一 session_id，
        // 所有线程必须返回同一 URL（entry API 保证原子性）。
        //
        // 场景：TTL 极短（1ms），所有线程几乎同时发现绑定过期，
        // 进入慢路径重选。修复前：各线程独立 rr_pick + insert，互相覆盖，
        // 返回不同 URL。修复后：entry API 保证只有一个线程 insert，
        // 其余线程在 entry 锁内复用。
        let pool = std::sync::Arc::new(
            make_pool_with_ttl(
                vec!["http://a:8080", "http://b:8080", "http://c:8080", "http://d:8080"],
                Duration::from_millis(1),
            )
            .with_sticky_max_capacity(100),
        );
        // 先建立初始绑定
        let initial_url = pool.sticky("race-session").unwrap();
        // 等待 TTL 过期
        thread::sleep(Duration::from_millis(5));

        let threads = 8;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(threads));
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let pool_clone = std::sync::Arc::clone(&pool);
            let barrier_clone = std::sync::Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier_clone.wait();
                pool_clone.sticky("race-session").unwrap()
            }));
        }

        let mut results = Vec::with_capacity(threads);
        for handle in handles {
            results.push(handle.join().expect("thread should not panic"));
        }

        // LOW-2 核心断言：所有线程必须返回同一 URL
        let first = &results[0];
        for (i, url) in results.iter().enumerate() {
            assert_eq!(
                url, first,
                "thread {} returned different URL (TOCTOU race not fixed): got {}, expected {}",
                i, url, first
            );
        }
        // 返回的 URL 应在池中
        assert!(
            first == "http://a:8080"
                || first == "http://b:8080"
                || first == "http://c:8080"
                || first == "http://d:8080",
            "returned URL should be a valid pool entry: {}",
            first
        );
        // initial_url 可能与 results[0] 不同（TTL 过期后重选），这是正常的
        let _ = initial_url; // 避免未使用变量警告
    }

    #[test]
    fn concurrent_sticky_different_sessions_independent_bindings() {
        // LOW-2 修复：不同 session_id 的并发 sticky 应各自独立绑定，互不干扰
        let pool = std::sync::Arc::new(make_pool(vec![
            "http://a:8080",
            "http://b:8080",
            "http://c:8080",
        ]));
        let sessions: Vec<String> = (0..10).map(|i| format!("session-{}", i)).collect();
        let sessions_arc = std::sync::Arc::new(sessions);
        let threads = 10;
        let mut handles = Vec::with_capacity(threads);
        for t in 0..threads {
            let pool_clone = std::sync::Arc::clone(&pool);
            let sessions_clone = std::sync::Arc::clone(&sessions_arc);
            handles.push(thread::spawn(move || {
                let session = &sessions_clone[t];
                let url1 = pool_clone.sticky(session).unwrap();
                let url2 = pool_clone.sticky(session).unwrap();
                let url3 = pool_clone.sticky(session).unwrap();
                assert_eq!(url1, url2, "same session should return same url (call 2)");
                assert_eq!(url2, url3, "same session should return same url (call 3)");
                url1
            }));
        }
        for (t, handle) in handles.into_iter().enumerate() {
            let url = handle.join().expect("thread should not panic");
            assert!(
                url == "http://a:8080" || url == "http://b:8080" || url == "http://c:8080",
                "thread {} returned invalid url: {}",
                t,
                url
            );
        }
    }
}
