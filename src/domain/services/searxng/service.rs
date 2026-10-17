// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 实例生命周期编排
//!
//! [`SearxngInstanceServiceImpl`] 负责：采集轮次、探活轮次、熔断记账、逐出与晋升、
//! 降级口径。所有外部依赖（登记簿仓储、HTTP 传输）都是注入的端口，测试用真实
//! trait impl 替身驱动（见 `test_support.rs`）。
//!
//! 并发模型：内部状态全部放在 `Arc` 内，`report(Failure)` 触发的晋升通过
//! `tokio::spawn` 异步执行 —— 失败请求路径不被探活阻塞；晋升本身幂等（池按 url 去重）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, error, info, warn};

use crate::config::SearxngSearchSettings;
use crate::domain::repositories::searxng_instance_repository::{
    normalize_instance_url, SearxngInstance, SearxngInstanceRepository, SearxngInstanceSource,
    SearxngInstanceState, UpsertOutcome,
};
use crate::domain::repositories::task_repository::RepositoryError;
use crate::engines::circuit_breaker::{CircuitBreaker, CircuitConfig};
use crate::search::types::EngineHealth;

use super::dataset::{admit, parse_dataset};
use super::probe::probe_instance;
use super::{
    BreakerMirror, CollectorReport, HealthReport, InstanceOutcome, InstancePool, PoolMember,
    PoolMemberState, PoolSnapshot, SearxngError, SearxngInstanceService, SearxngTransport,
};

/// 采集轮跨进程互斥锁的固定常量键（"searxng\0" 的 big-endian i64）
pub(crate) const COLLECT_ADVISORY_LOCK_KEY: i64 = 0x7365_6172_786E_6700;

/// 「不限制」分页上限：sqlx 把 limit 绑为 i64，直接用 u64::MAX 会 PosOverflow
const UNBOUNDED_LIMIT: u64 = i64::MAX as u64;

/// 熔断键前缀（实例专属，不与 engines 统计口径混用）
const BREAKER_KEY_PREFIX: &str = "searxng:";

/// 服务内部共享状态
struct SearxngInstanceInner {
    /// 登记簿仓储（DB 为唯一共享真相）
    repo: Arc<dyn SearxngInstanceRepository>,
    /// HTTP 传输端口（采集 / 探活 / 搜索共用）
    transport: Arc<dyn SearxngTransport>,
    /// `[search.searxng]` 配置快照
    settings: SearxngSearchSettings,
    /// 专属熔断器实例（不与 engines 共享，避免指标标签污染与阈值互相干扰）
    breaker: CircuitBreaker,
    /// 进程内热池
    pool: InstancePool,
    /// 启动期热池重建是否已完成（完成前 `health()` 报 Degraded）
    bootstrap_done: AtomicBool,
}

/// SearXNG 实例生命周期服务实现
#[derive(Clone)]
pub struct SearxngInstanceServiceImpl {
    inner: Arc<SearxngInstanceInner>,
}

impl SearxngInstanceServiceImpl {
    /// 构造服务（专属熔断器 + 目标为 `pool_target` 的空热池）
    pub fn new(
        repo: Arc<dyn SearxngInstanceRepository>,
        transport: Arc<dyn SearxngTransport>,
        settings: SearxngSearchSettings,
    ) -> Self {
        let breaker = CircuitBreaker::with_default_config(CircuitConfig {
            failure_threshold: settings.breaker.failure_threshold,
            recovery_timeout: Duration::from_secs(settings.breaker.recovery_timeout_seconds),
            ..CircuitConfig::default()
        });
        let pool = InstancePool::new(settings.pool_target);
        Self {
            inner: Arc::new(SearxngInstanceInner {
                repo,
                transport,
                settings,
                breaker,
                pool,
                bootstrap_done: AtomicBool::new(false),
            }),
        }
    }

    /// 配置快照（引擎侧读取超时 / UA 等）
    pub fn settings(&self) -> &SearxngSearchSettings {
        &self.inner.settings
    }

    /// 熔断键：`searxng:<url>`
    fn breaker_key(url: &str) -> String {
        format!("{BREAKER_KEY_PREFIX}{url}")
    }

    /// 从热池轮转取一个可用实例（跳过 `exclude` 与熔断开成员）
    pub fn acquire(&self, exclude: &[String]) -> Option<Arc<PoolMember>> {
        self.inner.pool.acquire(exclude, &|url| {
            !self.inner.breaker.is_open(&Self::breaker_key(url))
        })
    }

    /// 当前池成员数（测试与管理视图共用）
    pub fn pool_len(&self) -> usize {
        self.inner.pool.len()
    }

    /// 标记启动期重建完成（重建任务结束时调用）
    pub fn mark_bootstrap_done(&self) {
        self.inner.bootstrap_done.store(true, Ordering::Release);
    }

    /// 热池当前是否已满（达到 `pool_target`）
    pub fn pool_is_full(&self) -> bool {
        self.inner.pool.is_full()
    }

    /// 追加一个成员进池（启动期重建 / 晋升路径共用）
    fn add_member(&self, instance: SearxngInstance) -> bool {
        self.inner.pool.insert(Arc::new(PoolMember { instance }))
    }

    /// 引擎健康口径：池满 → Healthy；非空但不足 → Degraded（+warn）；空 → Unhealthy。
    /// 启动期重建完成前一律 Degraded（避免伪报可用）。
    pub fn health(&self) -> EngineHealth {
        if !self.inner.bootstrap_done.load(Ordering::Acquire) {
            return EngineHealth::Degraded;
        }
        let len = self.inner.pool.len();
        let target = self.inner.pool.target();
        if len >= target {
            EngineHealth::Healthy
        } else if len > 0 {
            warn!(
                "searxng pool degraded: {len}/{target} instances available \
                 (target is a control-loop goal, not a hard guarantee on third-party instances)"
            );
            EngineHealth::Degraded
        } else {
            EngineHealth::Unhealthy
        }
    }

    /// 管理视图：热池成员 + 熔断镜像态 + 健康口径
    pub async fn pool_snapshot(&self) -> PoolSnapshot {
        let members = self
            .inner
            .pool
            .snapshot()
            .iter()
            .map(|member| {
                let key = Self::breaker_key(member.url());
                let breaker = if self.inner.breaker.is_open(&key) {
                    BreakerMirror::Open
                } else if member.instance.state == SearxngInstanceState::Cooldown {
                    BreakerMirror::HalfOpen
                } else {
                    BreakerMirror::Closed
                };
                PoolMemberState {
                    url: member.instance.url.clone(),
                    state: member.instance.state,
                    latency_ms: member.instance.latency_ms,
                    breaker,
                }
            })
            .collect();
        PoolSnapshot {
            target: self.inner.pool.target(),
            members,
            health: self.health(),
        }
    }

    /// 管理视图：登记簿分页列表（active 优先 + uptime 降序）
    pub async fn list_instances(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, SearxngError> {
        self.inner
            .repo
            .list_paginated(limit, offset)
            .await
            .map_err(|e| SearxngError::Repository(e.to_string()))
    }

    /// 请求结果回填：熔断记账 + 时延/失败计数；失败达阈值时逐出并异步晋升替补。
    pub async fn report(&self, url: &str, outcome: InstanceOutcome) {
        let key = Self::breaker_key(url);
        match outcome {
            InstanceOutcome::Success { latency_ms } => {
                self.inner.breaker.record_success(&key).await;
                if let Err(e) = self
                    .inner
                    .repo
                    .record_probe_result(url, true, Some(latency_ms), None)
                    .await
                {
                    warn!("searxng success record failed url={url} error={e}");
                }
                // 刷新池内的时延快照（管理视图展示）
                if let Some(member) = self
                    .inner
                    .pool
                    .snapshot()
                    .into_iter()
                    .find(|m| m.url() == url)
                {
                    let mut instance = member.instance.clone();
                    instance.latency_ms = Some(latency_ms);
                    instance.consecutive_failures = 0;
                    instance.last_success_at = Some(chrono::Utc::now());
                    self.inner.pool.replace(Arc::new(PoolMember { instance }));
                }
            }
            InstanceOutcome::Failure { error } => {
                self.inner.breaker.record_failure(&key).await;
                let message = error.to_string();
                if let Err(e) = self
                    .inner
                    .repo
                    .record_probe_result(url, false, None, Some(&message))
                    .await
                {
                    warn!("searxng failure record failed url={url} error={e}");
                }

                let failures = match self.inner.repo.get_by_url(url).await {
                    Ok(Some(instance)) => instance.consecutive_failures,
                    Ok(None) => {
                        debug!("searxng reported failure for unknown instance url={url}");
                        return;
                    }
                    Err(e) => {
                        warn!("searxng failure count read failed url={url} error={e}");
                        return;
                    }
                };

                // 熔断开 或 连续失败 ≥2 → 逐出热池 + 转 cooldown + 异步晋升
                if (self.inner.breaker.is_open(&key) || failures >= 2)
                    && self.inner.pool.evict(url).is_some()
                {
                    if let Err(e) = self
                        .inner
                        .repo
                        .transition_state(url, SearxngInstanceState::Cooldown)
                        .await
                    {
                        warn!("searxng cooldown transition failed url={url} error={e}");
                    }
                    let this = self.clone();
                    tokio::spawn(async move {
                        this.promote_one().await;
                    });
                }
            }
        }
    }

    /// 采集一轮：拉数据集 → 宽容解析 → 准入 → 幂等 upsert → 缺席退役。
    ///
    /// 跨进程防重：执行体由 PG advisory lock 包裹（固定常量键）。拿不到锁时
    /// 本轮跳过（`CollectorReport::skipped()` + `debug!`），不重复采集。
    ///
    /// 失败语义：数据集拉取失败返回 `Err`（含原因）且**登记簿零变更**；
    /// 单条准入拒绝/写入失败只累计 `errors`，不中断整轮（其余实例继续处理）。
    pub async fn collect(&self) -> Result<CollectorReport, SearxngError> {
        let guard = match self
            .inner
            .repo
            .try_acquire_collect_lock(COLLECT_ADVISORY_LOCK_KEY)
            .await
        {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                debug!("searxng collect skipped: advisory lock held by another process");
                return Ok(CollectorReport::skipped());
            }
            Err(e) => {
                warn!("searxng collect lock acquisition failed: {e}");
                return Err(SearxngError::Repository(e.to_string()));
            }
        };

        let result = self.collect_inner().await;
        if let Err(e) = guard.release().await {
            warn!("searxng collect lock release failed: {e}");
        }
        result
    }

    /// 采集执行体（调用方必须已持有采集锁）
    async fn collect_inner(&self) -> Result<CollectorReport, SearxngError> {
        let collector = &self.inner.settings.collector;
        let timeout = Duration::from_secs(collector.request_timeout_seconds);

        let (status, payload) = self
            .inner
            .transport
            .get_json(&collector.data_url, timeout)
            .await
            .map_err(|e| {
                error!(
                    "searxng dataset fetch failed url={} class={} error={}",
                    collector.data_url,
                    e.class(),
                    e
                );
                SearxngError::DatasetFetch(format!("{}: {e}", collector.data_url))
            })?;

        if !(200..300).contains(&status) {
            let err = SearxngError::DatasetFetch(format!(
                "{} returned HTTP {status}",
                collector.data_url
            ));
            error!("{err}");
            return Err(err);
        }

        let dataset = parse_dataset(&payload).map_err(|e| {
            error!("searxng dataset parse failed: {e}");
            SearxngError::DatasetFetch(e.to_string())
        })?;

        let mut report = CollectorReport {
            fetched: dataset.instances.len(),
            ..Default::default()
        };

        // 数据集中「出现过」的规范化 url 集合：本轮的缺席判定依据。
        // 用原始键（含未通过准入的条目）：元数据不再满足准入 ≠ 已从数据集消失，
        // 不应因此累计缺席计数。
        let mut present: HashSet<String> = HashSet::with_capacity(dataset.instances.len());

        for (raw_url, entry) in &dataset.instances {
            if let Ok(url) = normalize_instance_url(raw_url) {
                present.insert(url);
            }

            let admitted = match admit(raw_url, entry, collector.min_uptime_day) {
                Ok(admitted) => admitted,
                Err(reject) => {
                    report.errors += 1;
                    debug!("searxng admission rejected: {reject}");
                    continue;
                }
            };
            report.seen += 1;

            match self.inner.repo.upsert_from_dataset(&admitted).await {
                Ok(UpsertOutcome::Inserted) => report.admitted_new += 1,
                Ok(UpsertOutcome::Updated) => report.updated += 1,
                Err(e) => {
                    report.errors += 1;
                    warn!("searxng upsert failed url={} error={e}", admitted.url);
                }
            }
        }

        // 缺席退役：既有 searx_space 实例未出现在本轮数据集 → 累计缺席；
        // seed 来源永不退役（仓储侧豁免）。
        let existing = self
            .inner
            .repo
            .list_paginated(UNBOUNDED_LIMIT, 0)
            .await
            .map_err(|e| {
                error!("searxng registry listing failed: {e}");
                SearxngError::Repository(e.to_string())
            })?;
        for instance in existing {
            if instance.source != SearxngInstanceSource::SearxSpace {
                continue;
            }
            if present.contains(&instance.url) {
                continue;
            }
            match self
                .inner
                .repo
                .record_dataset_miss(&instance.url, collector.retire_miss_cycles)
                .await
            {
                Ok(true) => {
                    report.retired += 1;
                    info!(
                        "searxng instance retired for dataset absence url={}",
                        instance.url
                    );
                }
                Ok(false) => {}
                Err(RepositoryError::NotFound) => {
                    debug!(
                        "searxng instance vanished during collection url={}",
                        instance.url
                    );
                }
                Err(e) => {
                    report.errors += 1;
                    warn!(
                        "searxng dataset-miss record failed url={} error={e}",
                        instance.url
                    );
                }
            }
        }

        info!(
            "searxng collect finished fetched={} seen={} admitted_new={} updated={} retired={} errors={}",
            report.fetched,
            report.seen,
            report.admitted_new,
            report.updated,
            report.retired,
            report.errors
        );
        Ok(report)
    }

    /// 探活一轮：热池全体 + 冷却期满的 cooldown + 前 `candidate_batch` 个候选。
    ///
    /// 状态机迁移由 [`Self::apply_probe_outcome`] 统一承载（探活通过 → active 入池；
    /// 失败 → cooldown 逐出；连续失败达 `failure_retire_threshold` → retired）。
    pub async fn health_cycle(&self) -> HealthReport {
        let mut report = HealthReport::default();
        let batch = self.inner.settings.health.candidate_batch.max(1);

        // 1) 热池全体
        for member in self.inner.pool.snapshot() {
            self.probe_and_apply(&member.instance, &mut report).await;
        }

        // 2) cooldown 冷却期满（熔断镜像已放行）→ 回池探测
        match self
            .inner
            .repo
            .list_by_state(SearxngInstanceState::Cooldown, batch)
            .await
        {
            Ok(cooldowns) => {
                for instance in cooldowns {
                    if self.inner.pool.contains(&instance.url) {
                        continue;
                    }
                    if self
                        .inner
                        .breaker
                        .is_open(&Self::breaker_key(&instance.url))
                    {
                        continue;
                    }
                    self.probe_and_apply(&instance, &mut report).await;
                }
            }
            Err(e) => {
                warn!("searxng cooldown listing failed: {e}");
                report.errors += 1;
            }
        }

        // 3) 候选补齐（按 uptime 降序、latency 升序，池满即停）
        match self.inner.repo.list_candidates(batch).await {
            Ok(candidates) => {
                for instance in candidates {
                    if self.inner.pool.is_full() {
                        break;
                    }
                    if self.inner.pool.contains(&instance.url) {
                        continue;
                    }
                    self.probe_and_apply(&instance, &mut report).await;
                }
            }
            Err(e) => {
                warn!("searxng candidate listing failed: {e}");
                report.errors += 1;
            }
        }

        info!(
            "searxng health cycle finished probed={} promoted={} demoted={} retired={} errors={} pool={}/{}",
            report.probed,
            report.promoted,
            report.demoted,
            report.retired,
            report.errors,
            self.inner.pool.len(),
            self.inner.pool.target()
        );
        report
    }

    /// 异步晋升一个替补：`list_candidates` → 探测一次 → 通过则 active 入池。
    ///
    /// 幂等：池按 url 去重，多路并发晋升最多补满 target。
    async fn promote_one(&self) {
        if self.inner.pool.is_full() {
            return;
        }
        let candidates = match self.inner.repo.list_candidates(4).await {
            Ok(candidates) => candidates,
            Err(e) => {
                warn!("searxng promotion candidate listing failed: {e}");
                return;
            }
        };
        for candidate in candidates {
            if self.inner.pool.is_full() {
                break;
            }
            if self.inner.pool.contains(&candidate.url) {
                continue;
            }
            let mut report = HealthReport::default();
            self.probe_and_apply(&candidate, &mut report).await;
            if report.promoted > 0 {
                break;
            }
        }
    }

    /// 探测一个实例并按状态机落库/入池/逐出。
    async fn probe_and_apply(&self, instance: &SearxngInstance, report: &mut HealthReport) {
        report.probed += 1;
        let outcome = probe_instance(
            self.inner.transport.as_ref(),
            instance,
            &self.inner.settings.health,
        )
        .await;
        let key = Self::breaker_key(&instance.url);

        if outcome.alive {
            self.inner.breaker.record_success(&key).await;
            if let Err(e) = self
                .inner
                .repo
                .record_probe_result(&instance.url, true, outcome.latency_ms, None)
                .await
            {
                warn!(
                    "searxng probe success record failed url={} error={e}",
                    instance.url
                );
            }

            let mut refreshed = instance.clone();
            refreshed.latency_ms = outcome.latency_ms;
            refreshed.consecutive_failures = 0;
            refreshed.last_success_at = Some(chrono::Utc::now());

            if refreshed.state != SearxngInstanceState::Active {
                if let Err(e) = self
                    .inner
                    .repo
                    .transition_state(&refreshed.url, SearxngInstanceState::Active)
                    .await
                {
                    warn!(
                        "searxng active transition failed url={} error={e}",
                        refreshed.url
                    );
                    return;
                }
                refreshed.state = SearxngInstanceState::Active;
            }

            if !self.inner.pool.contains(&refreshed.url) {
                if self.add_member(refreshed.clone()) {
                    report.promoted += 1;
                    info!("searxng instance promoted into pool url={}", refreshed.url);
                }
            } else {
                self.inner.pool.replace(Arc::new(PoolMember {
                    instance: refreshed,
                }));
            }
            return;
        }

        // 失败路径
        report.errors += 1;
        let message = outcome.error.unwrap_or_else(|| "probe failed".to_string());
        self.inner.breaker.record_failure(&key).await;
        if let Err(e) = self
            .inner
            .repo
            .record_probe_result(&instance.url, false, None, Some(&message))
            .await
        {
            warn!(
                "searxng probe failure record failed url={} error={e}",
                instance.url
            );
        }

        let failures = self
            .inner
            .repo
            .get_by_url(&instance.url)
            .await
            .ok()
            .flatten()
            .map(|i| i.consecutive_failures)
            .unwrap_or(instance.consecutive_failures + 1);

        let evicted = self.inner.pool.evict(&instance.url).is_some();
        if evicted {
            report.demoted += 1;
        }

        // 连续失败达阈值 → 退役（cooldown/candidate → retired；active 不允许直迁，
        // 先落 cooldown，下一轮由 cooldown → retired 完成）
        let retire = failures >= self.inner.settings.health.failure_retire_threshold;
        let target_state = if retire
            && instance
                .state
                .can_transition_to(SearxngInstanceState::Retired)
        {
            report.retired += 1;
            SearxngInstanceState::Retired
        } else {
            SearxngInstanceState::Cooldown
        };
        if instance.state != target_state && instance.state.can_transition_to(target_state) {
            if let Err(e) = self
                .inner
                .repo
                .transition_state(&instance.url, target_state)
                .await
            {
                warn!(
                    "searxng state transition failed url={} to={} error={e}",
                    instance.url, target_state
                );
            }
        }
        debug!(
            "searxng probe failed url={} failures={} state={} error={message}",
            instance.url, failures, target_state
        );
    }

    /// 启动期引导：`seed_urls` 逐条 upsert + 后台重建热池（不阻塞 API 就绪）。
    ///
    /// 重建完成前 [`Self::health`] 返回 `Degraded`（不伪报可用）。
    pub fn spawn_bootstrap(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            let report = this.bootstrap().await;
            info!(
                "searxng bootstrap finished probed={} promoted={} demoted={} errors={} pool={}/{} healthy_bootstrapped=true",
                report.probed,
                report.promoted,
                report.demoted,
                report.errors,
                this.inner.pool.len(),
                this.inner.pool.target()
            );
        });
    }

    /// 启动期重建（可等待版本，测试直接调用）：
    ///
    /// 1. `seed_urls` 逐条 `upsert_seed`（非法 URL 记 warn 跳过，不阻断启动）；
    /// 2. 取 `active` 与高分 `candidate` 逐个探活，通过者入池至 `pool_target`；
    ///    失败者按状态机落 `cooldown`（或达阈值转 `retired`）；
    /// 3. 标记重建完成（此后 `health()` 按三档口径返回）。
    pub async fn bootstrap(&self) -> HealthReport {
        for raw in &self.inner.settings.seed_urls {
            match normalize_instance_url(raw) {
                Ok(url) => match self.inner.repo.upsert_seed(&url).await {
                    Ok(outcome) => {
                        info!("searxng seed upserted url={url} outcome={outcome:?}")
                    }
                    Err(e) => warn!("searxng seed upsert failed url={url} error={e}"),
                },
                Err(e) => warn!("searxng seed url rejected raw={raw} error={e}"),
            }
        }

        let mut report = HealthReport::default();
        let limit = (self.inner.pool.target().max(1) * 2) as u64;
        let mut seeds: Vec<SearxngInstance> = Vec::new();
        match self
            .inner
            .repo
            .list_by_state(SearxngInstanceState::Active, limit)
            .await
        {
            Ok(instances) => seeds.extend(instances),
            Err(e) => {
                warn!("searxng bootstrap active listing failed: {e}");
                report.errors += 1;
            }
        }
        match self.inner.repo.list_candidates(limit).await {
            Ok(instances) => seeds.extend(instances),
            Err(e) => {
                warn!("searxng bootstrap candidate listing failed: {e}");
                report.errors += 1;
            }
        }

        for instance in seeds {
            if self.inner.pool.is_full() {
                break;
            }
            if self.inner.pool.contains(&instance.url) {
                continue;
            }
            self.probe_and_apply(&instance, &mut report).await;
        }

        self.mark_bootstrap_done();
        report
    }
}

#[async_trait::async_trait]
impl SearxngInstanceService for SearxngInstanceServiceImpl {
    fn acquire(&self, exclude: &[String]) -> Option<Arc<PoolMember>> {
        SearxngInstanceServiceImpl::acquire(self, exclude)
    }

    async fn report(&self, url: &str, outcome: InstanceOutcome) {
        SearxngInstanceServiceImpl::report(self, url, outcome).await
    }

    async fn collect(&self) -> Result<CollectorReport, SearxngError> {
        SearxngInstanceServiceImpl::collect(self).await
    }

    async fn health_cycle(&self) -> HealthReport {
        SearxngInstanceServiceImpl::health_cycle(self).await
    }

    async fn pool_snapshot(&self) -> PoolSnapshot {
        SearxngInstanceServiceImpl::pool_snapshot(self).await
    }

    async fn list_instances(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, SearxngError> {
        SearxngInstanceServiceImpl::list_instances(self, limit, offset).await
    }

    fn health(&self) -> EngineHealth {
        SearxngInstanceServiceImpl::health(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::services::searxng::test_support::{
        shared_repo, test_settings, InMemorySearxngRepo, Scripted, ScriptedTransport,
    };

    fn fixture_payload() -> serde_json::Value {
        let raw = include_str!("../../../../tests/fixtures/searxng/instances_sample.json");
        serde_json::from_str(raw).expect("fixture json")
    }

    fn service(
        repo: Arc<InMemorySearxngRepo>,
        transport: Arc<ScriptedTransport>,
    ) -> SearxngInstanceServiceImpl {
        SearxngInstanceServiceImpl::new(repo, transport, test_settings())
    }

    /// 构造一条 active 实例并放入池（模拟已跑起的热池）
    fn seed_pool(svc: &SearxngInstanceServiceImpl, repo: &Arc<InMemorySearxngRepo>, url: &str) {
        let instance = InMemorySearxngRepo::seed_instance(url, SearxngInstanceState::Active);
        repo.insert_raw(instance.clone());
        svc.add_member(instance);
    }

    fn alive_body() -> serde_json::Value {
        serde_json::json!({ "results": [] })
    }

    // ========== 采集（T006/T007 回归） ==========

    #[tokio::test]
    async fn test_collect_counts_match_registry() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: fixture_payload(),
        }]));
        let svc = service(repo.clone(), transport.clone());

        let report = svc.collect().await.expect("collect succeeds");

        // fixture 10 条：通过准入的是 working / api-detail / minimal / unknown-fields /
        // bad-metadata（5 条），其余被拒（api-off / http / low uptime / tor / 内网）。
        assert_eq!(report.fetched, 10);
        assert_eq!(report.seen, 5);
        assert_eq!(report.admitted_new, 5);
        assert_eq!(report.updated, 0);
        assert_eq!(report.retired, 0);
        assert_eq!(report.errors, 5);
        assert_eq!(repo.len(), 5);
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_collect_second_round_updates_not_inserts() {
        let repo = shared_repo();
        let payload = fixture_payload();
        let transport = Arc::new(ScriptedTransport::new(vec![
            Scripted::Json {
                status: 200,
                body: payload.clone(),
            },
            Scripted::Json {
                status: 200,
                body: payload,
            },
        ]));
        let svc = service(repo.clone(), transport);

        svc.collect().await.expect("first round");
        let second = svc.collect().await.expect("second round");

        assert_eq!(second.admitted_new, 0);
        assert_eq!(second.updated, 5);
        assert_eq!(repo.len(), 5, "idempotent upsert must not duplicate rows");
    }

    #[tokio::test]
    async fn test_collect_fetch_failure_makes_zero_repo_calls() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Network(
            "connection refused".to_string(),
        )]));
        let svc = service(repo.clone(), transport);

        let err = svc.collect().await.expect_err("fetch failure must surface");
        assert!(matches!(err, SearxngError::DatasetFetch(_)), "got: {err}");
        assert!(err.to_string().contains("connection refused"));
        assert_eq!(
            repo.dataset_upserts
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "registry must stay untouched when the dataset is unreachable"
        );
        assert!(repo.is_empty());
    }

    #[tokio::test]
    async fn test_collect_non_2xx_dataset_returns_err() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 503,
            body: serde_json::Value::Null,
        }]));
        let svc = service(repo, transport);

        let err = svc.collect().await.expect_err("503 must surface");
        assert!(matches!(err, SearxngError::DatasetFetch(_)));
        assert!(err.to_string().contains("503"));
    }

    #[tokio::test]
    async fn test_collect_network_error_is_structured() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Timeout]));
        let svc = service(repo, transport);

        let err = svc.collect().await.expect_err("timeout must surface");
        assert!(matches!(err, SearxngError::DatasetFetch(_)));
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_collect_retires_absent_dataset_instance_but_not_seed() {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::seed_instance(
            "https://searx.seed.example",
            SearxngInstanceState::Active,
        ));
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://searx.absent.example",
            SearxngInstanceState::Active,
        ));

        let mut settings = test_settings();
        settings.collector.retire_miss_cycles = 0; // 首次缺席即退役
        let transport = Arc::new(ScriptedTransport::new(vec![
            Scripted::Json {
                status: 200,
                body: serde_json::json!({ "instances": {} }),
            },
            Scripted::Json {
                status: 200,
                body: serde_json::json!({ "instances": {} }),
            },
        ]));
        let svc = SearxngInstanceServiceImpl::new(repo.clone(), transport, settings);

        // 第一次：active 不允许直迁 retired → 落 cooldown，不计数退役
        let first = svc.collect().await.expect("first collect");
        assert_eq!(first.retired, 0);
        assert_eq!(
            repo.get("https://searx.absent.example").expect("row").state,
            SearxngInstanceState::Cooldown
        );

        // 第二次：cooldown → retired
        let second = svc.collect().await.expect("second collect");
        assert_eq!(second.retired, 1);
        assert_eq!(
            repo.get("https://searx.absent.example").expect("row").state,
            SearxngInstanceState::Retired
        );
        assert_eq!(
            repo.get("https://searx.seed.example").expect("row").state,
            SearxngInstanceState::Active
        );
    }

    #[tokio::test]
    async fn test_collect_rejects_malformed_payload_without_panicking() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!("not-an-object"),
        }]));
        let svc = service(repo.clone(), transport);

        let err = svc
            .collect()
            .await
            .expect_err("malformed payload must surface");
        assert!(matches!(err, SearxngError::DatasetFetch(_)));
        assert!(repo.is_empty());
    }

    #[tokio::test]
    async fn test_collect_skips_when_lock_is_held() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let svc = service(repo.clone(), transport.clone());

        let guard = repo
            .try_acquire_collect_lock(COLLECT_ADVISORY_LOCK_KEY)
            .await
            .expect("lock attempt")
            .expect("lock is free");

        let report = svc.collect().await.expect("skip is not an error");
        assert!(report.skipped, "held lock must make the round skip");
        assert_eq!(report.fetched, 0);
        assert_eq!(
            transport.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a skipped round must not touch the dataset"
        );

        guard.release().await.expect("release");

        let transport2 = Arc::new(ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({ "instances": {} }),
        }]));
        let svc2 = service(repo, transport2);
        let report2 = svc2.collect().await.expect("run after release");
        assert!(!report2.skipped);
    }

    #[tokio::test]
    async fn test_collect_releases_lock_after_failure() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![Scripted::Network(
            "boom".to_string(),
        )]));
        let svc = service(repo.clone(), transport);

        let _ = svc.collect().await.expect_err("fetch failure");

        let guard = repo
            .try_acquire_collect_lock(COLLECT_ADVISORY_LOCK_KEY)
            .await
            .expect("lock attempt")
            .expect("lock must be free after a failed round");
        guard.release().await.expect("release");
    }

    #[test]
    fn test_collector_report_skipped_shape() {
        let report = CollectorReport::skipped();
        assert!(report.skipped);
        assert_eq!(report.fetched, 0);
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn test_settings_snapshot_is_accessible() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let svc = service(repo, transport);
        assert_eq!(svc.settings().pool_target, 3);
    }

    // ========== 健康口径（T010） ==========

    #[test]
    fn test_health_is_degraded_before_bootstrap_completes() {
        let svc = service(shared_repo(), Arc::new(ScriptedTransport::new(vec![])));
        assert_eq!(svc.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_health_three_tiers() {
        let repo = shared_repo();
        let svc = service(repo.clone(), Arc::new(ScriptedTransport::new(vec![])));
        svc.mark_bootstrap_done();

        // 空池 → Unhealthy
        assert_eq!(svc.health(), EngineHealth::Unhealthy);

        // 1/3 → Degraded
        seed_pool(&svc, &repo, "https://a.example");
        assert_eq!(svc.health(), EngineHealth::Degraded);

        // 3/3 → Healthy
        seed_pool(&svc, &repo, "https://b.example");
        seed_pool(&svc, &repo, "https://c.example");
        assert_eq!(svc.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_engine_health_is_available_semantics() {
        assert!(EngineHealth::Healthy.is_available());
        assert!(EngineHealth::Degraded.is_available());
        assert!(!EngineHealth::Unhealthy.is_available());
    }

    // ========== 熔断与晋升（T010） ==========

    #[tokio::test]
    async fn test_repeated_failures_evict_member_and_promote_candidate() {
        let repo = shared_repo();
        // 池内 1 个成员 + 1 个候补（候补探测返回存活）
        let svc = service(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 200,
                    body: alive_body(),
                },
                10,
            )),
        );
        svc.mark_bootstrap_done();
        seed_pool(&svc, &repo, "https://failing.example");
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://healthy.example",
            SearxngInstanceState::Candidate,
        ));

        // 连续失败 3 次（默认 failure_threshold=3）→ 熔断开 + 逐出
        for _ in 0..3 {
            svc.report(
                "https://failing.example",
                InstanceOutcome::Failure {
                    error: SearxngError::RateLimited,
                },
            )
            .await;
        }

        assert_eq!(
            repo.get("https://failing.example").expect("row").state,
            SearxngInstanceState::Cooldown
        );

        // 异步晋升：候补被探测通过后入池（轮询等待，避免假阴性）
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            if svc.acquire(&[]).map(|m| m.url().to_string()).as_deref()
                == Some("https://healthy.example")
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "candidate should be promoted into the pool"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // 已逐出成员不再被 acquire 返回
        let acquired = svc.acquire(&[]).expect("member");
        assert_ne!(acquired.url(), "https://failing.example");
        assert_eq!(svc.pool_len(), 1);
        assert_eq!(svc.health(), EngineHealth::Degraded);
    }

    #[tokio::test]
    async fn test_success_report_resets_failures_and_updates_latency() {
        let repo = shared_repo();
        let svc = service(repo.clone(), Arc::new(ScriptedTransport::new(vec![])));
        svc.mark_bootstrap_done();
        seed_pool(&svc, &repo, "https://a.example");

        svc.report(
            "https://a.example",
            InstanceOutcome::Failure {
                error: SearxngError::BadHttpStatus(500),
            },
        )
        .await;
        assert_eq!(
            repo.get("https://a.example")
                .expect("row")
                .consecutive_failures,
            1
        );
        // 失败 1 次未达逐出阈值（≥2）
        assert_eq!(svc.pool_len(), 1);

        svc.report(
            "https://a.example",
            InstanceOutcome::Success { latency_ms: 42 },
        )
        .await;
        let row = repo.get("https://a.example").expect("row");
        assert_eq!(row.consecutive_failures, 0);
        assert_eq!(row.latency_ms, Some(42));
        let snapshot = svc.pool_snapshot().await;
        assert_eq!(snapshot.members[0].latency_ms, Some(42));
    }

    #[tokio::test]
    async fn test_report_for_unknown_instance_is_ignored() {
        let repo = shared_repo();
        let svc = service(repo.clone(), Arc::new(ScriptedTransport::new(vec![])));
        svc.report(
            "https://unknown.example",
            InstanceOutcome::Failure {
                error: SearxngError::Network("boom".to_string()),
            },
        )
        .await;
        assert_eq!(svc.pool_len(), 0);
    }

    // ========== 探活轮次（T010） ==========

    #[tokio::test]
    async fn test_health_cycle_promotes_candidates_and_respects_target() {
        let repo = shared_repo();
        for i in 0..5 {
            repo.insert_raw(InMemorySearxngRepo::dataset_instance(
                &format!("https://candidate-{i}.example"),
                SearxngInstanceState::Candidate,
            ));
        }
        let svc = service(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 200,
                    body: alive_body(),
                },
                50,
            )),
        );
        svc.mark_bootstrap_done();

        let report = svc.health_cycle().await;

        assert_eq!(svc.pool_len(), 3, "pool must stop at pool_target");
        assert_eq!(report.promoted, 3);
        assert_eq!(
            repo.count_by_state(SearxngInstanceState::Active)
                .await
                .expect("count"),
            3
        );
        assert_eq!(svc.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_health_cycle_retires_candidate_after_threshold_failures() {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://bad.example",
            SearxngInstanceState::Candidate,
        ));
        let mut settings = test_settings();
        settings.health.failure_retire_threshold = 1;
        let svc = SearxngInstanceServiceImpl::new(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 200,
                    body: serde_json::json!({ "no_results": true }),
                },
                10,
            )),
            settings,
        );
        svc.mark_bootstrap_done();

        let report = svc.health_cycle().await;

        assert_eq!(report.errors, 1);
        assert_eq!(report.retired, 1);
        assert_eq!(
            repo.get("https://bad.example").expect("row").state,
            SearxngInstanceState::Retired
        );
        assert_eq!(svc.pool_len(), 0);
        assert_eq!(svc.health(), EngineHealth::Unhealthy);
    }

    #[tokio::test]
    async fn test_health_cycle_demotes_failed_pool_member_to_cooldown() {
        let repo = shared_repo();
        let svc = service(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 429,
                    body: serde_json::Value::Null,
                },
                10,
            )),
        );
        svc.mark_bootstrap_done();
        seed_pool(&svc, &repo, "https://throttled.example");

        let report = svc.health_cycle().await;

        assert_eq!(report.demoted, 1);
        assert_eq!(
            repo.get("https://throttled.example").expect("row").state,
            SearxngInstanceState::Cooldown
        );
        assert_eq!(svc.pool_len(), 0);
    }

    #[tokio::test]
    async fn test_health_cycle_revives_cooldown_when_probe_passes() {
        let repo = shared_repo();
        repo.insert_raw(InMemorySearxngRepo::dataset_instance(
            "https://recovered.example",
            SearxngInstanceState::Cooldown,
        ));
        let svc = service(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 200,
                    body: alive_body(),
                },
                5,
            )),
        );
        svc.mark_bootstrap_done();

        let report = svc.health_cycle().await;

        assert_eq!(report.promoted, 1);
        assert_eq!(
            repo.get("https://recovered.example").expect("row").state,
            SearxngInstanceState::Active
        );
        assert_eq!(svc.pool_len(), 1);
    }

    // ========== 启动期重建与 seed 引导（T011） ==========

    #[tokio::test]
    async fn test_bootstrap_promotes_seed_that_passes_probe() {
        let repo = shared_repo();
        let mut settings = test_settings();
        settings.seed_urls = vec!["https://seed.example/".to_string()];
        settings.pool_target = 3;
        let svc = SearxngInstanceServiceImpl::new(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 200,
                    body: alive_body(),
                },
                5,
            )),
            settings,
        );

        // 重建前：Degraded（不伪报可用）
        assert_eq!(svc.health(), EngineHealth::Degraded);

        let report = svc.bootstrap().await;

        assert_eq!(report.promoted, 1);
        assert_eq!(svc.pool_len(), 1);
        let acquired = svc.acquire(&[]).expect("seed in pool");
        assert_eq!(acquired.url(), "https://seed.example");
        assert_eq!(
            repo.get("https://seed.example").expect("row").state,
            SearxngInstanceState::Active
        );
    }

    #[tokio::test]
    async fn test_bootstrap_keeps_failing_seed_out_of_pool_and_cooldowns_it() {
        let repo = shared_repo();
        let mut settings = test_settings();
        settings.seed_urls = vec!["https://dead-seed.example".to_string()];
        let svc = SearxngInstanceServiceImpl::new(
            repo.clone(),
            Arc::new(ScriptedTransport::repeat(
                Scripted::Json {
                    status: 503,
                    body: serde_json::Value::Null,
                },
                5,
            )),
            settings,
        );

        let report = svc.bootstrap().await;

        assert_eq!(report.promoted, 0);
        assert_eq!(report.errors, 1);
        assert_eq!(svc.pool_len(), 0);
        assert_eq!(
            repo.get("https://dead-seed.example").expect("row").state,
            SearxngInstanceState::Cooldown
        );
        // 重建已完成：空池按三档口径为 Unhealthy
        assert_eq!(svc.health(), EngineHealth::Unhealthy);
    }

    #[tokio::test]
    async fn test_bootstrap_skips_invalid_seed_url_without_blocking() {
        let repo = shared_repo();
        let mut settings = test_settings();
        settings.seed_urls = vec!["http://127.0.0.1:9000".to_string(), "not-a-url".to_string()];
        let svc = SearxngInstanceServiceImpl::new(
            repo.clone(),
            Arc::new(ScriptedTransport::new(vec![])),
            settings,
        );

        let report = svc.bootstrap().await;

        assert_eq!(report.promoted, 0);
        assert!(
            repo.is_empty(),
            "invalid seed urls must not reach the registry"
        );
    }

    #[tokio::test]
    async fn test_trait_object_disabled_shape_is_option_none() {
        // disabled 语义：服务不构造 → Option<Arc<dyn SearxngInstanceService>> 为 None
        // （由 bootstrap/services.rs 装配层断言；这里锁定 trait 可装箱为 trait object）
        let svc: Arc<dyn SearxngInstanceService> = Arc::new(service(
            shared_repo(),
            Arc::new(ScriptedTransport::new(vec![])),
        ));
        assert!(svc.acquire(&[]).is_none());
        let snapshot = svc.pool_snapshot().await;
        assert_eq!(snapshot.target, 3);
        assert!(snapshot.members.is_empty());
    }

    #[tokio::test]
    async fn test_pool_snapshot_reports_breaker_mirror_and_health() {
        let repo = shared_repo();
        let svc = service(repo.clone(), Arc::new(ScriptedTransport::new(vec![])));
        svc.mark_bootstrap_done();
        seed_pool(&svc, &repo, "https://a.example");

        let snapshot = svc.pool_snapshot().await;
        assert_eq!(snapshot.target, 3);
        assert_eq!(snapshot.members.len(), 1);
        assert_eq!(snapshot.members[0].url, "https://a.example");
        assert_eq!(snapshot.members[0].breaker, BreakerMirror::Closed);
        assert_eq!(snapshot.health, EngineHealth::Degraded);
    }
}
