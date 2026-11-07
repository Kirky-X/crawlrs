// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 采集轮执行步骤
//!
//! [`SearxngInstanceServiceImpl::collect_inner`] 的三段执行体：数据集拉取解析、
//! 逐条准入 + 幂等 upsert、缺席退役。函数只依赖注入端口（传输 / 登记簿仓储），
//! 不持有服务内部状态；调用方负责采集锁与报告汇总。
//!
//! [`SearxngInstanceServiceImpl::collect_inner`]: super::service::SearxngInstanceServiceImpl::collect_inner

use std::collections::HashSet;

use log::{debug, error, info, warn};

use crate::config::SearxngCollectorSettings;
use crate::domain::repositories::searxng_instance_repository::{
    normalize_instance_url, SearxngInstanceRepository, SearxngInstanceSource, UpsertOutcome,
};
use crate::domain::repositories::task_repository::RepositoryError;

use super::dataset::{admit, parse_dataset, Dataset};
use super::{CollectorReport, SearxngError, SearxngTransport};

/// 「不限制」分页上限：sqlx 把 limit 绑为 i64，直接用 u64::MAX 会 PosOverflow
const UNBOUNDED_LIMIT: u64 = i64::MAX as u64;

/// 拉取并解析数据集：传输失败/非 2xx/解析失败均映射为 `SearxngError::DatasetFetch`
pub(super) async fn fetch_dataset(
    transport: &dyn SearxngTransport,
    collector: &SearxngCollectorSettings,
) -> Result<Dataset, SearxngError> {
    let timeout = std::time::Duration::from_secs(collector.request_timeout_seconds);

    let (status, payload) = transport
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
        let err =
            SearxngError::DatasetFetch(format!("{} returned HTTP {status}", collector.data_url));
        error!("{err}");
        return Err(err);
    }

    parse_dataset(&payload).map_err(|e| {
        error!("searxng dataset parse failed: {e}");
        SearxngError::DatasetFetch(e.to_string())
    })
}

/// 数据集条目逐条准入 + 幂等 upsert；`present` 记录规范化 url（缺席判定依据）
pub(super) async fn sync_instances(
    repo: &dyn SearxngInstanceRepository,
    dataset: &Dataset,
    collector: &SearxngCollectorSettings,
    report: &mut CollectorReport,
    present: &mut HashSet<String>,
) {
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

        match repo.upsert_from_dataset(&admitted).await {
            Ok(UpsertOutcome::Inserted) => report.admitted_new += 1,
            Ok(UpsertOutcome::Updated) => report.updated += 1,
            Err(e) => {
                report.errors += 1;
                warn!("searxng upsert failed url={} error={e}", admitted.url);
            }
        }
    }
}

/// 缺席退役：既有 searx_space 实例未出现在本轮数据集 → 累计缺席
pub(super) async fn retire_absent(
    repo: &dyn SearxngInstanceRepository,
    present: &HashSet<String>,
    collector: &SearxngCollectorSettings,
    report: &mut CollectorReport,
) -> Result<(), SearxngError> {
    // 缺席退役：既有 searx_space 实例未出现在本轮数据集 → 累计缺席；
    // seed 来源永不退役（仓储侧豁免）。
    let existing = repo.list_paginated(UNBOUNDED_LIMIT, 0).await.map_err(|e| {
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
        match repo
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
    Ok(())
}
