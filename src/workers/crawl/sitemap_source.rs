// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Sitemap 流式请求源（F11）：sitemap 作为 crawl 的一等 URL 发现源
//!
//! 消费 [`MapUseCase::execute_streaming`] 的 `<loc>` 有界通道，逐 URL 过
//! FilterChain（include/exclude 通配 + 域名黑名单）与分层去重
//! （Bloom 预筛 → DB 精确校验，语义对齐 `extract_and_queue_links`）后
//! 入队为 crawl Task 行（depth = 1，config 原样继承种子任务）。
//!
//! # 断点续爬（与 F3 共用 DB 持久）
//!
//! 每入队一批持久化进度指针 `{sitemap_url, consumed}` 到 `cache_service`；
//! worker 重启后种子任务被重新认领并再次进入喂入路径，从指针处继续。
//! 喂入完成（流耗尽）后删除指针。
//!
//! # seek 精度与正确性边界
//!
//! - `Urlset` 流保序：指针 `consumed=N` 时精确跳过前 N 条，从第 N+1 条继续。
//! - `SitemapIndex` 流的子 sitemap 并发展开、到达顺序天然不定：两次消费的
//!   顺序无法对齐，seek 会跳错条目。此场景不 seek，改为全量重喂，重复由
//!   分层去重兜底（已入队 loc 在 dedup/DB 中命中被跳过）——正确性由
//!   dedup + DB 校验保证，指针只加速保序的 Urlset 场景。
//!
//! 入队批与指针持久化非原子：create 成功、指针写回前崩溃 → 恢复后指针
//! 滞后，重复 loc 走 dedup/DB 被挡（安全方向：多检不漏喂）。
//!
//! # 失败显性化
//!
//! 拉取失败向上传播，由调用方打 error 日志并说明后果（crawl 继续以页面
//! 链接发现运行）；sitemap 404 是合法状态（站点无 sitemap），返回零入队
//! 而非错误。

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;
use uuid::Uuid;

use crate::application::dto::map_request::MapRequestDto;
use crate::application::use_cases::map_use_case::{
    MapUseCase, SitemapFetcher, SitemapKind, SitemapStreamOptions,
};
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::infrastructure::oxcache::CacheService;
use crate::utils::dedup::{DedupResult, Deduplicator};
use crate::workers::crawl::{DomainFilter, FilterContext, UrlFilter, UrlPatternFilter};

/// 批大小：每消费这么多 loc 执行一次过滤/去重/入队/指针持久化
const FEED_BATCH_SIZE: usize = 500;

/// 喂入进度指针：记录消费中的 sitemap 文档与已消费 `<loc>` 数
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitemapFeedPointer {
    /// 正在消费的 sitemap 文档 URL
    pub sitemap_url: String,
    /// 该文档已消费（喂入判定完毕）的 `<loc>` 数
    pub consumed: usize,
}

/// 喂入结果统计
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SitemapFeedStats {
    /// 入队的 crawl Task 行数
    pub enqueued: usize,
    /// 被 FilterChain 拒绝的 loc 数
    pub filtered_out: usize,
    /// 被去重跳过（DB 确认已存在）的 loc 数
    pub duplicates_skipped: usize,
    /// 从通道消费的 loc 总数（含 seek 跳过与重复）
    pub consumed: usize,
    /// 恢复起点（Urlset 流从指针跳过的条数；无指针 / Index 流为 None）
    pub resumed_from: Option<usize>,
}

/// 入队 Task 行的种子上下文（继承种子任务的归属与配置）
#[derive(Debug, Clone)]
pub struct SeedTaskContext {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub crawl_id: Uuid,
    /// 子任务 priority（对齐 `extract_and_queue_links`：继承种子任务）
    pub priority: i32,
    /// 种子任务 payload 中的 `config`（原样透传，保持与链接提取侧同构）
    pub config_json: serde_json::Value,
}

/// 指针的 cache key（每 crawl 一条）
pub fn sitemap_feed_pointer_key(crawl_id: Uuid) -> String {
    format!("crawl:sitemap-feed:{crawl_id}")
}

/// 从 crawl 种子 URL 派生默认 sitemap 地址：`scheme://host[:port]/sitemap.xml`
pub fn derive_sitemap_url(seed_url: &str) -> Option<String> {
    let parsed = Url::parse(seed_url).ok()?;
    let host = parsed.host_str()?;
    let origin = match parsed.port() {
        Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
        None => format!("{}://{host}", parsed.scheme()),
    };
    Some(format!("{origin}/sitemap.xml"))
}

/// 读取指针。cache 缺失 / 解析失败 / 指向不同 sitemap（源已变）→ None；
/// 指针是加速不是正确性依赖，任何异常都安全回退到全量重喂 + 去重兜底。
async fn load_pointer(
    cache: &dyn CacheService,
    crawl_id: Uuid,
    sitemap_url: &str,
) -> Option<usize> {
    let raw = cache
        .get(&sitemap_feed_pointer_key(crawl_id))
        .await
        .ok()??;
    match serde_json::from_str::<SitemapFeedPointer>(&raw) {
        Ok(p) if p.sitemap_url == sitemap_url => Some(p.consumed),
        Ok(p) => {
            log::warn!(
                "sitemap feed pointer for crawl {crawl_id} targets {} but feeding {sitemap_url}; \
                 ignoring — dedup/DB checks guard against re-enqueue",
                p.sitemap_url
            );
            None
        }
        Err(e) => {
            log::warn!("sitemap feed pointer for crawl {crawl_id} is corrupt ({e}); ignoring");
            None
        }
    }
}

/// 消费 sitemap 流并入队 crawl 任务（组件入口）
///
/// 拉取失败向上传播（调用方显性上报）；404 → 零入队的合法结果。
#[allow(clippy::too_many_arguments)]
pub async fn feed_sitemap_as_tasks(
    fetcher: Arc<dyn SitemapFetcher>,
    sitemap_url: String,
    options: SitemapStreamOptions,
    include_patterns: Option<Vec<String>>,
    exclude_patterns: Option<Vec<String>>,
    domain_blacklist: Vec<String>,
    seed: SeedTaskContext,
    dedup: &Arc<parking_lot::RwLock<Deduplicator>>,
    task_repo: &dyn TaskRepository,
    crawl_repo: &dyn CrawlRepository,
    cache: &dyn CacheService,
    pointer_ttl_secs: u64,
) -> anyhow::Result<SitemapFeedStats> {
    let use_case = MapUseCase::new(fetcher);
    let dto = MapRequestDto {
        url: sitemap_url.clone(),
        include_patterns: None,
        exclude_patterns: None,
        limit: None,
    };
    let stream = use_case
        .execute_streaming(&dto, options)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let mut rx = stream.rx;

    // seek 决策：仅 Urlset 流（保序）精确跳过；Index 流不 seek（见模块文档）
    let mut stats = SitemapFeedStats::default();
    let seek: Option<usize> = match load_pointer(cache, seed.crawl_id, &sitemap_url).await {
        Some(consumed) if consumed > 0 => {
            if stream.kind == SitemapKind::Urlset {
                stats.resumed_from = Some(consumed);
                Some(consumed)
            } else {
                log::info!(
                    "sitemap feed for crawl {} resumes without seek (index streams arrive in \
                     nondeterministic order; dedup guards re-enqueue), pointer consumed={consumed}",
                    seed.crawl_id
                );
                None
            }
        }
        _ => None,
    };

    // include 为显式空列表 = 全部拒绝（对齐 extract_and_queue_links 短路语义；
    // 与 UrlPatternFilter 的「空 include = 无限制」不同，必须单独短路）
    let empty_include_short_circuit =
        matches!(&include_patterns, Some(patterns) if patterns.is_empty());
    let pattern_filter = if empty_include_short_circuit {
        None
    } else {
        Some(UrlPatternFilter::new(
            include_patterns.unwrap_or_default(),
            exclude_patterns.unwrap_or_default(),
        ))
    };
    let blacklist_filter = if domain_blacklist.is_empty() {
        None
    } else {
        Some(DomainFilter::denylist(domain_blacklist))
    };
    let filter_ctx = FilterContext::default();

    let mut batch: Vec<String> = Vec::with_capacity(FEED_BATCH_SIZE.min(64));
    while let Some(loc) = rx.recv().await {
        if let Some(seek_n) = seek {
            if stats.consumed < seek_n {
                stats.consumed += 1;
                continue;
            }
        }
        stats.consumed += 1;
        batch.push(loc);
        // 先入批再 flush：flush 时指针 consumed 精确覆盖本批全部已处理 loc
        // （先 flush 后 push 会让指针超前于实际处理，崩溃恢复时漏喂边界 loc）
        if batch.len() >= FEED_BATCH_SIZE {
            flush_batch(
                &mut batch,
                &mut stats,
                pattern_filter.as_ref(),
                blacklist_filter.as_ref(),
                &filter_ctx,
                empty_include_short_circuit,
                &seed,
                &sitemap_url,
                dedup,
                task_repo,
                crawl_repo,
                cache,
                pointer_ttl_secs,
            )
            .await?;
        }
    }
    if !batch.is_empty() {
        flush_batch(
            &mut batch,
            &mut stats,
            pattern_filter.as_ref(),
            blacklist_filter.as_ref(),
            &filter_ctx,
            empty_include_short_circuit,
            &seed,
            &sitemap_url,
            dedup,
            task_repo,
            crawl_repo,
            cache,
            pointer_ttl_secs,
        )
        .await?;
    }

    // 喂入完成：指针使命结束（404/空流零消费时无指针可清）
    if stats.consumed > 0 {
        let _ = cache.delete(&sitemap_feed_pointer_key(seed.crawl_id)).await;
    }
    Ok(stats)
}

/// 单批处理：FilterChain 过滤 → 分层去重 → 入队 → 持久化指针
#[allow(clippy::too_many_arguments)]
async fn flush_batch(
    batch: &mut Vec<String>,
    stats: &mut SitemapFeedStats,
    pattern_filter: Option<&UrlPatternFilter>,
    blacklist_filter: Option<&DomainFilter>,
    filter_ctx: &FilterContext,
    empty_include_short_circuit: bool,
    seed: &SeedTaskContext,
    sitemap_url: &str,
    dedup: &Arc<parking_lot::RwLock<Deduplicator>>,
    task_repo: &dyn TaskRepository,
    crawl_repo: &dyn CrawlRepository,
    cache: &dyn CacheService,
    pointer_ttl_secs: u64,
) -> anyhow::Result<()> {
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

    // 分层去重：Bloom 阴性直接入队；阳性回落 DB 精确校验（假阳性必须入队）
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

    for url in &new_urls {
        let task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: seed.priority,
            team_id: seed.team_id,
            api_key_id: seed.api_key_id,
            url: url.clone(),
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
        };
        task_repo.create(&task).await?;
        crawl_repo.increment_total_tasks(seed.crawl_id).await?;
        stats.enqueued += 1;
    }

    // 每批持久化进度指针（断点续爬；此时 batch 已全部处理，consumed 精确）
    let pointer = SitemapFeedPointer {
        sitemap_url: sitemap_url.to_string(),
        consumed: stats.consumed,
    };
    cache
        .set(
            &sitemap_feed_pointer_key(seed.crawl_id),
            &serde_json::to_string(&pointer)?,
            pointer_ttl_secs,
        )
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::use_cases::map_use_case::MapError;
    use crate::domain::repositories::task_repository::RepositoryError;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ========== mocks ==========

    /// 内存 CacheService：记录 set/delete 调用（指针持久化断言用）
    #[derive(Default)]
    struct MockCache {
        store: parking_lot::Mutex<HashMap<String, String>>,
        set_keys: parking_lot::Mutex<Vec<String>>,
        deleted: parking_lot::Mutex<Vec<String>>,
    }

    impl MockCache {
        fn set_calls(&self, key: &str) -> usize {
            self.set_keys.lock().iter().filter(|k| *k == key).count()
        }
    }

    #[async_trait::async_trait]
    impl CacheService for MockCache {
        fn get(
            &self,
            key: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
        > {
            let v = self.store.lock().get(key).cloned();
            Box::pin(async move { Ok(v) })
        }
        fn set(
            &self,
            key: &str,
            value: &str,
            _ttl_seconds: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            self.store.lock().insert(key.to_string(), value.to_string());
            self.set_keys.lock().push(key.to_string());
            Box::pin(async { Ok(()) })
        }
        fn delete(
            &self,
            key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            self.store.lock().remove(key);
            self.deleted.lock().push(key.to_string());
            Box::pin(async { Ok(()) })
        }
        fn exists(
            &self,
            key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
        {
            let v = self.store.lock().contains_key(key);
            Box::pin(async move { Ok(v) })
        }
    }

    /// 计数任务仓库（记录 create 的 Task 行；可注入 DB 已存在 URL）
    struct CountingTaskRepo {
        created: parking_lot::Mutex<Vec<Task>>,
        existing_urls: HashSet<String>,
    }

    impl CountingTaskRepo {
        fn new() -> Self {
            Self {
                created: parking_lot::Mutex::new(Vec::new()),
                existing_urls: HashSet::new(),
            }
        }
        fn new_with_existing(urls: &[&str]) -> Self {
            Self {
                created: parking_lot::Mutex::new(Vec::new()),
                existing_urls: urls.iter().map(|s| s.to_string()).collect(),
            }
        }
        fn created_urls(&self) -> Vec<String> {
            self.created.lock().iter().map(|t| t.url.clone()).collect()
        }
    }

    #[async_trait]
    impl TaskRepository for CountingTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.created.lock().push(task.clone());
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(urls
                .iter()
                .filter(|u| self.existing_urls.contains(*u))
                .cloned()
                .collect())
        }
        async fn list_distinct_urls(
            &self,
            _limit: u32,
            _after_url: Option<String>,
        ) -> Result<Vec<String>, RepositoryError> {
            Ok(vec![])
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: crate::domain::repositories::task_repository::TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    /// 计数 crawl 仓库
    #[derive(Default)]
    struct CountingCrawlRepo {
        total_added: AtomicU32,
    }

    #[async_trait]
    impl CrawlRepository for CountingCrawlRepo {
        async fn create(
            &self,
            crawl: &crate::domain::models::Crawl,
        ) -> Result<crate::domain::models::Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn find_by_id(
            &self,
            _id: Uuid,
        ) -> Result<Option<crate::domain::models::Crawl>, RepositoryError> {
            Ok(None)
        }
        async fn update(
            &self,
            crawl: &crate::domain::models::Crawl,
        ) -> Result<crate::domain::models::Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: crate::domain::models::CrawlStatus,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            self.total_added.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<crate::domain::models::Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    /// 静态 sitemap mock：urlset XML 一次返回
    struct StaticSitemapFetcher {
        responses: HashMap<String, (u16, String)>,
    }

    fn urlset_xml(locs: &[String]) -> String {
        let inner: String = locs
            .iter()
            .map(|l| format!("<url><loc>{l}</loc></url>"))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{inner}</urlset>"#
        )
    }

    #[async_trait]
    impl SitemapFetcher for StaticSitemapFetcher {
        async fn fetch(
            &self,
            url: &str,
        ) -> Result<crate::application::use_cases::map_use_case::SitemapFetch, MapError> {
            match self.responses.get(url) {
                Some((status, body)) => {
                    Ok(crate::application::use_cases::map_use_case::SitemapFetch {
                        status: *status,
                        body: body.clone(),
                    })
                }
                None => Err(MapError::TargetUnreachable(format!("no mock for {url}"))),
            }
        }
    }

    fn seed_ctx(crawl_id: Uuid) -> SeedTaskContext {
        SeedTaskContext {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            crawl_id,
            priority: 100,
            config_json: json!({"max_depth": 2}),
        }
    }

    fn options(max_urls: usize) -> SitemapStreamOptions {
        SitemapStreamOptions {
            channel_capacity: 16,
            max_urls,
            max_concurrent_sub_sitemaps: 2,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn feed(
        fetcher: StaticSitemapFetcher,
        sitemap_url: &str,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        blacklist: Vec<String>,
        seed: SeedTaskContext,
        dedup: &Arc<parking_lot::RwLock<Deduplicator>>,
        task_repo: &CountingTaskRepo,
        crawl_repo: &CountingCrawlRepo,
        cache: &MockCache,
    ) -> anyhow::Result<SitemapFeedStats> {
        feed_sitemap_as_tasks(
            Arc::new(fetcher),
            sitemap_url.to_string(),
            options(50_000),
            include,
            exclude,
            blacklist,
            seed,
            dedup,
            task_repo,
            crawl_repo,
            cache,
            7 * 24 * 3600,
        )
        .await
    }

    const SITEMAP_URL: &str = "https://a.com/sitemap.xml";

    // ========== tests ==========

    /// 基础喂入：5 个 loc 全部入队为 depth=1 的 crawl Task 行，指针删除（完成）
    #[tokio::test]
    async fn feed_enqueues_task_rows_and_clears_pointer() {
        let locs: Vec<String> = (0..5).map(|i| format!("https://a.com/{i}")).collect();
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let crawl_id = Uuid::new_v4();
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(crawl_id),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(stats.enqueued, 5);
        assert_eq!(stats.consumed, 5);
        assert_eq!(stats.filtered_out, 0);
        assert_eq!(task_repo.created_urls().len(), 5);
        assert_eq!(crawl_repo.total_added.load(Ordering::SeqCst), 5);

        let created = task_repo.created.lock();
        let first = &created[0];
        assert_eq!(first.task_type, TaskType::Crawl);
        assert_eq!(first.status, TaskStatus::Queued);
        assert_eq!(first.crawl_id, Some(crawl_id));
        assert_eq!(first.payload["crawl_id"], crawl_id.to_string());
        assert_eq!(first.payload["depth"], 1);
        assert_eq!(first.payload["config"], json!({"max_depth": 2}));
        assert_eq!(first.team_id, first.team_id);

        // 喂入完成后指针必须删除（无残留断点）
        assert_eq!(
            cache.deleted.lock().len(),
            1,
            "pointer must be deleted after full feed"
        );
        assert!(!cache
            .store
            .lock()
            .contains_key(&sitemap_feed_pointer_key(crawl_id)));
    }

    /// FilterChain：include 白名单 + exclude 黑名单过滤后再入队
    #[tokio::test]
    async fn feed_applies_include_exclude_filters() {
        let locs: Vec<String> = vec![
            "https://a.com/blog/1".into(),
            "https://a.com/blog/tag/x".into(),
            "https://a.com/page/1".into(),
            "https://a.com/blog/2".into(),
        ];
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            Some(vec!["a\\.com/blog/.*".to_string()]),
            Some(vec!["a\\.com/blog/tag/.*".to_string()]),
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(stats.enqueued, 2, "only /blog/* minus /tag/* enqueue");
        assert_eq!(stats.filtered_out, 2);
        let mut urls = task_repo.created_urls();
        urls.sort();
        assert_eq!(urls, vec!["https://a.com/blog/1", "https://a.com/blog/2"]);
    }

    /// include 为空列表 = 全部拒绝（对齐 extract_and_queue_links 短路语义）
    #[tokio::test]
    async fn feed_empty_include_blocks_all() {
        let locs: Vec<String> = (0..3).map(|i| format!("https://a.com/{i}")).collect();
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            Some(vec![]),
            None,
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(stats.enqueued, 0);
        assert_eq!(stats.filtered_out, 3);
    }

    /// 域名黑名单绝对拒绝
    #[tokio::test]
    async fn feed_domain_blacklist_blocks() {
        let locs: Vec<String> = vec!["https://a.com/ok".into(), "https://evil.com/bad".into()];
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec!["evil.com".to_string()],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(task_repo.created_urls(), vec!["https://a.com/ok"]);
        assert_eq!(stats.filtered_out, 1);
    }

    /// 分层去重：进程内 dedup 已见（MaybeExisting）+ DB 确认存在 → 不入队；
    /// DB 确认不存在（布隆假阳性）→ 必须入队
    #[tokio::test]
    async fn feed_dedup_layers_skip_known_urls() {
        let locs: Vec<String> = vec![
            "https://a.com/in-process".into(), // dedup 预插 + DB 说存在 → 跳过
            "https://a.com/bloom-fp".into(),   // dedup 预插 + DB 说不存在 → 入队
            "https://a.com/fresh".into(),      // 全新 → 入队
        ];
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let task_repo = CountingTaskRepo::new_with_existing(&["https://a.com/in-process"]);
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));
        {
            let mut d = dedup.write();
            d.insert("https://a.com/in-process");
            d.insert("https://a.com/bloom-fp");
        }

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(
            stats.enqueued, 2,
            "in-process URL skipped, bloom false positive enqueued"
        );
        assert_eq!(stats.duplicates_skipped, 1);
        let mut urls = task_repo.created_urls();
        urls.sort();
        assert_eq!(urls, vec!["https://a.com/bloom-fp", "https://a.com/fresh"]);
    }

    /// 断点续爬：指针 consumed=3 → 重建全部组件后从第 4 条继续（Urlset 精确 seek）
    #[tokio::test]
    async fn feed_resumes_from_pointer_seek() {
        let locs: Vec<String> = (0..10).map(|i| format!("https://a.com/{i}")).collect();
        let xml = urlset_xml(&locs);
        let crawl_id = Uuid::new_v4();

        // 第一次：喂入 3 条后"崩溃"（手动写指针模拟断点）
        let cache = MockCache::default();
        let pointer = SitemapFeedPointer {
            sitemap_url: SITEMAP_URL.to_string(),
            consumed: 3,
        };
        cache
            .set(
                &sitemap_feed_pointer_key(crawl_id),
                &serde_json::to_string(&pointer).unwrap(),
                3600,
            )
            .await
            .unwrap();

        // 重启后重建 fetcher/dedup/repo（全新实例），指针仍在 cache
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, xml))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(crawl_id),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("resumed feed");

        assert_eq!(stats.resumed_from, Some(3), "must seek past consumed=3");
        assert_eq!(stats.consumed, 10, "total consumed includes sought locs");
        assert_eq!(stats.enqueued, 7, "only locs 3..10 enqueue after resume");
        assert_eq!(
            task_repo.created_urls(),
            (3..10)
                .map(|i| format!("https://a.com/{i}"))
                .collect::<Vec<_>>()
        );
        // 续爬完成后指针删除
        assert!(!cache
            .store
            .lock()
            .contains_key(&sitemap_feed_pointer_key(crawl_id)));
    }

    /// 指针指向不同 sitemap（源已变）→ 忽略指针从头喂，dedup 兜底
    #[tokio::test]
    async fn feed_ignores_pointer_for_different_sitemap_url() {
        let locs: Vec<String> = vec!["https://a.com/x".into(), "https://a.com/y".into()];
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let crawl_id = Uuid::new_v4();
        let cache = MockCache::default();
        let pointer = SitemapFeedPointer {
            sitemap_url: "https://a.com/old-sitemap.xml".to_string(),
            consumed: 99,
        };
        cache
            .set(
                &sitemap_feed_pointer_key(crawl_id),
                &serde_json::to_string(&pointer).unwrap(),
                3600,
            )
            .await
            .unwrap();

        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(crawl_id),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        assert_eq!(
            stats.resumed_from, None,
            "mismatched pointer must be ignored"
        );
        assert_eq!(stats.enqueued, 2);
    }

    /// 损坏的指针（非法 JSON）→ 安全回退全量喂入
    #[tokio::test]
    async fn feed_survives_corrupt_pointer() {
        let locs: Vec<String> = vec!["https://a.com/only".into()];
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let crawl_id = Uuid::new_v4();
        let cache = MockCache::default();
        cache
            .set(&sitemap_feed_pointer_key(crawl_id), "{{not json", 3600)
            .await
            .unwrap();

        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(crawl_id),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("corrupt pointer must not fail the feed");

        assert_eq!(stats.resumed_from, None);
        assert_eq!(stats.enqueued, 1);
    }

    /// sitemap 404 → 合法状态：零入队非错误
    #[tokio::test]
    async fn feed_sitemap_404_is_legal_empty_result() {
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (404u16, "not found".into()))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("404 must be Ok (legal empty)");

        assert_eq!(stats.enqueued, 0);
        assert_eq!(stats.consumed, 0);
    }

    /// 5xx → 错误向上传播（失败显性化）
    #[tokio::test]
    async fn feed_5xx_propagates_error() {
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (500u16, "boom".into()))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let result = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await;

        assert!(result.is_err(), "5xx must propagate as error");
    }

    /// max_urls 护栏：sitemap 有 50 条、上限 10 → 只消费/入队 10 条
    #[tokio::test]
    async fn feed_respects_max_urls_cap() {
        let locs: Vec<String> = (0..50).map(|i| format!("https://a.com/{i}")).collect();
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let stats = feed_sitemap_as_tasks(
            Arc::new(fetcher),
            SITEMAP_URL.to_string(),
            options(10),
            None,
            None,
            vec![],
            seed_ctx(Uuid::new_v4()),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
            3600,
        )
        .await
        .expect("feed");

        assert_eq!(stats.enqueued, 10);
        assert_eq!(stats.consumed, 10);
    }

    /// 指针持久化：喂入过程至少 set 一次且记录总消费数；完成后 delete
    #[tokio::test]
    async fn feed_persists_pointer_with_consumed_progress() {
        let locs: Vec<String> = (0..7).map(|i| format!("https://a.com/{i}")).collect();
        let fetcher = StaticSitemapFetcher {
            responses: HashMap::from([(SITEMAP_URL.to_string(), (200u16, urlset_xml(&locs)))]),
        };
        let crawl_id = Uuid::new_v4();
        let task_repo = CountingTaskRepo::new();
        let crawl_repo = CountingCrawlRepo::default();
        let cache = MockCache::default();
        let dedup = Arc::new(parking_lot::RwLock::new(Deduplicator::new()));

        let _ = feed(
            fetcher,
            SITEMAP_URL,
            None,
            None,
            vec![],
            seed_ctx(crawl_id),
            &dedup,
            &task_repo,
            &crawl_repo,
            &cache,
        )
        .await
        .expect("feed");

        let key = sitemap_feed_pointer_key(crawl_id);
        assert!(
            cache.set_calls(&key) >= 1,
            "pointer must be persisted at least once"
        );
        // 完成后删除：指针生命周期 = 喂入过程
        assert!(cache.deleted.lock().contains(&key));
    }

    // ========== derive_sitemap_url ==========

    #[test]
    fn derive_sitemap_url_from_seed() {
        assert_eq!(
            derive_sitemap_url("https://a.com"),
            Some("https://a.com/sitemap.xml".to_string())
        );
        assert_eq!(
            derive_sitemap_url("https://a.com/some/page?q=1"),
            Some("https://a.com/sitemap.xml".to_string())
        );
        assert_eq!(
            derive_sitemap_url("http://localhost:8899"),
            Some("http://localhost:8899/sitemap.xml".to_string())
        );
        assert_eq!(derive_sitemap_url("not-a-url"), None);
    }

    /// 显式 sitemap_url 覆盖派生值（调用契约：sitemap_url=Some 时直接使用，
    /// 仅在缺位时才调 derive——crawl_task 挂接侧 or_else 语义）
    #[test]
    fn explicit_sitemap_url_takes_precedence_by_caller_contract() {
        let explicit = Some("https://cdn.b.com/sitemap-index.xml".to_string());
        let resolved = explicit
            .or_else(|| derive_sitemap_url("https://a.com"))
            .expect("explicit wins");
        assert_eq!(resolved, "https://cdn.b.com/sitemap-index.xml");
    }
}
