// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例登记簿仓储接口
//!
//! 登记簿是实例生命周期的唯一持久真相（DB source of truth）：采集器写入数据集
//! 元数据、探活器回填探测结果、状态机迁移实例状态。进程内热池只是这份数据的缓存。
//!
//! 本模块同时承载领域类型（`SearxngInstance` / `SearxngInstanceState` /
//! `SearxngInstanceSource`）与实例 URL 规范化 —— 三者都是「实例身份」的领域规则，
//! 不属于任何具体存储技术。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use url::Url;

use super::task_repository::RepositoryError;

/// 实例来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearxngInstanceSource {
    /// 由 searx.space 数据集采集进入（缺席数据集超过阈值会退役）
    SearxSpace,
    /// 由配置 `seed_urls` 手工引导进入（采集器永不因数据集缺席退役）
    Seed,
}

impl SearxngInstanceSource {
    /// 存储与 DTO 使用的规范字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            SearxngInstanceSource::SearxSpace => "searx_space",
            SearxngInstanceSource::Seed => "seed",
        }
    }
}

impl fmt::Display for SearxngInstanceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SearxngInstanceSource {
    type Err = InstanceUrlError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "searx_space" => Ok(SearxngInstanceSource::SearxSpace),
            "seed" => Ok(SearxngInstanceSource::Seed),
            other => Err(InstanceUrlError::Invalid(format!(
                "unknown instance source: {other}"
            ))),
        }
    }
}

/// 实例生命周期状态
///
/// 状态机（合法迁移由 [`SearxngInstanceState::can_transition_to`] 约束）：
///
/// ```text
/// candidate --probe ok--> active（入池）
/// active    --probe fail / 熔断开--> cooldown（逐出池）
/// cooldown  --probe ok--> active（回池）
/// cooldown  --连续探活失败达阈值--> retired
/// candidate --数据集缺席超阈值--> retired
/// retired   --重新出现在数据集且探活通过--> active
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearxngInstanceState {
    /// 候选：已准入登记但尚未通过探活
    Candidate,
    /// 活跃：探活通过，可进入热池
    Active,
    /// 冷却：失败逐出，等待恢复探测
    Cooldown,
    /// 退役：不再参与晋升（除非重新出现在数据集并探活通过）
    Retired,
}

impl SearxngInstanceState {
    /// 存储与 DTO 使用的规范字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            SearxngInstanceState::Candidate => "candidate",
            SearxngInstanceState::Active => "active",
            SearxngInstanceState::Cooldown => "cooldown",
            SearxngInstanceState::Retired => "retired",
        }
    }

    /// 是否为合法迁移（自身迁移视为幂等无操作，放行）
    ///
    /// 说明：`candidate → cooldown` 用于「候选首次探活即失败」的降级（例如启动期
    /// 重建对 seed 候选探测失败），与 `active → cooldown` 的逐出语义不同。
    pub fn can_transition_to(&self, to: SearxngInstanceState) -> bool {
        use SearxngInstanceState::*;
        if *self == to {
            return true;
        }
        matches!(
            (self, to),
            (Candidate, Active)
                | (Candidate, Cooldown)
                | (Candidate, Retired)
                | (Active, Cooldown)
                | (Cooldown, Active)
                | (Cooldown, Retired)
                | (Retired, Active)
        )
    }
}

impl fmt::Display for SearxngInstanceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SearxngInstanceState {
    type Err = InstanceUrlError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "candidate" => Ok(SearxngInstanceState::Candidate),
            "active" => Ok(SearxngInstanceState::Active),
            "cooldown" => Ok(SearxngInstanceState::Cooldown),
            "retired" => Ok(SearxngInstanceState::Retired),
            other => Err(InstanceUrlError::Invalid(format!(
                "unknown instance state: {other}"
            ))),
        }
    }
}

/// 实例 URL 规范化 / 解析错误
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstanceUrlError {
    /// 非法 URL（无法解析、缺 host、非 http(s)）
    #[error("invalid instance url: {0}")]
    Invalid(String),
    /// 指向内网/环回/链路本地地址（SSRF 面）
    #[error("unsafe instance url (internal address): {0}")]
    Unsafe(String),
}

/// 规范化实例 base URL：强制 https、host 小写、去尾斜杠。
///
/// `http://` 输入被升级为 `https://`（公共实例统一 HTTPS 面），其余 scheme 拒绝；
/// 命中 [`is_internal_url`](crate::infrastructure::security::ssrf::is_internal_url)
/// 的内网地址拒绝（SSRF 静态校验复用）。
pub fn normalize_instance_url(raw: &str) -> Result<String, InstanceUrlError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(InstanceUrlError::Invalid("empty url".to_string()));
    }
    let mut parsed =
        Url::parse(trimmed).map_err(|e| InstanceUrlError::Invalid(format!("{trimmed}: {e}")))?;
    match parsed.scheme() {
        "http" => {
            parsed.set_scheme("https").map_err(|_| {
                InstanceUrlError::Invalid(format!("{trimmed}: cannot upgrade to https"))
            })?;
        }
        "https" => {}
        other => {
            return Err(InstanceUrlError::Invalid(format!(
                "{trimmed}: unsupported scheme {other}"
            )));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| InstanceUrlError::Invalid(format!("{trimmed}: missing host")))?;
    if host.is_empty() {
        return Err(InstanceUrlError::Invalid(format!(
            "{trimmed}: missing host"
        )));
    }
    parsed
        .set_host(Some(&host.to_ascii_lowercase()))
        .map_err(|_| InstanceUrlError::Invalid(format!("{trimmed}: host cannot be normalized")))?;

    // 去尾斜杠（仅根路径；带路径的实例保留路径语义）
    let normalized = parsed.as_str().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err(InstanceUrlError::Invalid(format!(
            "{trimmed}: empty after normalize"
        )));
    }
    if crate::infrastructure::security::ssrf::is_internal_url(&normalized) {
        return Err(InstanceUrlError::Unsafe(normalized));
    }
    Ok(normalized)
}

/// 登记簿中的一条实例记录（领域视图）
#[derive(Debug, Clone, PartialEq)]
pub struct SearxngInstance {
    /// 规范化 base URL（唯一键）
    pub url: String,
    /// 来源
    pub source: SearxngInstanceSource,
    /// 生命周期状态
    pub state: SearxngInstanceState,
    /// 数据集元数据：网络类型（`normal` / `tor` / `ipv6` / ...）
    pub network_type: Option<String>,
    /// 数据集元数据：SearXNG 版本
    pub version: Option<String>,
    /// 数据集元数据：JSON API 评级
    pub api_grade: Option<String>,
    /// 数据集元数据：HTTP 评级
    pub http_grade: Option<String>,
    /// 数据集元数据：TLS 评级
    pub tls_grade: Option<String>,
    /// 数据集元数据：CDN 评级
    pub cdn_grade: Option<String>,
    /// 日在线率（百分比）
    pub uptime_day: Option<f64>,
    /// 月在线率（百分比）
    pub uptime_month: Option<f64>,
    /// 最近一次成功探活时延（毫秒）
    pub latency_ms: Option<i64>,
    /// 连续失败计数（探活/请求失败累计）
    pub consecutive_failures: i32,
    /// 最近成功时间
    pub last_success_at: Option<DateTime<Utc>>,
    /// 最近失败时间
    pub last_failure_at: Option<DateTime<Utc>>,
    /// 最近失败原因（截断存储）
    pub last_error: Option<String>,
    /// 连续缺席数据集的采集轮数（`searx_space` 来源退役依据）
    pub dataset_miss_count: i32,
    /// 入库时间
    pub created_at: DateTime<Utc>,
    /// 最近更新时间
    pub updated_at: DateTime<Utc>,
}

impl SearxngInstance {
    /// 熔断器键（实例专属，避免与 engines 指标口径混淆）
    pub fn breaker_key(&self) -> String {
        format!("searxng:{}", self.url)
    }
}

/// 数据集准入通过后待 upsert 的实例元数据
///
/// 由准入谓词（`searxng::dataset::admit`）产出，是 `upsert_from_dataset` 的载荷。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AdmittedInstance {
    /// 规范化 base URL
    pub url: String,
    /// 网络类型
    pub network_type: Option<String>,
    /// 版本
    pub version: Option<String>,
    /// JSON API 评级
    pub api_grade: Option<String>,
    /// HTTP 评级
    pub http_grade: Option<String>,
    /// TLS 评级
    pub tls_grade: Option<String>,
    /// CDN 评级
    pub cdn_grade: Option<String>,
    /// 日在线率
    pub uptime_day: Option<f64>,
    /// 月在线率
    pub uptime_month: Option<f64>,
}

/// upsert 结果（采集报告计数依据）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// 登记簿新增一行
    Inserted,
    /// 已有行被刷新
    Updated,
}

/// 采集轮互斥守卫
///
/// 跨进程（api / worker 双进程）防重：守卫存活期间持有 PG advisory lock 的会话，
/// 显式 [`release`](CollectLockGuard::release) 释放（`Drop` 不释放，避免隐式吞错）。
#[async_trait]
pub trait CollectLockGuard: Send + Sync {
    /// 释放锁（幂等）
    async fn release(&self) -> Result<(), RepositoryError>;
}

/// SearXNG 实例登记簿仓储
#[async_trait]
pub trait SearxngInstanceRepository: Send + Sync {
    /// 尝试获取采集轮互斥锁（跨进程，固定常量键）。
    ///
    /// 返回 `Ok(None)` 表示锁已被其他进程持有（本轮应跳过）；
    /// 返回 `Ok(Some(guard))` 时调用方必须在执行体结束后 `guard.release()`。
    async fn try_acquire_collect_lock(
        &self,
        key: i64,
    ) -> Result<Option<Box<dyn CollectLockGuard>>, RepositoryError>;

    /// 按 url 幂等写入数据集实例：新实例→`candidate`；已有→刷新元数据且
    /// `dataset_miss_count = 0`（重新出现在数据集视为回归，`retired` 回到 `candidate`）。
    async fn upsert_from_dataset(
        &self,
        instance: &AdmittedInstance,
    ) -> Result<UpsertOutcome, RepositoryError>;

    /// 写入手工引导实例（`source = seed`）：新→`candidate`；已有→置 `source = seed`
    /// （此后不再因数据集缺席退役）。
    async fn upsert_seed(&self, url: &str) -> Result<UpsertOutcome, RepositoryError>;

    /// 候选实例列表（晋升来源）：按 `uptime_day DESC, latency_ms ASC NULLS LAST`
    /// 排序，排除 `retired`。
    async fn list_candidates(&self, limit: u64) -> Result<Vec<SearxngInstance>, RepositoryError>;

    /// 指定状态的实例列表
    async fn list_by_state(
        &self,
        state: SearxngInstanceState,
        limit: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError>;

    /// 登记簿分页列表（管理 API 视图）：按 state（active 优先）+ uptime_day 降序
    async fn list_paginated(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError>;

    /// 回填一次探测结果：成功刷新时延与成功时间并清零失败计数；
    /// 失败累加失败计数并记录错误（截断存储）。
    async fn record_probe_result(
        &self,
        url: &str,
        ok: bool,
        latency_ms: Option<i64>,
        error: Option<&str>,
    ) -> Result<(), RepositoryError>;

    /// 状态迁移（校验合法迁移；非法迁移返回 `RepositoryError`）
    async fn transition_state(
        &self,
        url: &str,
        to: SearxngInstanceState,
    ) -> Result<(), RepositoryError>;

    /// 记录一轮「未出现在数据集」：`dataset_miss_count += 1`；
    /// `source = searx_space` 且计数超过 `retire_after` 时置 `retired`。
    /// 返回 `Ok(true)` 表示本轮触发了退役。
    async fn record_dataset_miss(
        &self,
        url: &str,
        retire_after: i32,
    ) -> Result<bool, RepositoryError>;

    /// 按 url 查询
    async fn get_by_url(&self, url: &str) -> Result<Option<SearxngInstance>, RepositoryError>;

    /// 指定状态的实例计数
    async fn count_by_state(&self, state: SearxngInstanceState) -> Result<u64, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== 状态枚举往返 ==========

    #[test]
    fn test_state_display_from_str_roundtrip() {
        for state in [
            SearxngInstanceState::Candidate,
            SearxngInstanceState::Active,
            SearxngInstanceState::Cooldown,
            SearxngInstanceState::Retired,
        ] {
            let text = state.to_string();
            let back: SearxngInstanceState = text.parse().expect("state roundtrip");
            assert_eq!(back, state, "roundtrip failed for {text}");
        }
    }

    #[test]
    fn test_state_as_str_canonical() {
        assert_eq!(SearxngInstanceState::Candidate.as_str(), "candidate");
        assert_eq!(SearxngInstanceState::Active.as_str(), "active");
        assert_eq!(SearxngInstanceState::Cooldown.as_str(), "cooldown");
        assert_eq!(SearxngInstanceState::Retired.as_str(), "retired");
    }

    #[test]
    fn test_state_from_str_rejects_unknown() {
        assert!(SearxngInstanceState::from_str("zombie").is_err());
    }

    #[test]
    fn test_source_roundtrip() {
        for source in [
            SearxngInstanceSource::SearxSpace,
            SearxngInstanceSource::Seed,
        ] {
            let back: SearxngInstanceSource = source.to_string().parse().expect("source roundtrip");
            assert_eq!(back, source);
        }
        assert!(SearxngInstanceSource::from_str("manual").is_err());
    }

    // ========== 状态机合法迁移 ==========

    #[test]
    fn test_legal_transitions() {
        use SearxngInstanceState::*;
        let legal = [
            (Candidate, Active),
            (Candidate, Cooldown),
            (Candidate, Retired),
            (Active, Cooldown),
            (Cooldown, Active),
            (Cooldown, Retired),
            (Retired, Active),
            // 幂等自迁移
            (Candidate, Candidate),
            (Active, Active),
            (Cooldown, Cooldown),
            (Retired, Retired),
        ];
        for (from, to) in legal {
            assert!(from.can_transition_to(to), "{from} -> {to} should be legal");
        }
    }

    #[test]
    fn test_illegal_transitions() {
        use SearxngInstanceState::*;
        let illegal = [
            (Retired, Candidate),
            (Active, Candidate),
            (Active, Retired),
            (Retired, Cooldown),
        ];
        for (from, to) in illegal {
            assert!(
                !from.can_transition_to(to),
                "{from} -> {to} should be illegal"
            );
        }
    }

    // ========== URL 规范化 ==========

    #[test]
    fn test_normalize_upgrades_http_and_lowercases_host() {
        assert_eq!(
            normalize_instance_url("HTTP://Example.com/").expect("normalize"),
            "https://example.com"
        );
    }

    #[test]
    fn test_normalize_strips_trailing_slash() {
        assert_eq!(
            normalize_instance_url("https://searx.example.org/").expect("normalize"),
            "https://searx.example.org"
        );
    }

    #[test]
    fn test_normalize_keeps_path_but_strips_trailing_slash() {
        assert_eq!(
            normalize_instance_url("https://EXAMPLE.com/searx/").expect("normalize"),
            "https://example.com/searx"
        );
    }

    #[test]
    fn test_normalize_keeps_port() {
        assert_eq!(
            normalize_instance_url("http://Search.Example.org:8080").expect("normalize"),
            "https://search.example.org:8080"
        );
    }

    #[test]
    fn test_normalize_rejects_internal_address() {
        let err = normalize_instance_url("http://127.0.0.1:8080/").expect_err("loopback rejected");
        assert!(matches!(err, InstanceUrlError::Unsafe(_)), "got: {err:?}");
    }

    #[test]
    fn test_normalize_rejects_private_hostname() {
        let err = normalize_instance_url("https://localhost:8888").expect_err("localhost rejected");
        assert!(matches!(err, InstanceUrlError::Unsafe(_)), "got: {err:?}");
    }

    #[test]
    fn test_normalize_rejects_non_http_scheme() {
        let err = normalize_instance_url("ftp://example.com").expect_err("ftp rejected");
        assert!(matches!(err, InstanceUrlError::Invalid(_)), "got: {err:?}");
    }

    #[test]
    fn test_normalize_rejects_garbage_and_empty() {
        assert!(normalize_instance_url("not-a-url").is_err());
        assert!(normalize_instance_url("   ").is_err());
        assert!(normalize_instance_url("https://").is_err());
    }

    // ========== 熔断键 ==========

    #[test]
    fn test_breaker_key_prefixes_url() {
        let instance = SearxngInstance {
            url: "https://searx.example.org".to_string(),
            source: SearxngInstanceSource::Seed,
            state: SearxngInstanceState::Candidate,
            network_type: None,
            version: None,
            api_grade: None,
            http_grade: None,
            tls_grade: None,
            cdn_grade: None,
            uptime_day: None,
            uptime_month: None,
            latency_ms: None,
            consecutive_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
            dataset_miss_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert_eq!(instance.breaker_key(), "searxng:https://searx.example.org");
    }
}
