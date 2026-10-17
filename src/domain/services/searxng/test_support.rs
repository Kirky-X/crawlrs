// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 领域服务的测试替身
//!
//! 代码库惯例：**不引入 mock 库**，用真实 trait impl 写测试行为。
//! 这里提供两个替身：
//! - [`InMemorySearxngRepo`]：内存版登记簿（语义对齐 Sea-ORM 实现，含状态机校验）；
//! - [`ScriptedTransport`]：按脚本返回响应并记录调用 URL 的传输替身。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::Mutex;
use serde_json::Value;

use crate::domain::repositories::searxng_instance_repository::{
    AdmittedInstance, CollectLockGuard, SearxngInstance, SearxngInstanceRepository,
    SearxngInstanceSource, SearxngInstanceState, UpsertOutcome,
};
use crate::domain::repositories::task_repository::RepositoryError;

use super::{SearxngError, SearxngTransport};

/// 内存版采集锁守卫（释放时复位标志）
struct InMemoryLockGuard {
    locked: Arc<Mutex<bool>>,
}

#[async_trait]
impl CollectLockGuard for InMemoryLockGuard {
    async fn release(&self) -> Result<(), RepositoryError> {
        *self.locked.lock() = false;
        Ok(())
    }
}

/// 内存版登记簿（测试替身）
#[derive(Default)]
pub struct InMemorySearxngRepo {
    instances: Mutex<HashMap<String, SearxngInstance>>,
    /// 采集轮锁标志（模拟跨进程互斥）
    collect_locked: Arc<Mutex<bool>>,
    /// `try_acquire_collect_lock` 调用次数
    pub lock_attempts: AtomicU32,
    /// `try_acquire_collect_lock` 成功次数
    pub lock_acquisitions: AtomicU32,
    /// `upsert_from_dataset` 调用次数（断言「失败时 repo 零调用」）
    pub dataset_upserts: AtomicU32,
    /// `upsert_seed` 调用次数
    pub seed_upserts: AtomicU32,
    /// `record_probe_result` 调用次数
    pub probe_records: AtomicU32,
}

impl InMemorySearxngRepo {
    /// 空登记簿
    pub fn new() -> Self {
        Self::default()
    }

    /// 直接注入一条实例（测试布线用）
    pub fn insert_raw(&self, instance: SearxngInstance) {
        self.instances.lock().insert(instance.url.clone(), instance);
    }

    /// 构造一条 seed 实例
    pub fn seed_instance(url: &str, state: SearxngInstanceState) -> SearxngInstance {
        Self::instance(url, state, SearxngInstanceSource::Seed)
    }

    /// 构造一条 searx.space 实例
    pub fn dataset_instance(url: &str, state: SearxngInstanceState) -> SearxngInstance {
        Self::instance(url, state, SearxngInstanceSource::SearxSpace)
    }

    fn instance(
        url: &str,
        state: SearxngInstanceState,
        source: SearxngInstanceSource,
    ) -> SearxngInstance {
        let now = Utc::now();
        SearxngInstance {
            url: url.to_string(),
            source,
            state,
            network_type: Some("normal".to_string()),
            version: None,
            api_grade: None,
            http_grade: None,
            tls_grade: None,
            cdn_grade: None,
            uptime_day: Some(99.0),
            uptime_month: None,
            latency_ms: None,
            consecutive_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
            dataset_miss_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// 当前登记簿行数
    pub fn len(&self) -> usize {
        self.instances.lock().len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.instances.lock().is_empty()
    }

    /// 读取一条实例快照
    pub fn get(&self, url: &str) -> Option<SearxngInstance> {
        self.instances.lock().get(url).cloned()
    }

    /// 全部实例快照（按 url 排序，便于断言）
    pub fn all(&self) -> Vec<SearxngInstance> {
        let mut all: Vec<SearxngInstance> = self.instances.lock().values().cloned().collect();
        all.sort_by(|a, b| a.url.cmp(&b.url));
        all
    }
}

#[async_trait]
impl SearxngInstanceRepository for InMemorySearxngRepo {
    async fn try_acquire_collect_lock(
        &self,
        _key: i64,
    ) -> Result<Option<Box<dyn CollectLockGuard>>, RepositoryError> {
        self.lock_attempts.fetch_add(1, Ordering::SeqCst);
        let mut locked = self.collect_locked.lock();
        if *locked {
            return Ok(None);
        }
        *locked = true;
        self.lock_acquisitions.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Box::new(InMemoryLockGuard {
            locked: Arc::clone(&self.collect_locked),
        })))
    }

    async fn upsert_from_dataset(
        &self,
        instance: &AdmittedInstance,
    ) -> Result<UpsertOutcome, RepositoryError> {
        self.dataset_upserts.fetch_add(1, Ordering::SeqCst);
        let mut map = self.instances.lock();
        match map.get_mut(&instance.url) {
            Some(existing) => {
                existing.dataset_miss_count = 0;
                existing.uptime_day = instance.uptime_day;
                existing.uptime_month = instance.uptime_month;
                existing.network_type = instance.network_type.clone();
                if existing.state == SearxngInstanceState::Retired {
                    existing.state = SearxngInstanceState::Candidate;
                }
                existing.updated_at = Utc::now();
                Ok(UpsertOutcome::Updated)
            }
            None => {
                let mut row = Self::instance(
                    &instance.url,
                    SearxngInstanceState::Candidate,
                    SearxngInstanceSource::SearxSpace,
                );
                row.uptime_day = instance.uptime_day;
                row.uptime_month = instance.uptime_month;
                row.network_type = instance.network_type.clone();
                map.insert(instance.url.clone(), row);
                Ok(UpsertOutcome::Inserted)
            }
        }
    }

    async fn upsert_seed(&self, url: &str) -> Result<UpsertOutcome, RepositoryError> {
        self.seed_upserts.fetch_add(1, Ordering::SeqCst);
        let mut map = self.instances.lock();
        match map.get_mut(url) {
            Some(existing) => {
                existing.source = SearxngInstanceSource::Seed;
                existing.updated_at = Utc::now();
                Ok(UpsertOutcome::Updated)
            }
            None => {
                map.insert(
                    url.to_string(),
                    Self::seed_instance(url, SearxngInstanceState::Candidate),
                );
                Ok(UpsertOutcome::Inserted)
            }
        }
    }

    async fn list_candidates(&self, limit: u64) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let mut candidates: Vec<SearxngInstance> = self
            .instances
            .lock()
            .values()
            .filter(|i| i.state == SearxngInstanceState::Candidate)
            .cloned()
            .collect();
        candidates.sort_by(|a, b| {
            let uptime = b.uptime_day.unwrap_or(f64::NEG_INFINITY);
            let lhs = a.uptime_day.unwrap_or(f64::NEG_INFINITY);
            uptime
                .partial_cmp(&lhs)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| match (a.latency_ms, b.latency_ms) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                })
        });
        candidates.truncate(limit as usize);
        Ok(candidates)
    }

    async fn list_by_state(
        &self,
        state: SearxngInstanceState,
        limit: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let mut rows: Vec<SearxngInstance> = self
            .instances
            .lock()
            .values()
            .filter(|i| i.state == state)
            .cloned()
            .collect();
        rows.truncate(limit as usize);
        Ok(rows)
    }

    async fn list_paginated(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, RepositoryError> {
        let mut rows = self.all();
        rows.sort_by(|a, b| {
            a.state.as_str().cmp(b.state.as_str()).then_with(|| {
                b.uptime_day
                    .unwrap_or(f64::NEG_INFINITY)
                    .partial_cmp(&a.uptime_day.unwrap_or(f64::NEG_INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        Ok(rows
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    async fn record_probe_result(
        &self,
        url: &str,
        ok: bool,
        latency_ms: Option<i64>,
        error: Option<&str>,
    ) -> Result<(), RepositoryError> {
        self.probe_records.fetch_add(1, Ordering::SeqCst);
        let mut map = self.instances.lock();
        let instance = map.get_mut(url).ok_or(RepositoryError::NotFound)?;
        if ok {
            instance.consecutive_failures = 0;
            instance.latency_ms = latency_ms;
            instance.last_success_at = Some(Utc::now());
            instance.last_error = None;
        } else {
            instance.consecutive_failures += 1;
            instance.last_failure_at = Some(Utc::now());
            instance.last_error = error.map(|e| e.to_string());
        }
        instance.updated_at = Utc::now();
        Ok(())
    }

    async fn transition_state(
        &self,
        url: &str,
        to: SearxngInstanceState,
    ) -> Result<(), RepositoryError> {
        let mut map = self.instances.lock();
        let instance = map.get_mut(url).ok_or(RepositoryError::NotFound)?;
        if !instance.state.can_transition_to(to) {
            return Err(RepositoryError::Database(anyhow::anyhow!(
                "illegal searxng instance state transition for {url}: {} -> {to}",
                instance.state
            )));
        }
        instance.state = to;
        instance.updated_at = Utc::now();
        Ok(())
    }

    async fn record_dataset_miss(
        &self,
        url: &str,
        retire_after: i32,
    ) -> Result<bool, RepositoryError> {
        let mut map = self.instances.lock();
        let instance = map.get_mut(url).ok_or(RepositoryError::NotFound)?;
        if instance.source != SearxngInstanceSource::SearxSpace {
            return Ok(false);
        }
        instance.dataset_miss_count += 1;
        if instance.dataset_miss_count > retire_after {
            if instance
                .state
                .can_transition_to(SearxngInstanceState::Retired)
            {
                instance.state = SearxngInstanceState::Retired;
                return Ok(true);
            }
            if instance.state == SearxngInstanceState::Active {
                instance.state = SearxngInstanceState::Cooldown;
            }
        }
        Ok(false)
    }

    async fn get_by_url(&self, url: &str) -> Result<Option<SearxngInstance>, RepositoryError> {
        Ok(self.instances.lock().get(url).cloned())
    }

    async fn count_by_state(&self, state: SearxngInstanceState) -> Result<u64, RepositoryError> {
        Ok(self
            .instances
            .lock()
            .values()
            .filter(|i| i.state == state)
            .count() as u64)
    }
}

/// 脚本化响应
#[derive(Debug, Clone)]
pub enum Scripted {
    /// 成功响应（status + body）
    Json {
        /// HTTP 状态码
        status: u16,
        /// JSON 响应体
        body: Value,
    },
    /// 连接失败
    Network(String),
    /// 超时
    Timeout,
}

/// 传输替身：按脚本顺序返回响应，并记录收到的 URL
pub struct ScriptedTransport {
    queue: Mutex<VecDeque<Scripted>>,
    /// 已发出的请求数
    pub calls: AtomicUsize,
    /// 已收到的 URL（顺序与调用一致）
    pub urls: Mutex<Vec<String>>,
}

impl ScriptedTransport {
    /// 用脚本构造
    pub fn new(script: Vec<Scripted>) -> Self {
        Self {
            queue: Mutex::new(script.into_iter().collect()),
            calls: AtomicUsize::new(0),
            urls: Mutex::new(Vec::new()),
        }
    }

    /// 重复同一响应的脚本
    pub fn repeat(response: Scripted, count: usize) -> Self {
        Self::new(std::iter::repeat_n(response, count).collect())
    }

    /// 已收到的 URL 快照
    pub fn received_urls(&self) -> Vec<String> {
        self.urls.lock().clone()
    }
}

#[async_trait]
impl SearxngTransport for ScriptedTransport {
    async fn get_json(&self, url: &str, timeout: Duration) -> Result<(u16, Value), SearxngError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.urls.lock().push(url.to_string());
        let scripted = self.queue.lock().pop_front().unwrap_or_else(|| {
            Scripted::Network(format!("scripted transport exhausted for {url}"))
        });
        match scripted {
            Scripted::Json { status, body } => Ok((status, body)),
            Scripted::Network(message) => Err(SearxngError::Network(message)),
            Scripted::Timeout => Err(SearxngError::Timeout(timeout)),
        }
    }
}

/// 构造测试用配置（默认值 + 可选覆盖）
pub fn test_settings() -> crate::config::SearxngSearchSettings {
    crate::config::SearxngSearchSettings::default()
}

/// `Arc` 包裹的内存登记簿
pub fn shared_repo() -> Arc<InMemorySearxngRepo> {
    Arc::new(InMemorySearxngRepo::new())
}
