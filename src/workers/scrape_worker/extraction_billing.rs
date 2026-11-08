// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 提取与计费 — 从 `ScrapeWorker` 拆分的 partial impl block
//!
//! 唯一提取调度：[`ScrapeWorker::dispatch_extraction`] 按 rules / prompt /
//! schema 口径执行提取，并以显式结算策略（Immediate / Deferred）决定扣费
//! 时机；成功收尾与失败处理见 [`super::success_finalization`]。

use super::*;

/// 单次提取的输入口径（三选一；候选优先级由调用方按各自既有语义决定）
pub(super) enum ExtractionSpec<'a> {
    /// 显式规则（`extraction_rules`）
    Rules(&'a HashMap<String, crate::domain::services::extraction_service::ExtractionRule>),
    /// Prompt（包装为单条 LLM 规则，`extraction_prompt`）
    Prompt(&'a str),
    /// Schema（LLM schema 抽取，经 RAG 自动增强入口，`extraction_schema`）
    Schema(&'a Value),
}

/// 提取成功后的结算策略（显式化三种任务路径的既有扣费时机）
pub(super) enum ExtractionSettlement<'a> {
    /// 提取成功即扣费（crawl / extract 路径的既有扣费点）
    Immediate { description: &'a str },
    /// 仅记录 token 用量，扣费由调用方在守卫胜利后统一执行（scrape 路径）
    Deferred {
        /// 待扣费用量累积器（每条提取的用量独立入列）
        usage_acc: &'a mut Vec<crate::domain::services::llm::TokenUsage>,
    },
}

impl ExtractionSpec<'_> {
    /// 提取失败日志；文案逐字沿用三条既有路径
    pub(super) fn log_failure(&self, url: &str, err: &anyhow::Error) {
        match self {
            Self::Rules(_) => error!("Extraction failed for url {}: {}", url, err),
            Self::Prompt(_) => error!("Prompt extraction failed for url {}: {}", url, err),
            Self::Schema(_) => error!("Schema extraction failed for url {}: {}", url, err),
        }
    }
}

impl ScrapeWorker {
    /// 唯一提取调度：按 `spec` 执行一次提取；成功后按 `settlement` 立即扣费
    /// 或只记录用量（Deferred）。
    ///
    /// 提取失败时返回 `Err` 且不写日志，由调用方沿用既有语义处理：
    /// - scrape / crawl：`ExtractionSpec::log_failure` 记 error 后降级为 `None`
    /// - extract：`?` 向上传播
    pub(super) async fn dispatch_extraction(
        &self,
        task: &Task,
        content: &str,
        base_url: &str,
        spec: &ExtractionSpec<'_>,
        settlement: ExtractionSettlement<'_>,
    ) -> Result<Value> {
        let (data, usage) = match spec {
            ExtractionSpec::Rules(rules) => {
                self.extraction_service
                    .extract(content, rules, Some(base_url))
                    .await
            }
            ExtractionSpec::Prompt(prompt) => {
                self.extract_with_prompt(content, prompt, base_url).await
            }
            ExtractionSpec::Schema(schema) => self.extract_with_schema(content, schema).await,
        }?;

        match settlement {
            // 立即结算：保持 crawl / extract 路径既有扣费点（提取成功后、
            // 落库/守卫之前）
            ExtractionSettlement::Immediate { description } => {
                self.deduct_token_credits(task.team_id, task.id, &usage, description)
                    .await;
            }
            // 仅记录用量；DB 扣费由调用方在守卫胜利后执行
            ExtractionSettlement::Deferred { usage_acc } => {
                self.record_extraction_usage(task, usage, usage_acc);
            }
        }

        Ok(data)
    }

    /// 使用配置的规则提取数据（支持 rules > prompt > schema 优先级）
    ///
    /// Crawl 路径：提取成功后立即扣费（Immediate 结算，保持既有扣费点），
    /// 提取失败记 error 日志后返回 `None`。
    pub(super) async fn extract_data_with_rules(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        config: &CrawlConfigDto,
    ) -> Option<Value> {
        let (spec, description) = if let Some(rules) = &config.extraction_rules {
            (ExtractionSpec::Rules(rules), "Tokens used for extraction")
        } else if let Some(prompt) = &config.extraction_prompt {
            if prompt.is_empty() {
                return None;
            }
            (
                ExtractionSpec::Prompt(prompt.as_str()),
                "Tokens used for prompt extraction",
            )
        } else if let Some(schema) = &config.extraction_schema {
            (
                ExtractionSpec::Schema(schema),
                "Tokens used for schema extraction",
            )
        } else {
            return None;
        };

        match self
            .dispatch_extraction(
                task,
                &response.content,
                &task.url,
                &spec,
                ExtractionSettlement::Immediate { description },
            )
            .await
        {
            Ok(data) => Some(data),
            Err(e) => {
                spec.log_failure(&task.url, &e);
                None
            }
        }
    }

    /// 使用 Prompt 提取数据（共享辅助方法）
    ///
    /// 将 prompt 包装为 `ExtractionRule { use_llm: true }` 后调用 `extraction_service.extract()`。
    /// 仅供 [`Self::dispatch_extraction`] 的 Prompt 口径调用。
    pub(super) async fn extract_with_prompt(
        &self,
        html: &str,
        prompt: &str,
        base_url: &str,
    ) -> Result<(Value, crate::domain::services::llm::TokenUsage)> {
        let mut rules = HashMap::with_capacity(1);
        rules.insert(
            "extracted_data".to_string(),
            crate::domain::services::extraction_service::ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some(prompt.to_string()),
                output_format: None,
            },
        );
        self.extraction_service
            .extract(html, &rules, Some(base_url))
            .await
    }

    /// 使用 Schema 提取数据（共享辅助方法）
    ///
    /// 直接调用 `extraction_service.extract_with_schema()`。
    /// 仅供 [`Self::dispatch_extraction`] 的 Schema 口径调用。
    pub(super) async fn extract_with_schema(
        &self,
        html: &str,
        schema: &Value,
    ) -> Result<(Value, crate::domain::services::llm::TokenUsage)> {
        // 走 rag-auto 入口：rag 未注入时与 extract_with_schema 完全等价；
        // 注入后按 schema 派生检索 query，用相关分块压缩 LLM 输入（fail-open）
        self.extraction_service
            .extract_with_rag_auto(html, None, schema)
            .await
    }

    /// 记录单次提取的 token 用量（Deferred 结算，自 `task_lifecycle` 迁移）
    ///
    /// 1. Record in-memory for real-time tracking
    /// 2. Defer DB deduction until the completion guard is won
    pub(super) fn record_extraction_usage(
        &self,
        task: &Task,
        usage: crate::domain::services::llm::TokenUsage,
        pending_token_usages: &mut Vec<crate::domain::services::llm::TokenUsage>,
    ) {
        if usage.total_tokens > 0 {
            // 1. Record in-memory for real-time tracking
            self.token_usage
                .entry(task.team_id)
                .or_insert_with(|| AtomicI64::new(0))
                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

            // 2. Defer DB deduction until the completion guard is won
            pending_token_usages.push(usage);
        }
    }

    // handle_crawl_failure 已迁移至 `crawl_task.rs`

    // process_extract_task / handle_rules_extraction / handle_prompt_extraction
    // / handle_schema_extraction / save_extract_result 已迁移至 `extract_task.rs`

    pub(super) async fn deduct_feature_credits(
        &self,
        team_id: crate::domain::id::Id,
        task_id: crate::domain::id::Id,
        screenshot: bool,
        proxy: bool,
    ) {
        let mut extra_credits = 0;

        // 2. Screenshot: 2 Credits
        if screenshot {
            extra_credits += 2;
        }

        // 3. Proxy: 1 Credit
        if proxy {
            extra_credits += 1;
        }

        if extra_credits > 0 {
            // 幂等守卫：丢失完成守卫的陈旧 worker 若重跑本
            // 扣费路径，不得对同一任务二次扣费。已存在同任务 Scrape 扣费流水则跳过。
            match self
                .credits_repository
                .has_deduction_for_task(
                    task_id,
                    crate::domain::models::CreditsTransactionType::Scrape,
                )
                .await
            {
                Ok(true) => {
                    warn!(
                        "Skipping duplicate feature-credit deduction for task {} (already charged)",
                        task_id
                    );
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    // fail-closed：无法确认幂等时不扣费，避免客户可见的双扣费事故；
                    // 显式 error 记录供对账补偿（漏扣可事后追偿，双扣不可逆）。
                    error!(
                        "Idempotency check failed for task {} feature credits: {}; \
                         skipping deduction to avoid double-charge",
                        task_id, e
                    );
                    return;
                }
            }
            if let Err(e) = self
                .credits_repository
                .deduct_credits(
                    team_id,
                    extra_credits,
                    crate::domain::models::CreditsTransactionType::Scrape,
                    format!(
                        "Extra credits for scrape (screenshot/proxy) for task {}",
                        task_id
                    ),
                    Some(task_id),
                )
                .await
            {
                error!("Failed to deduct extra credits for task {}: {}", task_id, e);
                // 欠费追偿记录负数流水，余额进入负值并留 DEBT 标记
                self.record_debt(
                    team_id,
                    task_id,
                    extra_credits,
                    "screenshot/proxy extra credits",
                )
                .await;
            }
        }
    }

    pub(super) async fn deduct_token_credits(
        &self,
        team_id: crate::domain::id::Id,
        task_id: crate::domain::id::Id,
        usage: &crate::domain::services::llm::TokenUsage,
        description: &str,
    ) {
        if usage.total_tokens > 0 {
            // 幂等守卫：已存在同任务 Extract 扣费流水则跳过，
            // 防止陈旧 worker 重跑扣费路径造成双扣费；in-memory 计数一并跳过避免虚高。
            match self
                .credits_repository
                .has_deduction_for_task(
                    task_id,
                    crate::domain::models::CreditsTransactionType::Extract,
                )
                .await
            {
                Ok(true) => {
                    warn!(
                        "Skipping duplicate token-credit deduction for task {} (already charged)",
                        task_id
                    );
                    return;
                }
                Ok(false) => {}
                Err(e) => {
                    error!(
                        "Idempotency check failed for task {} token credits: {}; \
                         skipping deduction to avoid double-charge",
                        task_id, e
                    );
                    return;
                }
            }
            // 1. Record in-memory for real-time tracking
            self.token_usage
                .entry(team_id)
                .or_insert_with(|| AtomicI64::new(0))
                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

            // 2. Convert to credits and deduct from database
            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
            let credits_to_deduct = std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
            if credits_to_deduct > 0 {
                if let Err(e) = self
                    .credits_repository
                    .deduct_credits(
                        team_id,
                        credits_to_deduct,
                        crate::domain::models::CreditsTransactionType::Extract,
                        format!("{} ({} tokens)", description, usage.total_tokens),
                        Some(task_id),
                    )
                    .await
                {
                    error!("Failed to deduct credits for token usage: {}", e);
                    // 欠费追偿同上，记录负数流水
                    self.record_debt(team_id, task_id, credits_to_deduct, "LLM token usage")
                        .await;
                } else {
                    info!(
                        "Deducted {} credits for {} tokens for team {}",
                        credits_to_deduct, usage.total_tokens, team_id
                    );
                }
            }
        }
    }

    /// 欠费追偿后置扣费失败时以负数流水入账。
    ///
    /// `add_credits_safe` 存储过程不检查下限，负数金额使余额进入负值，
    /// 并在 `credits_transactions` 留下 `DEBT:` 标记流水，供补账与对账使用，
    /// 避免后置扣费失败被静默吞掉形成坏账。
    pub(super) async fn record_debt(
        &self,
        team_id: crate::domain::id::Id,
        task_id: crate::domain::id::Id,
        amount: i64,
        reason: &str,
    ) {
        if let Err(debt_err) = self
            .credits_repository
            .add_credits(
                team_id,
                -amount,
                crate::domain::models::CreditsTransactionType::ManualAdjustment,
                format!("DEBT: post-task charge failed ({})", reason),
                Some(task_id),
            )
            .await
        {
            error!(
                "CRITICAL: Failed to record debt of {} credits for team {} (task {}): {}",
                amount, team_id, task_id, debt_err
            );
        } else {
            log::warn!(
                "Recorded debt of {} credits for team {} (task {})",
                amount,
                team_id,
                task_id
            );
        }
    }
}
