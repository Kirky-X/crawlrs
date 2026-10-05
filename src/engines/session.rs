// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Session 身份三件套（F2）——代理 + UA/TLS 档 + Cookie 的绑定与记分退休
//!
//! 把「同一来源的连续请求」绑到单一身份上：身份持有代理 URL、UA profile
//! 下标（profile 内部 UA / Accept-Language / sec-ch-ua / viewport /
//! tls_emulation 已绑定一致，见 `utils::ua_pool`）与 Cookie 缓存，按
//! error_score 记分退休（mark_bad +1.0 / mark_good -0.5 下限 0），score
//! 达 `max_error_score`、uses 达 `max_uses` 或 age 达 `max_age` 时从池中
//! 移除，下一次 `acquire` 获得全新身份（新代理 + 新 UA 档 + 空 Cookie）。
//!
//! # Cookie 手动通道的语义限制（设计取舍，显性声明）
//!
//! 响应 `Set-Cookie` 头按 `name=value` 对捕获累进身份 cookie 缓存，后续
//! 请求注入 `Cookie` 头——**无 expiry / path 属性语义，作用域按 host 隔离**：
//! - 不过期：服务端下发的短时效 cookie 会一直带到过期之后（可能被拒）
//! - 按 host 分桶：cookie 只写入捕获时的目标 host 桶，渲染只取目标 host 桶
//!   ——session_id 跨 host 亲和同一身份时，A 站 Set-Cookie 不会注入其他 host
//!   的请求；桶内无 expiry / path / domain 属性语义
//! - 只取首个 `name=value` 对：Path/Secure/HttpOnly 等属性全部忽略
//!
//! 目标站 cookie 语义复杂时建议关闭 `[session].enabled`。浏览器引擎
//! （Playwright/MLLM）的 context 自管 cookie，不经此通道——Session 对
//! 浏览器路径仅生效 UA/代理选择。
//!
//! # 并发模型
//!
//! 与 `HostDiscipline` 同款 `DashMap` 纪律：容量检查、惰性清扫都在取
//! `entry` 守卫**之前**完成（entry 持 shard 写锁期间 `len()`/`retain`
//! 需跨 shard 锁 → 死锁）；身份内部的 score / cookie 用 `parking_lot::
//! Mutex`，锁内不触碰任何 map，守卫不跨 `await`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::config::settings::SessionSettings;
use crate::engines::provider::{ProxyCategory, ProxyProvider};
use crate::utils::ua_pool::{UaPool, UaProfile};

/// 惰性清扫间隔：达到间隔后下一次 `acquire` 触发一次全表回收
/// （对齐 `HostDiscipline::maybe_sweep`：不引入后台任务，60s 摊销）
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// 单个会话身份：代理 + UA/TLS 档 + Cookie 的一致绑定单元
///
/// 通过 `Arc<SessionIdentity>` 在 route 层与 SessionManager 之间共享；
/// 可变状态（uses / error_score / cookie_jar）全部内部可变。
pub struct SessionIdentity {
    /// 单调递增身份号（退休换新后 id 不同，调用方可据此感知切换）
    pub id: u64,
    /// 亲和键（session_id 哈希或 host 哈希）——身份在池中的键，退休按此移除。
    /// 生产路径经 `SessionManager::retire` 按键移除（键由入参重算），
    /// 字段本体由测试读取断言退休语义。
    #[cfg_attr(not(test), allow(dead_code))]
    affinity_key: u64,
    /// 绑定的代理 URL（可能含 userinfo 凭据，日志输出必须脱敏）
    pub proxy_url: Option<String>,
    /// UA profile 在 [`UaPool`] 对应分组中的下标。
    /// 引用下标而非 `&'static UaProfile` 引用：保住 `Arc<SessionIdentity>`
    /// 的 'static 语义，profile 本体是编译期常量表，按 (分组, 下标) 解析。
    ua_profile_idx: usize,
    /// 身份设备分组（mobile/desktop 各自成档，避免 UA 与视口/平台矛盾）
    mobile: bool,
    /// 手动 Cookie 通道累积的 Cookie 头值，按捕获时的目标 host 分桶
    /// （`host → "k=v; k2=v2"`，桶内最旧在前），单桶上限 `cookie_cache_max_bytes`。
    /// host 分桶保证 session_id 跨 host 亲和同一身份时，A 站 Set-Cookie
    /// 不会注入其他 host 的请求（跨站凭据隔离）
    cookie_jar: parking_lot::Mutex<HashMap<String, String>>,
    /// 创建时刻（age 退休依据）
    created_at: Instant,
    /// 使用次数（每次 `acquire` 递增；达 `max_uses` 退休）
    uses: AtomicU64,
    /// 错误分（mark_bad +1.0 / mark_good -0.5 下限 0；≥ `max_error_score` 退休）
    error_score: parking_lot::Mutex<f64>,
}

impl SessionIdentity {
    /// 当前错误分
    pub fn error_score(&self) -> f64 {
        *self.error_score.lock()
    }

    /// 已使用次数
    pub fn uses(&self) -> u64 {
        self.uses.load(Ordering::Relaxed)
    }

    /// 身份年龄
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 解析身份绑定的 UA profile（UA 三件 header 与 TLS 档同源一致）
    pub fn ua_profile<'a>(&self, pool: &'a UaPool) -> &'a UaProfile {
        let group = if self.mobile {
            &pool.mobile
        } else {
            &pool.desktop
        };
        // 取模防御：内置池内容恒定，仍避免任何越界 panic
        &group[self.ua_profile_idx % group.len()]
    }
}

/// Session 身份管理器
///
/// `enabled=false`（默认）时 route 层不进入 session 路径，本管理器不被
/// 触碰（保持现状盲轮换）。
pub struct SessionManager {
    /// 是否启用（`[session].enabled`）
    enabled: bool,
    /// 身份池：亲和键 → 身份
    sessions: DashMap<u64, std::sync::Arc<SessionIdentity>>,
    /// UA 池（与 EngineRouter 共享同一实例，profile 下标同源）
    ua_pool: std::sync::Arc<UaPool>,
    /// 代理提供者（`None` = 未配置代理，身份 proxy_url 为 None）
    proxy_provider: Option<std::sync::Arc<dyn ProxyProvider>>,
    /// 池容量上限（内存护栏）
    max_sessions: usize,
    /// 退休分数阈值
    max_error_score: f64,
    /// 单身份使用上限
    max_uses: u64,
    /// 身份最长寿命
    max_age: Duration,
    /// 每身份 Cookie 缓存上限（字节）
    cookie_cache_max_bytes: usize,
    /// 身份号发号器（单调递增）
    next_id: AtomicU64,
    /// 上次全表清扫时刻（惰性触发，间隔 [`SWEEP_INTERVAL`]）
    last_sweep: parking_lot::Mutex<Instant>,
}

impl SessionManager {
    /// 从 `[session]` 配置构造
    ///
    /// `proxy_provider` 与引擎层共享同一 `Arc<dyn ProxyProvider>`；代理的
    /// 具体选择以 `ProxyProvider::sticky`（键 = 身份 id）实现——代理与身份
    /// 同生共死：身份退休换代理，代理冷却时 sticky 内部重选健康代理。
    pub fn from_settings(
        settings: &SessionSettings,
        ua_pool: std::sync::Arc<UaPool>,
        proxy_provider: Option<std::sync::Arc<dyn ProxyProvider>>,
    ) -> Self {
        Self {
            enabled: settings.enabled,
            sessions: DashMap::new(),
            ua_pool,
            proxy_provider,
            max_sessions: settings.max_sessions.max(1),
            max_error_score: settings.max_error_score,
            max_uses: settings.max_uses.max(1),
            max_age: Duration::from_secs(settings.max_age_secs),
            cookie_cache_max_bytes: settings.cookie_cache_max_bytes.max(64),
            next_id: AtomicU64::new(0),
            last_sweep: parking_lot::Mutex::new(Instant::now()),
        }
    }

    /// 未启用构造（默认配置 + enabled=false；与 `SessionSettings::default`
    /// 一致，route 层据此走现状盲轮换路径）
    pub fn disabled() -> Self {
        Self::from_settings(
            &SessionSettings::default(),
            std::sync::Arc::new(UaPool::new()),
            None,
        )
    }

    /// 是否启用（route 层据此选择 session / 盲轮换路径）
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 与 EngineRouter 共享的 UA 池
    pub fn ua_pool(&self) -> &std::sync::Arc<UaPool> {
        &self.ua_pool
    }

    /// 获取（或创建）host/session 亲和的会话身份
    ///
    /// - `session_id` Some：按其哈希亲和（同一会话跨 host 同身份）
    /// - `session_id` None：按 host 亲和（同 host 稳定身份）
    /// - 命中身份已满足退休条件（score/uses/age）→ 原子换新并返回新身份
    /// - 池达 `max_sessions` 上限：降级返回临时身份（本请求仍获得完整
    ///   三件套），不入池（对齐 `ProxyPool::sticky` 的降级不拒服语义）
    ///
    /// 每次调用递增身份 uses 计数。
    pub fn acquire(
        &self,
        host: &str,
        session_id: Option<&str>,
        mobile: bool,
    ) -> std::sync::Arc<SessionIdentity> {
        use dashmap::mapref::entry::Entry;

        self.maybe_sweep();
        let key = Self::affinity_key(host, session_id);
        // 容量检查必须在 entry() 之前：entry 持 shard 写锁期间 len() 需
        // 读取所有 shard 读锁 → 死锁（对齐 ProxyPool::sticky 注释约定）
        let at_capacity = self.sessions.len() >= self.max_sessions;

        let identity = match self.sessions.entry(key) {
            Entry::Occupied(mut occ) => {
                let existing = occ.get().clone();
                if self.should_retire(&existing) {
                    let fresh = self.build_identity(key, mobile);
                    log::info!(
                        "session: identity #{} retired (score={:.1}, uses={}, age={:?}), \
                         replacing with #{}",
                        existing.id,
                        existing.error_score(),
                        existing.uses(),
                        existing.age(),
                        fresh.id
                    );
                    occ.insert(fresh.clone());
                    fresh
                } else {
                    existing
                }
            }
            Entry::Vacant(vac) => {
                let fresh = self.build_identity(key, mobile);
                if at_capacity {
                    // 降级：返回临时身份（本请求完整三件套），不入池
                    log::debug!(
                        "session pool at capacity ({}), serving ephemeral identity #{}",
                        self.max_sessions,
                        fresh.id
                    );
                } else {
                    vac.insert(fresh.clone());
                }
                fresh
            }
        };
        identity.uses.fetch_add(1, Ordering::Relaxed);
        identity
    }

    /// 记坏分：score +1.0（401/403/429 与反爬挑战页调用）
    ///
    /// 退休是惰性的：score 达阈值后由下一次 `acquire` 换新身份。
    pub fn mark_bad(&self, identity: &SessionIdentity) {
        *identity.error_score.lock() += 1.0;
    }

    /// 记好分：score -0.5，下限 0（2xx 成功调用）
    pub fn mark_good(&self, identity: &SessionIdentity) {
        let mut score = identity.error_score.lock();
        *score = (*score - 0.5).max(0.0);
    }

    /// 强制退休 host/session 亲和的当前身份（A 簇 `is_stalled` 置位时调用）
    ///
    /// 下一次 `acquire` 即获得新身份；身份不存在时静默（幂等）。
    pub fn retire(&self, host: &str, session_id: Option<&str>) {
        let key = Self::affinity_key(host, session_id);
        if let Some((_, identity)) = self.sessions.remove(&key) {
            log::info!(
                "session: identity #{} force-retired for host {} (score={:.1}, uses={})",
                identity.id,
                host,
                identity.error_score(),
                identity.uses()
            );
        }
    }

    /// 捕获响应 `Set-Cookie` 头到身份 Cookie 缓存（手动通道，语义限制见
    /// 模块 doc）
    ///
    /// `host` 为本次响应的目标 host，cookie 只进该 host 的桶；只取
    /// `name=value` 对（首个 `;` 前）；单条超限丢弃该条（warn），累积超限
    /// 丢最旧条目。headers 为引擎返回的 `HashMap<String,String>`，同名头会
    /// 折叠（HashMap 语义），能捕获多少捕获多少。
    ///
    /// F2：reqwest 引擎把重定向链各跳 + 最终响应的 Cookie 以 `'\n'` 连接进
    /// 单一 set-cookie 值，本方法按行拆分逐条捕获。
    pub fn capture_cookies(
        &self,
        identity: &SessionIdentity,
        host: &str,
        headers: &HashMap<String, String>,
    ) {
        let mut jar = identity.cookie_jar.lock();
        let blob = jar.entry(host.to_string()).or_default();
        for (name, value) in headers {
            if !name.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            for cookie_str in value.split('\n') {
                // 手动通道：只取 "name=value" 对（首个 ';' 前），属性全忽略
                let pair = cookie_str.split(';').next().unwrap_or("").trim();
                if pair.is_empty() {
                    continue;
                }
                let pair_len = pair.len() + if blob.is_empty() { 0 } else { 2 };
                if pair_len > self.cookie_cache_max_bytes {
                    log::warn!(
                        "session: dropping set-cookie pair ({} bytes) exceeding cache cap {} \
                         for identity #{}",
                        pair.len(),
                        self.cookie_cache_max_bytes,
                        identity.id
                    );
                    continue;
                }
                if !blob.is_empty() {
                    blob.push_str("; ");
                }
                blob.push_str(pair);
                // 容量护栏：超出上限丢最旧条目（分号分隔的首段）
                while blob.len() > self.cookie_cache_max_bytes {
                    match blob.find("; ") {
                        Some(pos) => {
                            blob.drain(..pos + 2);
                        }
                        None => {
                            blob.clear();
                            break;
                        }
                    }
                }
            }
        }
    }

    /// 渲染身份 Cookie 头值——只取目标 `host` 的桶（跨 host 凭据隔离；
    /// 缓存为空时 `None`，调用方不注入该头）
    pub fn cookie_header(&self, identity: &SessionIdentity, host: &str) -> Option<String> {
        let jar = identity.cookie_jar.lock();
        jar.get(host).filter(|blob| !blob.is_empty()).cloned()
    }

    /// host/session 亲和键：session_id Some 时按其哈希（会话跨 host 同身份），
    /// None 时按 host 哈希（同 host 稳定身份）
    fn affinity_key(host: &str, session_id: Option<&str>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match session_id {
            Some(sid) => sid.hash(&mut hasher),
            None => host.hash(&mut hasher),
        }
        hasher.finish()
    }

    /// 身份是否满足退休条件（score / uses / age 任一达标）
    fn should_retire(&self, identity: &SessionIdentity) -> bool {
        identity.error_score() >= self.max_error_score
            || identity.uses() >= self.max_uses
            || identity.age() >= self.max_age
    }

    /// 构造新身份：代理经 `sticky`（键 = 身份 id，代理与身份同生共死），
    /// UA profile 按 `pick_seeded(id)` 分组取（mobile/desktop 不串档）
    fn build_identity(&self, key: u64, mobile: bool) -> std::sync::Arc<SessionIdentity> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let proxy_url = self.proxy_provider.as_ref().and_then(|provider| {
            let sticky_key = format!("session-identity-{id}");
            provider
                .sticky(&sticky_key)
                .or_else(|| provider.next(ProxyCategory::Html))
        });
        // pick_seeded(id) 与该下标解析同源（同 id 必同 profile，见 ua_pool）
        let pool_len = if mobile {
            self.ua_pool.mobile.len()
        } else {
            self.ua_pool.desktop.len()
        };
        let ua_profile_idx = (id as usize) % pool_len.max(1);
        std::sync::Arc::new(SessionIdentity {
            id,
            affinity_key: key,
            proxy_url,
            ua_profile_idx,
            mobile,
            cookie_jar: parking_lot::Mutex::new(HashMap::new()),
            created_at: Instant::now(),
            uses: AtomicU64::new(0),
            error_score: parking_lot::Mutex::new(0.0),
        })
    }

    /// 距上次清扫超过 [`SWEEP_INTERVAL`] 时做一次全表回收（移除已满足
    /// 退休条件的身份；下一次 acquire 会为对应亲和键重建）。
    ///
    /// 在 `acquire` 上惰性触发（不引入后台任务），且必须先于 `entry`
    /// 守卫调用（`retain` 逐 shard 持锁）。
    fn maybe_sweep(&self) {
        let mut last = self.last_sweep.lock();
        if last.elapsed() < SWEEP_INTERVAL {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.sessions
            .retain(|_, identity| !self.should_retire(identity));
    }

    /// 池内身份数（观测用）
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// 池是否为空
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::proxy_pool::ProxyPool;
    use std::sync::Arc;

    /// 三代理池（sticky 键不同 → RR 取到不同 URL，代理轮换可观测）
    fn make_provider() -> Arc<dyn ProxyProvider> {
        Arc::new(ProxyPool::from_urls(
            vec![
                "http://proxy-a:8080".to_string(),
                "http://proxy-b:8080".to_string(),
                "http://proxy-c:8080".to_string(),
            ],
            Duration::from_secs(60),
            Duration::from_secs(30),
        ))
    }

    /// 定制参数的测试管理器（其余字段对齐 spec 默认）
    fn make_manager(
        max_error_score: f64,
        max_uses: u64,
        max_age_secs: u64,
        cookie_cap: usize,
    ) -> SessionManager {
        SessionManager::from_settings(
            &SessionSettings {
                enabled: true,
                max_sessions: 16,
                max_error_score,
                max_uses,
                max_age_secs,
                cookie_cache_max_bytes: cookie_cap,
            },
            Arc::new(UaPool::new()),
            Some(make_provider()),
        )
    }

    /// 同 (host, session_id) 连续 acquire 返回同一身份（代理 URL + UA 档稳定）
    #[test]
    fn acquire_same_host_session_returns_stable_identity() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let a = mgr.acquire("shop.example.com", Some("sess-1"), false);
        let b = mgr.acquire("shop.example.com", Some("sess-1"), false);
        let c = mgr.acquire("shop.example.com", Some("sess-1"), false);
        assert_eq!(a.id, b.id, "第二次 acquire 必须命中同一身份");
        assert_eq!(b.id, c.id, "第三次 acquire 仍命中同一身份");
        assert_eq!(
            a.proxy_url, b.proxy_url,
            "同身份代理 URL 必须稳定（sticky 绑定）"
        );
        assert!(a.proxy_url.is_some(), "配置了 provider 时身份必须持有代理");
        let pool = mgr.ua_pool();
        assert_eq!(
            a.ua_profile(pool).ua,
            b.ua_profile(pool).ua,
            "同身份 UA 必须稳定"
        );
        assert_eq!(
            a.ua_profile(pool).tls_emulation,
            b.ua_profile(pool).tls_emulation,
            "同身份 TLS 档必须稳定"
        );
    }

    /// 不同 session 各自独立身份（代理互不相同）
    #[test]
    fn different_sessions_get_different_identities() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let s1 = mgr.acquire("h.example.com", Some("s1"), false);
        let s2 = mgr.acquire("h.example.com", Some("s2"), false);
        assert_ne!(s1.id, s2.id, "不同 session_id 必须是不同身份");
        assert_ne!(s1.proxy_url, s2.proxy_url, "不同身份从 RR 池取得不同代理");
    }

    /// mark_bad 两次 score=2.0 < 3.0 不退休；第三次达阈值身份被移除，
    /// acquire 返回全新身份（score 归零）
    #[test]
    fn error_score_retirement_on_threshold() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let id = mgr.acquire("api.example.com", Some("s"), false);
        mgr.mark_bad(&id);
        mgr.mark_bad(&id);
        assert!(
            (id.error_score() - 2.0).abs() < f64::EPSILON,
            "两次 mark_bad 后 score=2.0"
        );
        assert_eq!(
            mgr.acquire("api.example.com", Some("s"), false).id,
            id.id,
            "score 2.0 < 3.0 不得退休"
        );
        mgr.mark_bad(&id);
        assert!((id.error_score() - 3.0).abs() < f64::EPSILON);
        let fresh = mgr.acquire("api.example.com", Some("s"), false);
        assert_ne!(fresh.id, id.id, "score 达阈值后 acquire 必须换新身份");
        assert!(
            (fresh.error_score() - 0.0).abs() < f64::EPSILON,
            "新身份 score 归零"
        );
        assert_ne!(
            mgr.sessions.get(&id.affinity_key).map(|i| i.id),
            Some(id.id),
            "旧身份不得继续留在原亲和键上"
        );
        assert_eq!(
            fresh.affinity_key, id.affinity_key,
            "亲和键不变（同 session）"
        );
    }

    /// mark_good 抵扣：+1.0 后 mark_good 两次 score 回 0 且不为负
    #[test]
    fn mark_good_floors_at_zero() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let id = mgr.acquire("ok.example.com", Some("s"), false);
        mgr.mark_bad(&id);
        assert!((id.error_score() - 1.0).abs() < f64::EPSILON);
        mgr.mark_good(&id);
        mgr.mark_good(&id);
        assert_eq!(id.error_score(), 0.0, "mark_good 抵扣到 0 为下限，不得为负");
    }

    /// uses 达 max_uses 触发退休（不依赖错误分数）
    #[test]
    fn max_uses_retires_identity() {
        let mgr = make_manager(100.0, 2, 1800, 4096); // score 阈值调高，隔离 uses 维度
        let a = mgr.acquire("h.example.com", Some("s"), false);
        let b = mgr.acquire("h.example.com", Some("s"), false);
        assert_eq!(a.id, b.id, "uses=1、2 时仍同一身份");
        assert_eq!(b.uses(), 2);
        let c = mgr.acquire("h.example.com", Some("s"), false);
        assert_ne!(c.id, a.id, "uses=2 达 max_uses 后必须换新身份");
        assert_eq!(c.uses(), 1, "新身份 uses 从本次 acquire 起计");
    }

    /// age 达 max_age 触发退休（max_age_secs=0 → 恒过期，验证 age 分支）
    #[test]
    fn max_age_zero_rotates_identity_every_acquire() {
        let mgr = make_manager(100.0, 100, 0, 4096);
        let a = mgr.acquire("h.example.com", Some("s"), false);
        let b = mgr.acquire("h.example.com", Some("s"), false);
        assert_ne!(a.id, b.id, "max_age=0 时每次 acquire 都必须换新身份");
    }

    /// retire 强制退休：同 host 下一次 acquire 换新身份（A 簇 stalled 触发的
    /// manager 层语义）
    #[test]
    fn retire_forces_new_identity_on_next_acquire() {
        let mgr = make_manager(100.0, 100, 1800, 4096);
        let host = "stall.example.com";
        let a = mgr.acquire(host, None, false);
        mgr.retire(host, None);
        let b = mgr.acquire(host, None, false);
        assert_ne!(a.id, b.id, "retire 后同 host acquire 必须换新身份");
        // 幂等：retire 不存在的身份不 panic
        mgr.retire(host, None);
        mgr.retire("never-seen.example.com", Some("nope"));
    }

    /// Set-Cookie 捕获注入：首个响应捕获 → cookie_header 渲染；非 Set-Cookie
    /// 头忽略；多次捕获累进；跨 host 不串桶
    #[test]
    fn capture_and_render_cookie_header() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let id = mgr.acquire("h.example.com", Some("s"), false);
        assert!(
            mgr.cookie_header(&id, "h.example.com").is_none(),
            "未捕获前无 Cookie 头"
        );

        let mut first = HashMap::new();
        first.insert("Content-Type".to_string(), "text/html".to_string());
        first.insert(
            "set-cookie".to_string(),
            "sid=abc123; Path=/; HttpOnly".to_string(),
        );
        mgr.capture_cookies(&id, "h.example.com", &first);
        assert_eq!(
            mgr.cookie_header(&id, "h.example.com").as_deref(),
            Some("sid=abc123"),
            "只取 name=value 对，属性丢弃"
        );

        let mut second = HashMap::new();
        second.insert("Set-Cookie".to_string(), "theme=dark".to_string());
        mgr.capture_cookies(&id, "h.example.com", &second);
        assert_eq!(
            mgr.cookie_header(&id, "h.example.com").as_deref(),
            Some("sid=abc123; theme=dark"),
            "多次捕获累进，旧条目在前"
        );
    }

    /// 跨 host 凭据隔离：session_id 亲和同一身份时，A 站捕获的 cookie
    /// 不得渲染到 B 站请求
    #[test]
    fn cookies_do_not_leak_across_hosts_under_session_affinity() {
        let mgr = make_manager(3.0, 100, 1800, 4096);
        let id = mgr.acquire("a.example.com", Some("shared-session"), false);
        let mut h = HashMap::new();
        h.insert(
            "set-cookie".to_string(),
            "secret=a-site-credential".to_string(),
        );
        mgr.capture_cookies(&id, "a.example.com", &h);

        assert_eq!(
            mgr.cookie_header(&id, "a.example.com").as_deref(),
            Some("secret=a-site-credential"),
            "目标 host 桶正常渲染"
        );
        assert!(
            mgr.cookie_header(&id, "b.example.com").is_none(),
            "非捕获 host 必须无 Cookie 头"
        );
        assert!(
            !mgr.cookie_header(&id, "b.example.com")
                .unwrap_or_default()
                .contains("a-site-credential"),
            "A 站凭据不得出现在其他 host 的请求中"
        );
    }

    /// cookie 超限截断：丢最旧条目、单条超限丢弃、均不 panic
    #[test]
    fn cookie_cache_truncates_oldest_without_panic() {
        let mgr = make_manager(3.0, 100, 1800, 64); // 64 字节上限（from_settings 下限钳制同值）
        let id = mgr.acquire("h.example.com", Some("s"), false);
        for i in 0..10 {
            let mut h = HashMap::new();
            h.insert("set-cookie".to_string(), format!("k{i}=value{i}"));
            mgr.capture_cookies(&id, "h.example.com", &h);
        }
        let blob = mgr
            .cookie_header(&id, "h.example.com")
            .expect("截断后仍有存活条目");
        assert!(blob.len() <= 64, "缓存 {blob:?} 超过 64 字节上限");
        assert!(!blob.contains("k0="), "最旧条目必须被丢弃，得到 {blob:?}");
        assert!(blob.contains("k9="), "最新条目必须保留，得到 {blob:?}");

        // 单条超限：整条丢弃（warn），不影响既有缓存
        let before = mgr.cookie_header(&id, "h.example.com").unwrap();
        let mut big = HashMap::new();
        big.insert("set-cookie".to_string(), "x".repeat(64));
        mgr.capture_cookies(&id, "h.example.com", &big);
        assert_eq!(
            mgr.cookie_header(&id, "h.example.com").as_deref(),
            Some(before.as_str()),
            "单条超限必须整条丢弃且不污染既有缓存"
        );
    }

    /// 池容量护栏：达 max_sessions 后新亲和降级为临时身份（不入池、不拒服）
    #[test]
    fn capacity_limit_degrades_to_ephemeral_identity() {
        let settings = SessionSettings {
            enabled: true,
            max_sessions: 1,
            max_error_score: 3.0,
            max_uses: 100,
            max_age_secs: 1800,
            cookie_cache_max_bytes: 4096,
        };
        let mgr = SessionManager::from_settings(
            &settings,
            Arc::new(UaPool::new()),
            Some(make_provider()),
        );
        let a = mgr.acquire("a.example.com", None, false);
        let b = mgr.acquire("b.example.com", None, false);
        assert_ne!(a.id, b.id);
        assert_eq!(mgr.len(), 1, "超限临时身份不得入池");
        assert!(b.proxy_url.is_some(), "临时身份仍持有代理（完整三件套）");
        assert_eq!(
            mgr.acquire("a.example.com", None, false).id,
            a.id,
            "已有绑定不受容量降级影响"
        );
    }

    /// 并发 acquire / mark_bad / mark_good / capture 无死锁
    /// （DashMap 同 shard 守卫纪律对齐 HostDiscipline 注释约定，超时显性化）
    #[tokio::test]
    async fn concurrent_acquire_and_mark_no_deadlock() {
        let mgr = Arc::new(make_manager(3.0, 100_000, 1800, 4096));
        let mut handles = Vec::new();
        for t in 0..8 {
            let m = Arc::clone(&mgr);
            handles.push(tokio::spawn(async move {
                for i in 0..50 {
                    let id = m.acquire("hot.example.com", Some("shared"), false);
                    if i % 3 == 0 {
                        m.mark_bad(&id);
                    } else {
                        m.mark_good(&id);
                    }
                    let mut h = HashMap::new();
                    h.insert("set-cookie".to_string(), format!("t{t}={i}"));
                    m.capture_cookies(&id, "hot.example.com", &h);
                    let _ = m.cookie_header(&id, "hot.example.com");
                }
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(handles))
            .await
            .expect("并发 session 操作不得死锁");
    }
}
