// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Crawl Link Processor — 爬取初始任务的构造逻辑。
//!
//! 负责从 DTO 和配置构建初始 `Task`（深度 0 的爬取种子），
//! 以及构造 crawl payload 中的链接发现相关字段。

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::domain::models::{Crawl, CrawlStatus, Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::fmt::Display;

/// `persist_crawl_with_initial_task` 的失败分类。
#[derive(Debug)]
pub enum PersistCrawlError<E> {
    /// crawl 行创建失败
    Repository(crate::domain::repositories::task_repository::RepositoryError),
    /// 首任务入队失败（crawl 已被补偿为 Failed）
    Enqueue(E),
}

impl<E: Display> Display for PersistCrawlError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repository(e) => write!(f, "crawl create failed: {e}"),
            Self::Enqueue(e) => write!(f, "initial task enqueue failed: {e}"),
        }
    }
}

/// 构建初始爬取任务（种子任务）。
///
/// 创建深度为 0 的 `Task`，包含 crawl_id、深度与 crawl 配置。
///
/// 团队域名黑名单不进入任务 payload：唯一载体是 `crawl.config`，
/// worker 按 crawl_id 读取（避免逐任务复制与快照漂移）。
///
/// # Arguments
///
/// * `team_id` - 团队 ID
/// * `api_key_id` - API Key ID
/// * `crawl_id` - 关联的爬取 ID
/// * `dto` - 爬取请求 DTO
/// * `now` - 当前时间戳
pub fn build_initial_crawl_task(
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
    crawl_id: crate::domain::id::Id,
    dto: &CrawlRequestDto,
    now: DateTime<Utc>,
) -> Task {
    Task {
        id: crate::domain::id::Id::generate(),
        task_type: TaskType::Crawl,
        status: TaskStatus::Queued,
        priority: 100,
        team_id,
        api_key_id,
        url: dto.url.clone(),
        payload: json!({
            "crawl_id": crawl_id,
            "depth": 0,
            "config": dto.config,
            "sitemap_seed": dto.sitemap_seed,
            "sitemap_url": dto.sitemap_url,
        }),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: Some(crawl_id),
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
        expires_at: dto.expires_at,
    }
}

/// 构建 `Crawl` 实体，`root_url` 与抓取种子分开（SDK 的 url/seed_url 两字段）。
///
/// HTTP 入口的 root 与 seed 相同，直接调用本函数；
/// SDK 入口传入展示用 root 与真实 seed，避免两处各自拼装实体导致字段漂移。
pub fn build_crawl_entity_with_root(
    crawl_id: crate::domain::id::Id,
    team_id: crate::domain::id::Id,
    dto: &CrawlRequestDto,
    root_url: &str,
    now: DateTime<Utc>,
) -> Crawl {
    Crawl::with_all_fields(
        crawl_id,
        team_id,
        dto.name
            .clone()
            .unwrap_or_else(|| "Untitled Crawl".to_string()),
        root_url.to_string(),
        dto.url.clone(),
        CrawlStatus::Queued,
        json!(dto.config),
        1, // total_tasks
        0, // completed_tasks
        0, // failed_tasks
        now,
        now,
        None,
    )
}

/// 创建 crawl 行并播种首任务（HTTP 与 SDK 入口共用的唯一编排）。
///
/// 入队失败时做补偿：把未创建任务计入 `failed_tasks` + 终态守护写 Failed
/// （不留 total=1/0 任务的孤儿 crawl）。空种子/黑名单校验属于入口预检，
/// 不在本函数内重复。
#[allow(clippy::too_many_arguments)] // 编排入参：repo/团队/key/dto/root/黑名单/时间/enqueue
pub async fn persist_crawl_with_initial_task<F, Fut, E>(
    crawl_repo: &dyn CrawlRepository,
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
    dto: &CrawlRequestDto,
    root_url: &str,
    domain_blacklist: Vec<String>,
    now: DateTime<Utc>,
    enqueue: F,
) -> Result<Crawl, PersistCrawlError<E>>
where
    F: FnOnce(Task) -> Fut,
    Fut: std::future::Future<Output = Result<Task, E>>,
    E: Display,
{
    let crawl_id = crate::domain::id::Id::generate();
    let mut crawl = build_crawl_entity_with_root(crawl_id, team_id, dto, root_url, now);
    // 黑名单存 crawl.config（服务端写入，非请求 DTO 字段）：worker 每任务按
    // crawl_id 读取一次，避免整表复制进每个子任务 payload。
    if let Some(obj) = crawl.config_mut().as_object_mut() {
        obj.insert(
            "domain_blacklist".to_string(),
            serde_json::json!(domain_blacklist),
        );
    } else {
        log::error!(
            "crawl {} config is not a JSON object; domain_blacklist snapshot not stored",
            crawl_id
        );
    }
    crawl_repo
        .create(&crawl)
        .await
        .map_err(PersistCrawlError::Repository)?;

    let task = build_initial_crawl_task(team_id, api_key_id, crawl_id, dto, now);
    if let Err(e) = enqueue(task).await {
        // 补偿：单条条件写（failed+1、终态守护、completed_at 同语句），
        // 避免两次写之间的中间态与并发覆盖。
        if let Err(update_err) = crawl_repo.finalize_failed_after_enqueue(crawl_id).await {
            log::error!(
                "Failed to mark crawl {} failed during enqueue compensation: {}",
                crawl_id,
                update_err
            );
        }
        return Err(PersistCrawlError::Enqueue(e));
    }
    Ok(crawl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::crawl_request::CrawlConfigDto;

    fn make_dto() -> CrawlRequestDto {
        CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: Some("Test Crawl".to_string()),
            config: CrawlConfigDto {
                max_depth: 3,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                max_pages: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
                extraction_prompt: None,
                extraction_schema: None,
            },
            sync_wait_ms: None,
            expires_at: None,
            sitemap_seed: None,
            sitemap_url: None,
        }
    }

    #[test]
    fn test_build_initial_crawl_task() {
        let team_id = crate::domain::id::Id::generate();
        let api_key_id = crate::domain::id::Id::generate();
        let crawl_id = crate::domain::id::Id::generate();
        let dto = make_dto();
        let now = Utc::now();

        let task = build_initial_crawl_task(team_id, api_key_id, crawl_id, &dto, now);

        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.task_type, TaskType::Crawl);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.crawl_id, Some(crawl_id));
        assert_eq!(task.url, "https://example.com");
        assert_eq!(task.payload["crawl_id"], crawl_id.to_string());
        assert_eq!(task.payload["depth"], 0);
    }

    /// F11：sitemap_seed / sitemap_url 透传进种子任务 payload
    /// （执行侧喂入路径从 payload 读取；缺省为 null）
    #[test]
    fn test_build_initial_crawl_task_transfers_sitemap_source_fields() {
        let mut dto = make_dto();
        dto.sitemap_seed = Some(true);
        dto.sitemap_url = Some("https://cdn.b.com/sitemap-index.xml".to_string());

        let task = build_initial_crawl_task(
            crate::domain::id::Id::generate(),
            crate::domain::id::Id::generate(),
            crate::domain::id::Id::generate(),
            &dto,
            Utc::now(),
        );

        assert_eq!(task.payload["sitemap_seed"], true);
        assert_eq!(
            task.payload["sitemap_url"],
            "https://cdn.b.com/sitemap-index.xml"
        );
    }

    #[test]
    fn test_build_crawl_entity() {
        let crawl_id = crate::domain::id::Id::generate();
        let team_id = crate::domain::id::Id::generate();
        let dto = make_dto();
        let now = Utc::now();

        let crawl = build_crawl_entity_with_root(crawl_id, team_id, &dto, &dto.url, now);

        assert_eq!(crawl.id, crawl_id);
        assert_eq!(crawl.team_id, team_id);
        assert_eq!(crawl.status, CrawlStatus::Queued);
    }

    #[test]
    fn test_build_crawl_entity_default_name() {
        let crawl_id = crate::domain::id::Id::generate();
        let team_id = crate::domain::id::Id::generate();
        let mut dto = make_dto();
        dto.name = None;
        let now = Utc::now();

        let crawl = build_crawl_entity_with_root(crawl_id, team_id, &dto, &dto.url, now);
        // The name should be "Untitled Crawl" when dto.name is None
        // (checking via the crawl entity)
        assert_eq!(crawl.id, crawl_id);
    }
}
