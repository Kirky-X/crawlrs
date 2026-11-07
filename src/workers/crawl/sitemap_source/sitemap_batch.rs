// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! sitemap 批处理辅助（从 `sitemap_source.rs` 拆分的纯函数）

use super::{SeedTaskContext, SitemapFeedStats};
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::dedup::{DedupResult, Deduplicator};
use crate::workers::crawl::{DomainFilter, FilterContext, UrlFilter, UrlPatternFilter};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

/// FilterChain 过滤（自 [`flush_batch`] 提取），判定顺序与短路行为逐字保留：
/// 黑名单绝对拒绝（优先级高于 include/exclude，对齐 extract_and_queue_links）
/// → include 为显式空列表整体短路 → include/exclude patterns。
/// 被拒 URL 计入 `stats.filtered_out`。
pub(super) fn filter_batch_urls(
    batch: &mut Vec<String>,
    stats: &mut SitemapFeedStats,
    pattern_filter: Option<&UrlPatternFilter>,
    blacklist_filter: Option<&DomainFilter>,
    filter_ctx: &FilterContext,
    empty_include_short_circuit: bool,
) -> Vec<String> {
    let mut to_enqueue: Vec<String> = Vec::new();
    for url in batch.drain(..) {
        // 黑名单绝对拒绝（优先级高于 include/exclude，对齐 extract_and_queue_links）
        if let Some(bf) = blacklist_filter {
            if !bf.accept(&url, filter_ctx) {
                stats.filtered_out += 1;
                continue;
            }
        }
        if empty_include_short_circuit {
            stats.filtered_out += 1;
            continue;
        }
        if let Some(filter) = pattern_filter {
            if !filter.accept(&url, filter_ctx) {
                stats.filtered_out += 1;
                continue;
            }
        }
        to_enqueue.push(url);
    }
    to_enqueue
}

/// 分层去重（自 [`flush_batch`] 提取）：Bloom 阴性直接入队；阳性回落 DB 精确
/// 校验（假阳性必须入队）。末尾批内去重（同一 sitemap 可能重复列出同一 URL）。
pub(super) async fn dedup_filtered_batch(
    to_enqueue: Vec<String>,
    dedup: &Arc<parking_lot::RwLock<Deduplicator>>,
    task_repo: &dyn TaskRepository,
    stats: &mut SitemapFeedStats,
) -> anyhow::Result<Vec<String>> {
    let mut new_urls: Vec<String> = Vec::new();
    let mut db_check: Vec<String> = Vec::new();
    {
        let mut dedup = dedup.write();
        for url in to_enqueue {
            match dedup.check_and_insert(&url) {
                Ok(DedupResult::DefinitelyNew { normalized }) => new_urls.push(normalized),
                Ok(DedupResult::MaybeExisting { normalized, .. }) => db_check.push(normalized),
                Err(e) => return Err(anyhow::anyhow!("URL dedup check failed for {url}: {e}")),
            }
        }
    }

    if !db_check.is_empty() {
        db_check.sort_unstable();
        db_check.dedup();
        let existing: HashSet<String> = task_repo.find_existing_urls(&db_check).await?;
        for normalized in db_check {
            if existing.contains(&normalized) {
                stats.duplicates_skipped += 1;
            } else {
                // DB 确认不存在：布隆阳性必为假阳性，无条件 insert + 入队
                //（禁止二次 check_and_insert——首轮已置位布隆，必复现阳性而漏抓）
                dedup.write().insert(&normalized);
                new_urls.push(normalized);
            }
        }
    }

    // 批内去重（同一 sitemap 可能重复列出同一 URL）
    new_urls.sort_unstable();
    new_urls.dedup();
    Ok(new_urls)
}

/// 构造 sitemap crawl 子任务（纯数据映射，自 [`flush_batch`] 提取）
pub(super) fn build_sitemap_task(url: &str, seed: &SeedTaskContext) -> Task {
    Task {
        id: crate::domain::id::Id::generate(),
        task_type: TaskType::Crawl,
        status: TaskStatus::Queued,
        priority: seed.priority,
        team_id: seed.team_id,
        api_key_id: seed.api_key_id,
        url: url.to_string(),
        payload: json!({
            "crawl_id": seed.crawl_id,
            "depth": 1,
            "config": seed.config_json,
        }),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: None,
        crawl_id: Some(seed.crawl_id),
        updated_at: chrono::Utc::now(),
        lock_token: None,
        lock_expires_at: None,
        expires_at: None,
    }
}
