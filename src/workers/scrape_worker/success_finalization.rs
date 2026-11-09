// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 成功收尾与失败处理 — 从 `ScrapeWorker` 拆分的 partial impl block
//!
//! 包含抓取成功收尾（结果落库 + 任务终结 + webhook）、webhook 触发与
//! 失败重试分派。

use super::*;

impl ScrapeWorker {
    pub(super) async fn handle_scrape_success(
        &self,
        task: &Task,
        scrape_request_dto: Option<&ScrapeRequestDto>,
        response: ScrapeResponse,
    ) -> Result<bool> {
        debug!("task_id: {}", task.id);

        // 文本编码处理 - 集成文本处理功能
        // process_text_encoding 返回 Cow<'_, str>，禁用路径零 clone
        let processed_content = Self::process_content_with_fallback(task, &response).await;

        // PERF- 复用 process_scrape_task 已解析的 ScrapeRequestDto 引用，
        // 不再在 handle_scrape_success 内部二次 from_value(task.payload.clone())。
        let parsed_req = scrape_request_dto;

        // 创建处理后的响应用于后续处理
        // 调用 MarkdownPostProcessor（独立组件）
        // 若 formats 含 "markdown" 则生成 Markdown，否则返回 Ok(None)
        //
        // （错误显性化）：generate() 现返回 Result<Option<String>, _>，
        // 区分"未请求 markdown"（Ok(None)）与"转换失败/空结果"（Err）。
        // 调用方策略：markdown 为增强字段，失败不阻断基础抓取结果，
        // 错误时记录告警并继续（generated_markdown = None）。
        // generate() 改为 async，支持 only_main_content 前置正文提取
        let generated_markdown = self
            .generate_markdown_or_warn(task, parsed_req, &processed_content)
            .await;

        // handle_scrape_success 改为 owned ScrapeResponse，
        // 构造 processed_response 时直接 move 字段，避免 clone screenshot(100KB+)/headers/...
        let processed_response = ScrapeResponse {
            js_degraded: response.js_degraded,
            content: processed_content,
            status_code: response.status_code,
            screenshot: response.screenshot,
            content_type: response.content_type,
            headers: response.headers,
            response_time_ms: response.response_time_ms,
            final_url: response.final_url,
            markdown: generated_markdown.or(response.markdown),
            captured_responses: response.captured_responses,
        };

        // Token 扣费延后：先收集用量，仅在赢得 mark_completed 守卫后统一扣除，
        // 防止丢守卫的陈旧 worker 与认领方对同一次任务双扣费
        let (extracted_data, pending_token_usages) = self
            .run_extraction_stage(task, parsed_req, &processed_response.content)
            .await;

        // 先落库结果（save_result 按 task_id 幂等 upsert），再以锁守卫终结任务：
        // 中间崩溃的窗口退化为"结果已存、任务可被僵尸恢复重跑"，重跑后幂等覆盖
        // 不会产生双结果；而旧顺序（先 Completed 后存结果）中间崩溃会留下
        // "任务 Completed 但结果永久缺失"的不可恢复状态。
        save_result(
            task,
            &processed_response,
            extracted_data,
            self.result_repository.as_ref(),
        )
        .await?;
        let marked = self
            .repository
            .mark_completed(task.id, Some(self.worker_id))
            .await?;
        if marked == 0 {
            warn!(
                "Task {} no longer owned by worker {}, skipping token/feature credit \
                 deduction and webhook delivery",
                task.id, self.worker_id
            );
            return Ok(false);
        }

        // 守卫胜利：统一执行延后的 token 扣费（仅本 worker 认领期间产生的用量）
        for usage in &pending_token_usages {
            self.deduct_token_credits(task.team_id, task.id, usage, "Tokens used for extraction")
                .await;
        }
        debug!(
            "task_id: {}, Successfully marked task as completed",
            task.id
        );

        self.trigger_webhook(task, None).await;
        Ok(true)
    }

    /// 文本编码处理并保留失败回退（自 [`Self::handle_scrape_success`] 提取）：
    /// `process_text_encoding` 返回 `Cow<'_, str>`，禁用路径零 clone；
    /// 失败时 warn 并回退原始 content。
    async fn process_content_with_fallback(task: &Task, response: &ScrapeResponse) -> String {
        match process_text_encoding(task, response).await {
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
        }
    }

    /// 按请求生成 Markdown；失败降级为 `None` 并告警（自
    /// [`Self::handle_scrape_success`] 提取）。
    #[cfg(feature = "content")]
    async fn generate_markdown_or_warn(
        &self,
        task: &Task,
        parsed_req: Option<&ScrapeRequestDto>,
        processed_content: &str,
    ) -> Option<String> {
        if let Some(req) = parsed_req {
            self.markdown_post_processor
                .generate(task.id, req, processed_content)
                .await
                .unwrap_or_else(|e| {
                    warn!(
                        "task_id: {}, markdown post-processing failed: {}",
                        task.id, e
                    );
                    None
                })
        } else {
            None
        }
    }

    /// content 特性关闭时不生成 Markdown（自
    /// [`Self::handle_scrape_success`] 提取）。
    #[cfg(not(feature = "content"))]
    async fn generate_markdown_or_warn(
        &self,
        _task: &Task,
        _parsed_req: Option<&ScrapeRequestDto>,
        _processed_content: &str,
    ) -> Option<String> {
        None
    }

    // `maybe_generate_markdown` 方法已迁移至 `MarkdownPostProcessor`（独立组件）。
    // 原 `HtmdMarkdownService` 调用已替换为 `self.markdown_post_processor.generate(...)`，
    // 由 `handle_scrape_success` 中调用。

    pub(super) async fn trigger_webhook(&self, task: &Task, error_msg: Option<String>) {
        let result = match error_msg {
            Some(msg) => self.webhook_service.trigger_failure(task, msg).await,
            None => self.webhook_service.trigger_completion(task).await,
        };

        if let Err(e) = result {
            error!("Failed to trigger webhook for task {}: {}", task.id, e);
        }
    }

    /// 返回重试器结论：`Retried` 表示任务已回队（**不是终态**），
    /// `Failed` 表示重试预算耗尽/不可重试（终态）。crawl 层据此只在
    /// 终态失败时累加 failed_tasks，避免把“首次可重试失败”当终态
    /// （会提前把 crawl 标 Failed 并取消重试）。
    pub(super) async fn handle_failure(
        &self,
        task: &mut Task,
        error: &str,
    ) -> Result<crate::domain::services::retry_handler::HandleFailureResult> {
        match self.retry_handler.handle_failure(task, error).await {
            outcome @ crate::domain::services::retry_handler::HandleFailureResult::Retried {
                ..
            } => Ok(outcome),
            outcome @ crate::domain::services::retry_handler::HandleFailureResult::Failed => {
                Ok(outcome)
            }
            outcome @ crate::domain::services::retry_handler::HandleFailureResult::Stale => {
                Ok(outcome)
            }
            crate::domain::services::retry_handler::HandleFailureResult::Error(e) => Err(e),
        }
    }
}
