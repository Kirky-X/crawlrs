// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 元搜索领域服务
//!
//! 公共 SearXNG 实例的完整生命周期：采集（searx.space 数据集）→ 登记簿（DB）
//! → 探活 → 热池（进程内 target=3 缓存）→ 熔断与晋升。搜索侧只通过
//! [`SearxngInstanceService`] 端口取实例、回填结果，不直连 DB 与数据集。
//!
//! 模块边界（规则：`mod.rs` 只放接口与类型）：
//! - `dataset.rs` — 数据集宽容解析 + 准入谓词
//! - `pool.rs` — 进程内热池（纯内存，无外部依赖）
//! - `probe.rs` — JSON API 探活判活
//! - `service.rs` — 编排（采集 / 探活 / 熔断 / 晋升 / 降级）
//! - `http.rs` — [`SearxngTransport`] 的 reqwest 实现

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::repositories::searxng_instance_repository::SearxngInstance;

pub mod dataset;
pub mod http;
pub mod pool;
pub mod probe;
pub mod service;

#[cfg(test)]
pub(crate) mod test_support;

pub use dataset::{admit, parse_dataset, AdmissionReject, Dataset, DatasetEntry};
pub use http::ReqwestSearxngTransport;
pub use pool::InstancePool;
pub use probe::{probe_instance, probe_url, ProbeOutcome};
pub use service::SearxngInstanceServiceImpl;

/// SearXNG 子系统错误（结构化：可被采集报告与搜索错误分类消费）
#[derive(Debug, Clone, Error)]
pub enum SearxngError {
    /// 连接/传输失败
    #[error("searxng network error: {0}")]
    Network(String),
    /// 请求超时
    #[error("searxng request timed out after {0:?}")]
    Timeout(Duration),
    /// 非 2xx 状态（429 单独映射为 [`SearxngError::RateLimited`]）
    #[error("searxng returned HTTP status {0}")]
    BadHttpStatus(u16),
    /// 被实例限流（HTTP 429）
    #[error("searxng instance rate limited")]
    RateLimited,
    /// 响应体解析失败（非 JSON / 结构不符）
    #[error("searxng parse error: {0}")]
    Parse(String),
    /// 数据集拉取失败（登记簿保持不动）
    #[error("searxng dataset fetch failed: {0}")]
    DatasetFetch(String),
    /// 登记簿读写失败
    #[error("searxng repository error: {0}")]
    Repository(String),
    /// 功能未启用（`search.searxng.enabled = false`）
    #[error("searxng disabled")]
    Disabled,
    /// 热池为空（无可用实例）
    #[error("no searxng instance available")]
    NoInstanceAvailable,
}

impl SearxngError {
    /// HTTP 状态码 → 结构化错误（429 单独分类）
    pub fn from_status(status: u16) -> Self {
        if status == 429 {
            SearxngError::RateLimited
        } else {
            SearxngError::BadHttpStatus(status)
        }
    }

    /// 便于日志/报告展示的错误分类名
    pub fn class(&self) -> &'static str {
        match self {
            SearxngError::Network(_) => "network",
            SearxngError::Timeout(_) => "timeout",
            SearxngError::BadHttpStatus(_) => "bad_http_status",
            SearxngError::RateLimited => "rate_limited",
            SearxngError::Parse(_) => "parse",
            SearxngError::DatasetFetch(_) => "dataset_fetch",
            SearxngError::Repository(_) => "repository",
            SearxngError::Disabled => "disabled",
            SearxngError::NoInstanceAvailable => "no_instance_available",
        }
    }
}

/// HTTP 端口：数据集采集、探活、引擎搜索三处共用。
///
/// 测试以 trait impl 替身驱动（代码库惯例：无 mock 库，用真实 impl 写行为）。
#[async_trait]
pub trait SearxngTransport: Send + Sync {
    /// GET 目标 url（带 UA 与超时）。
    ///
    /// 约定：2xx 返回 `(status, 解析后的 JSON)`；非 2xx 返回 `(status, Value::Null)`
    /// （错误体不参与解析，分类由调用方按状态码决定）。
    async fn get_json(
        &self,
        url: &str,
        timeout: Duration,
    ) -> Result<(u16, serde_json::Value), SearxngError>;
}

/// 一轮采集的显性计数报告
///
/// 字段口径：`fetched` = 数据集条目总数；`seen` = 通过准入的条目数；
/// `admitted_new` = 登记簿新增行数；`updated` = 既有行刷新数；
/// `retired` = 本轮因数据集缺席而退役的实例数；
/// `errors` = 准入拒绝 + 登记簿写入失败的总数。`skipped` 为 advisory lock
/// 抢占失败跳过的轮次（此时其余计数全为 0）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectorReport {
    /// 本轮是否因并发采集锁被跳过
    pub skipped: bool,
    /// 数据集条目总数
    pub fetched: usize,
    /// 通过准入的条目数
    pub seen: usize,
    /// 登记簿新增行数
    pub admitted_new: usize,
    /// 既有行刷新数
    pub updated: usize,
    /// 本轮退役数
    pub retired: usize,
    /// 准入拒绝 + 写入失败总数
    pub errors: usize,
}

impl CollectorReport {
    /// 被并发锁跳过的空报告
    pub fn skipped() -> Self {
        Self {
            skipped: true,
            ..Default::default()
        }
    }
}

/// 热池中的成员（实例值快照）
#[derive(Debug, Clone)]
pub struct PoolMember {
    /// 登记簿中的实例快照
    pub instance: SearxngInstance,
}

impl PoolMember {
    /// 实例 base URL
    pub fn url(&self) -> &str {
        &self.instance.url
    }
}

/// 一次请求/探测的结果回填
#[derive(Debug, Clone)]
pub enum InstanceOutcome {
    /// 成功（带时延毫秒）
    Success {
        /// 请求耗时（毫秒）
        latency_ms: i64,
    },
    /// 失败（结构化错误）
    Failure {
        /// 失败分类
        error: SearxngError,
    },
}

impl InstanceOutcome {
    /// 是否成功
    pub fn is_success(&self) -> bool {
        matches!(self, InstanceOutcome::Success { .. })
    }
}

/// 熔断镜像态（管理 API 只透出这一枚举，不透出内部实现）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakerMirror {
    /// 关闭（正常放行）
    Closed,
    /// 打开（拒绝请求）
    Open,
    /// 半开（放行探测）
    HalfOpen,
}

/// 池成员对外视图（管理 API DTO 数据源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolMemberState {
    /// 实例 url
    pub url: String,
    /// 登记簿状态
    pub state: crate::domain::repositories::searxng_instance_repository::SearxngInstanceState,
    /// 最近成功时延
    pub latency_ms: Option<i64>,
    /// 熔断镜像态
    pub breaker: BreakerMirror,
}

/// 热池快照（`GET /v1/searxng/pool` 数据源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSnapshot {
    /// 控制回路目标成员数
    pub target: usize,
    /// 当前成员
    pub members: Vec<PoolMemberState>,
    /// 服务健康口径
    pub health: crate::search::types::EngineHealth,
}

/// 一轮探活报告的显性计数
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthReport {
    /// 本轮探测实例数
    pub probed: usize,
    /// 晋升入池数
    pub promoted: usize,
    /// 逐出/降级数
    pub demoted: usize,
    /// 退役数
    pub retired: usize,
    /// 探测失败数（含超时/限流/解析失败）
    pub errors: usize,
}

/// SearXNG 实例生命周期服务端口
///
/// 搜索侧只依赖本端口（不直连 DB / 数据集）：
/// - [`acquire`](SearxngInstanceService::acquire) 从热池轮转取实例；
/// - [`report`](SearxngInstanceService::report) 回填请求结果（熔断记账 + 晋升）；
/// - [`collect`](SearxngInstanceService::collect) / [`health_cycle`](SearxngInstanceService::health_cycle)
///   由周期 worker 与管理 API 驱动；
/// - [`pool_snapshot`](SearxngInstanceService::pool_snapshot) 供管理 API 展示；
/// - [`health`](SearxngInstanceService::health) 提供引擎健康口径。
#[async_trait]
pub trait SearxngInstanceService: Send + Sync {
    /// 从热池轮转取一个可用实例（跳过 `exclude` 与熔断开成员）；池空返回 None
    fn acquire(&self, exclude: &[String]) -> Option<Arc<PoolMember>>;

    /// 请求结果回填：成功→熔断记账 + 时延；失败→熔断记账 + 连续失败累计，
    /// 触发逐出与异步晋升（维持 `pool_target`）
    async fn report(&self, url: &str, outcome: InstanceOutcome);

    /// 采集一轮（周期 worker / 管理 API 手动触发）
    async fn collect(&self) -> Result<CollectorReport, SearxngError>;

    /// 探活一轮：热池全体 + 前 `candidate_batch` 个候选；按状态机迁移
    async fn health_cycle(&self) -> HealthReport;

    /// 管理 API 视图（热池 + 熔断镜像态 + 健康口径）
    async fn pool_snapshot(&self) -> PoolSnapshot;

    /// 登记簿分页列表（管理 API 视图：active 优先 + uptime 降序）
    async fn list_instances(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SearxngInstance>, SearxngError>;

    /// 引擎健康口径：池成员数 ≥ target → Healthy；0 < 成员数 < target → Degraded；0 → Unhealthy
    fn health(&self) -> crate::search::types::EngineHealth;
}
