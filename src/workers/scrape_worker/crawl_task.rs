// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
use log::{error, info, warn};
use uuid::Uuid;

// T026 拆分：提取到独立模块的函数导入
use crate::workers::scrape_executor::{process_text_encoding, save_result};

impl ScrapeWorker {
    pub(super) async fn process_crawl_task(&self, mut task: Task) -> Result<()> {
        // 1. 解析 Crawl 任务特定的 Payload
        let (crawl_id, depth, config) = match parse_crawl_payload(&task) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse crawl payload: {}", e);
                self.repository.mark_failed(task.id).await?;
                return Ok(());
            }
        };

        // 2. Robots.txt Check
        if !check_robots_txt_fn(&task, self.robots_checker.as_ref()).await {
            self.repository.mark_failed(task.id).await?;
            return Ok(());
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
                self.repository.mark_failed(task.id).await?;
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
        crawl_id: Uuid,
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
                warn!("文本编码处理失败，使用原始内容: {}", e);
                response.content.clone()
            }
        };

        let processed_response = ScrapeResponse {
            content: processed_content,
            ..response
        };

        let extracted_data = self
            .extract_data_with_rules(task, &processed_response, config)
            .await;

        save_result(
            task,
            &processed_response,
            extracted_data,
            self.result_repository.as_ref(),
        )
        .await?;

        self.repository.mark_completed(task.id).await?;
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

        // T067/R-frontier-004：自适应停止条件检查
        //
        // 每完成一个爬取步骤后，评估是否应提前终止整个 crawl：
        // - `MaxPagesReached`: completed_tasks >= max_pages（可配置上限）
        // - `NoPendingLinks`: total_tasks 已全部完成（无待处理链接）
        //
        // 命中时直接标记 crawl 为 Completed，跳过后续链接提取。
        // 注：完整 `AdaptiveStrategy::evaluate`（BM25/覆盖率/饱和度）
        // 需 CrawlConfigDto 扩展 keywords 字段后接入（当前 DTO 无 keywords）。
        if let Ok(Some(crawl_state)) = self.crawl_repository.find_by_id(crawl_id).await {
            let pages_crawled = crawl_state.completed_tasks() as usize;
            let total = crawl_state.total_tasks() as usize;
            let pending = total.saturating_sub(pages_crawled + crawl_state.failed_tasks() as usize);

            // 可配置上限：后续从 CrawlConfigDto.max_pages 读取，当前用 1000 兜底
            let max_pages = 1000usize;
            let stop_condition = StopCondition::new().with_max_pages(max_pages);
            let stats = CrawlStats::new()
                .with_pages(pages_crawled)
                .with_pending(pending);

            if let Some(reason) = stop_condition.should_stop(&stats) {
                info!(
                    "T067: adaptive stop for crawl {}: {} (pages={}, pending={})",
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
                // 未触发停止条件，继续正常流程
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
                update_crawl_completion_status_fn(crawl_id, self.crawl_repository.as_ref()).await;
            }
        } else {
            // crawl 查询失败，回退到原流程（继续提取链接 + 更新状态）
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

    /// 处理 Crawl 任务失败响应
    pub(super) async fn handle_crawl_failure(
        &self,
        task: &mut Task,
        error: anyhow::Error,
        crawl_id: Uuid,
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
