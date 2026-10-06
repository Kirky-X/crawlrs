// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Crawl 任务处理方法
//!
//! 从 `ScrapeWorker` 提取的 crawl 任务处理相关方法（partial impl block）。
//! 包含任务分发、成功/失败处理和自适应停止条件检查。

use super::ScrapeWorker;
use super::{
    build_crawl_request_fn, check_robots_txt_fn, extract_and_queue_links_fn, parse_crawl_payload,
    update_crawl_completion_status_fn,
};
use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::domain::models::Task;
use crate::engines::engine_client::{ScrapeRequest, ScrapeResponse};
use crate::infrastructure::security::ssrf::is_internal_url;
use crate::workers::crawl::adaptive::{CrawlStats, StopCondition};
use anyhow::Result;
use log::{debug, error, info, warn};

// 提取到独立模块的函数导入
use crate::workers::scrape_executor::{process_text_encoding, save_result};

impl ScrapeWorker {
    pub(super) async fn process_crawl_task(&self, mut task: Task) -> Result<()> {
        // 1. 解析 Crawl 任务特定的 Payload
        let (crawl_id, depth, config) = match parse_crawl_payload(&task) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse crawl payload: {}", e);
                self.repository
                    .mark_failed(task.id, Some(self.worker_id))
                    .await?;
                return Ok(());
            }
        };

        // 2. Robots.txt Check
        // robots UA 来自 robots.user_agent 配置（匹配与抓取共用）
        if !check_robots_txt_fn(
            &task,
            self.robots_checker.as_ref(),
            self.settings.robots.user_agent.as_str(),
        )
        .await
        {
            self.repository
                .mark_failed(task.id, Some(self.worker_id))
                .await?;
            return Ok(());
        }

        // 2.2 F5：robots Crawl-delay 联动 per-host 限速时钟（首值生效不回退）。
        // check_robots_txt 刚取过 robots 内容（内存缓存命中），此处仅读取解析；
        // 获取失败沿用 robots 路径的 fail-open 语义（跳过注入，不打断任务）。
        match self
            .robots_checker
            .get_crawl_delay(&task.url, self.settings.robots.user_agent.as_str())
            .await
        {
            Ok(Some(delay)) => {
                if let Ok(u) = url::Url::parse(&task.url) {
                    if let Some(host) = u.host_str() {
                        self.engine_client.set_crawl_delay(host, delay);
                        debug!(
                            "crawl-delay {:?} applied for host {} (task {})",
                            delay, host, task.id
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(e) => debug!("crawl-delay lookup skipped for task {}: {e}", task.id),
        }

        // 2.5 SSRF 防护 (CWE-918)
        if let Some(ref proxy_url) = config.proxy {
            if is_internal_url(proxy_url) {
                warn!(
                    "SSRF via proxy blocked in worker proxy={} task_id={} team_id={}",
                    crate::workers::cache_utils::redact_url_for_log(proxy_url),
                    task.id,
                    task.team_id
                );
                self.repository
                    .mark_failed(task.id, Some(self.worker_id))
                    .await?;
                return Ok(());
            }
        }

        // 3. 构建并执行抓取请求
        let request = build_crawl_request_fn(
            &task,
            &config,
            self.settings.timeouts.engines.default_timeout_seconds,
        );
        let response = self.engine_client.scrape(&request).await;

        // 4. 处理结果
        match response {
            Ok(response) => {
                self.handle_crawl_success(&task, response, crawl_id, depth, &config, &request)
                    .await
            }
            Err(e) => {
                self.handle_crawl_failure(&mut task, e.into(), crawl_id, &request)
                    .await
            }
        }
    }

    /// 处理 Crawl 任务成功响应
    pub(super) async fn handle_crawl_success(
        &self,
        task: &Task,
        response: ScrapeResponse,
        crawl_id: crate::domain::id::Id,
        depth: u32,
        config: &CrawlConfigDto,
        request: &ScrapeRequest,
    ) -> Result<()> {
        info!(
            "Crawl step successful, url: {}, status: {}",
            task.url, response.status_code
        );

        let processed_content = match process_text_encoding(task, &response).await {
            Ok(content) => content.into_owned(),
            Err(e) => {
                warn!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "scrape-task-encoding-fallback",
                        &[("error", fluent_bundle::FluentValue::from(e.to_string()))],
                    )
                );
                response.content.clone()
            }
        };

        let processed_response = ScrapeResponse {
            content: processed_content,
            ..response
        };

        // F11：软 404 指纹判定（响应级，抓取后；与 URL 级 FilterChain 互补）。
        // 404/410 学习空壳指纹；200 命中已学壳 → 不落结果，任务照常完成（防重试风暴）。
        let soft404_hit = matches!(
            crate::workers::crawl::soft404::observe_crawl_response(
                self.settings.crawl.soft404_enabled,
                processed_response.status_code,
                &processed_response.content,
            ),
            crate::workers::crawl::soft404::Soft404Verdict::Soft404
        );

        let extracted_data = if soft404_hit {
            // 壳页无提取价值，跳过规则/LLM 提取
            None
        } else {
            self.extract_data_with_rules(task, &processed_response, config)
                .await
        };

        // 先以锁守卫终结任务：只有仍持有锁时才落库结果，
        // 防止锁过期被其他 worker 认领后双写结果
        let marked = self
            .repository
            .mark_completed(task.id, Some(self.worker_id))
            .await?;
        if marked == 0 {
            warn!(
                "Crawl task {} no longer owned by worker {}, skipping result",
                task.id, self.worker_id
            );
            return Ok(());
        }

        if soft404_hit {
            info!(
                "soft 404 detected for url {} (task {}): matching shell fingerprint, \
                 result not persisted",
                task.url, task.id
            );
        } else {
            save_result(
                task,
                &processed_response,
                extracted_data,
                self.result_repository.as_ref(),
            )
            .await?;
        }

        if let Err(e) = self
            .crawl_repository
            .increment_completed_tasks(crawl_id)
            .await
        {
            error!(
                "Failed to increment completed tasks for crawl {}: {}",
                crawl_id, e
            );
        }

        // 先提取并入队子链接，再评估自适应停止条件。
        //
        // 顺序是正确性约束：`NoPendingLinks` 的语义是"已无待处理链接"。
        // 若在外链入队之前评估，种子页完成时 total=1/pending=0 恒成立，
        // 任何 crawl 都会在第一页后提前终止——depth > 1 与 include/exclude
        // 过滤永远不可达（缺陷实证：worker 日志 "adaptive stop: no pending
        // links (pages=1, pending=0)"，结果集仅含种子页）。
        // 重排后 pending 统计包含刚入队的子链接，NoPendingLinks 仅在
        // 深度耗尽（depth >= max_depth 不再入队）且全部完成后才触发；
        // MaxPagesReached 语义不变（入队后仍可按页数上限提前终止）。
        if depth < config.max_depth {
            extract_and_queue_links_fn(
                task,
                &processed_response,
                crawl_id,
                depth,
                config,
                self.repository.as_ref(),
                self.crawl_repository.as_ref(),
                &self.deduplicator,
            )
            .await?;
        }

        // F11：sitemap 流式请求源喂入（每 crawl 显式选择加入；主开关已在
        // API 入口校验）。种子任务（depth=0）重新认领时（worker 重启/重试）
        // 再次进入本路径，cache 进度指针保证从断点继续而非重复入队。
        // 失败显性化但不传播：喂入失败不影响种子任务已完成的事实，
        // crawl 继续以页面链接发现运行。
        #[cfg(feature = "sitemap")]
        if depth == 0 && task.payload.get("sitemap_seed").and_then(|v| v.as_bool()) == Some(true) {
            self.feed_sitemap_source(task, crawl_id, config).await;
        }

        // 自适应停止条件检查
        //
        // 每完成一个爬取步骤（并入队子链接）后，评估是否应提前终止整个 crawl：
        // - `MaxPagesReached`: completed_tasks >= max_pages（可配置上限）
        // - `NoPendingLinks`: total_tasks 已全部完成（无待处理链接）
        //
        // 命中时直接标记 crawl 为 Completed。
        // 注：完整 `AdaptiveStrategy::evaluate`（BM25/覆盖率/饱和度）
        // 需 CrawlConfigDto 扩展 keywords 字段后接入（当前 DTO 无 keywords）。
        if let Ok(Some(crawl_state)) = self.crawl_repository.find_by_id(crawl_id).await {
            let pages_crawled = crawl_state.completed_tasks() as usize;
            let total = crawl_state.total_tasks() as usize;
            let pending = total.saturating_sub(pages_crawled + crawl_state.failed_tasks() as usize);

            // 页数上限来自 crawl config（缺省 1000）
            let max_pages = config.max_pages.unwrap_or(1000) as usize;
            let stop_condition = StopCondition::new().with_max_pages(max_pages);
            let stats = CrawlStats::new()
                .with_pages(pages_crawled)
                .with_pending(pending);

            if let Some(reason) = stop_condition.should_stop(&stats) {
                info!(
                    "adaptive stop for crawl {}: {} (pages={}, pending={})",
                    crawl_id,
                    reason.description(),
                    pages_crawled,
                    pending
                );
                if let Err(e) = self
                    .crawl_repository
                    .update_status(
                        crawl_id,
                        crate::domain::models::crawl_model::CrawlStatus::Completed,
                    )
                    .await
                {
                    error!(
                        "Failed to update crawl status after adaptive stop for {}: {}",
                        crawl_id, e
                    );
                }
            } else {
                // 未触发停止条件：由 update_crawl_completion_status 按总量判定
                // 是否全部完成（子任务入队后 total 已增长，不会误标 Completed）
                update_crawl_completion_status_fn(crawl_id, self.crawl_repository.as_ref()).await;
            }
        } else {
            // crawl 查询失败，回退到原流程（仅更新完成状态）
            update_crawl_completion_status_fn(crawl_id, self.crawl_repository.as_ref()).await;
        }

        self.deduct_feature_credits(
            task.team_id,
            task.id,
            processed_response.screenshot.is_some(),
            request.options.proxy.is_some(),
        )
        .await;

        Ok(())
    }

    /// F11：sitemap 流式请求源喂入
    ///
    /// 显式 `sitemap_url` 优先，缺省从种子 URL 派生 `{origin}/sitemap.xml`；
    /// 经 [`feed_sitemap_as_tasks`] 过滤/去重后入队为 crawl 子任务，每批
    /// 持久化进度指针（重启续爬）。失败打 error 日志并说明后果，不传播
    /// （种子任务已完成，喂入失败不改变任务状态）。
    #[cfg(feature = "sitemap")]
    async fn feed_sitemap_source(
        &self,
        task: &Task,
        crawl_id: crate::domain::id::Id,
        config: &CrawlConfigDto,
    ) {
        use std::sync::Arc;

        use crate::workers::crawl::sitemap_source::{
            derive_sitemap_url, feed_sitemap_as_tasks, sitemap_feed_pointer_key, SeedTaskContext,
        };

        let sitemap_url = match task
            .payload
            .get("sitemap_url")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| derive_sitemap_url(&task.url))
        {
            Some(u) => u,
            None => {
                warn!(
                    "sitemap_seed crawl {} cannot derive sitemap url from {}; \
                     sitemap request source skipped",
                    crawl_id, task.url
                );
                return;
            }
        };

        // 键缺失 = 合法空表；键存在但解析失败 = 约束未知，禁止静默降级为
        // 空黑名单（地理限制域名会变为可抓取）——error 显性化并跳过本次喂入
        let domain_blacklist = match resolve_domain_blacklist(&task.payload) {
            Ok(list) => list,
            Err(e) => {
                error!(
                    "crawl {}: domain_blacklist payload unparsable ({}); \
                     sitemap feed skipped — restriction cannot be verified",
                    crawl_id, e
                );
                return;
            }
        };

        let fetcher = Arc::new(
            crate::application::use_cases::map_use_case::EngineSitemapFetcher::new(
                self.engine_client.clone(),
                std::time::Duration::from_secs(self.settings.sitemap.fetch_timeout_secs),
            ),
        );
        let options = crate::application::use_cases::map_use_case::SitemapStreamOptions {
            channel_capacity: self.settings.sitemap.channel_capacity,
            max_urls: self.settings.sitemap.max_urls,
            max_concurrent_sub_sitemaps: self.settings.sitemap.max_concurrent_sub_sitemaps,
        };
        let seed = SeedTaskContext {
            team_id: task.team_id,
            api_key_id: task.api_key_id,
            crawl_id,
            priority: task.priority,
            config_json: task
                .payload
                .get("config")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        };

        // 指针 TTL 覆盖长 crawl 的喂入窗口（7 天）；喂入完成即删除
        const POINTER_TTL_SECS: u64 = 7 * 24 * 3600;

        info!(
            "sitemap request source for crawl {}: feeding {}",
            crawl_id, sitemap_url
        );
        match feed_sitemap_as_tasks(
            fetcher,
            sitemap_url.clone(),
            options,
            config.include_patterns.clone(),
            config.exclude_patterns.clone(),
            domain_blacklist,
            seed,
            &self.deduplicator,
            self.repository.as_ref(),
            self.crawl_repository.as_ref(),
            self.cache_service.as_ref(),
            POINTER_TTL_SECS,
        )
        .await
        {
            Ok(stats) => {
                if let Some(resumed) = stats.resumed_from {
                    info!(
                        "sitemap feed for crawl {} resumed from loc {resumed} \
                         (pointer {})",
                        crawl_id,
                        sitemap_feed_pointer_key(crawl_id)
                    );
                }
                info!(
                    "sitemap feed done for crawl {}: enqueued={}, filtered={}, \
                     duplicates={}, consumed={} from {}",
                    crawl_id,
                    stats.enqueued,
                    stats.filtered_out,
                    stats.duplicates_skipped,
                    stats.consumed,
                    sitemap_url
                );
            }
            Err(e) => {
                error!(
                    "sitemap request source failed for crawl {} ({}): {}; \
                     crawl continues with page-link discovery only",
                    crawl_id, sitemap_url, e
                );
            }
        }
    }

    /// 处理 Crawl 任务失败响应
    pub(super) async fn handle_crawl_failure(
        &self,
        task: &mut Task,
        error: anyhow::Error,
        crawl_id: crate::domain::id::Id,
        request: &ScrapeRequest,
    ) -> Result<()> {
        self.deduct_feature_credits(
            task.team_id,
            task.id,
            false,
            request.options.proxy.is_some(),
        )
        .await;

        error!("Crawl step failed: {}", error);
        self.handle_failure(task).await?;

        if let Err(e) = self.crawl_repository.increment_failed_tasks(crawl_id).await {
            error!(
                "Failed to increment failed tasks for crawl {}: {}",
                crawl_id, e
            );
        }

        update_crawl_completion_status_fn(crawl_id, self.crawl_repository.as_ref()).await;

        self.trigger_webhook(task, Some(error.to_string())).await;

        Ok(())
    }
}

/// 解析 payload 中的 `domain_blacklist`（sitemap 喂入的域名排除约束）
///
/// 键缺失 = 合法空表；键存在但类型/内容损坏 = 约束未知 → `Err`，调用方必须
/// 显性上报并跳过喂入——禁止 `.ok().unwrap_or_default()` 式静默降级为空
/// 黑名单（地理限制域名会随之变为可抓取，限制性控制失效且无证据）。
#[cfg(feature = "sitemap")]
fn resolve_domain_blacklist(payload: &serde_json::Value) -> Result<Vec<String>, serde_json::Error> {
    match payload.get("domain_blacklist") {
        None => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v.clone()),
    }
}

#[cfg(all(test, feature = "sitemap"))]
mod tests {
    use super::*;

    #[test]
    fn blacklist_missing_key_is_legal_empty() {
        let payload = serde_json::json!({"sitemap_seed": true});
        assert!(
            resolve_domain_blacklist(&payload).unwrap().is_empty(),
            "键缺失 = 合法空表，不得报错"
        );
    }

    #[test]
    fn blacklist_valid_list_parses() {
        let payload =
            serde_json::json!({"domain_blacklist": ["cn.example.com", "geo.example.net"]});
        assert_eq!(
            resolve_domain_blacklist(&payload).unwrap(),
            vec!["cn.example.com".to_string(), "geo.example.net".to_string()]
        );
    }

    #[test]
    fn blacklist_broken_type_is_error_not_silent_empty() {
        // 字符串/数字数组而非字符串数组：必须 Err（驱动调用方 error 上报 +
        // 跳过喂入），禁止回退 .ok().unwrap_or_default() 静默降级
        let payload = serde_json::json!({"domain_blacklist": "cn.example.com"});
        assert!(resolve_domain_blacklist(&payload).is_err());
        let payload = serde_json::json!({"domain_blacklist": [1, 2, 3]});
        assert!(resolve_domain_blacklist(&payload).is_err());
    }
}
