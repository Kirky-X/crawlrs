// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 提取与计费 — 从 `ScrapeWorker` 拆分的 partial impl block
//!
//! 包含规则/Prompt/Schema 提取与积分扣减；成功收尾与失败处理见
//! [`super::success_finalization`]。

use super::*;

impl ScrapeWorker {
    /// 使用配置的规则提取数据（支持 rules > prompt > schema 优先级）
    pub(super) async fn extract_data_with_rules(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        config: &CrawlConfigDto,
    ) -> Option<Value> {
        if let Some(rules) = &config.extraction_rules {
            match self
                .extraction_service
                .extract(&response.content, rules, Some(&task.url))
                .await
            {
                Ok((data, usage)) => {
                    self.deduct_token_credits(
                        task.team_id,
                        task.id,
                        &usage,
                        "Tokens used for extraction",
                    )
                    .await;
                    Some(data)
                }
                Err(e) => {
                    error!("Extraction failed for url {}: {}", task.url, e);
                    None
                }
            }
        } else if let Some(prompt) = &config.extraction_prompt {
            if !prompt.is_empty() {
                match self
                    .extract_with_prompt(&response.content, prompt, &task.url)
                    .await
                {
                    Ok((data, usage)) => {
                        self.deduct_token_credits(
                            task.team_id,
                            task.id,
                            &usage,
                            "Tokens used for prompt extraction",
                        )
                        .await;
                        Some(data)
                    }
                    Err(e) => {
                        error!("Prompt extraction failed for url {}: {}", task.url, e);
                        None
                    }
                }
            } else {
                None
            }
        } else if let Some(schema) = &config.extraction_schema {
            match self.extract_with_schema(&response.content, schema).await {
                Ok((data, usage)) => {
                    self.deduct_token_credits(
                        task.team_id,
                        task.id,
                        &usage,
                        "Tokens used for schema extraction",
                    )
                    .await;
                    Some(data)
                }
                Err(e) => {
                    error!("Schema extraction failed for url {}: {}", task.url, e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// 使用 Prompt 提取数据（共享辅助方法）
    ///
    /// 将 prompt 包装为 `ExtractionRule { use_llm: true }` 后调用 `extraction_service.extract()`。
    /// 供 Scrape 路径（handle_scrape_success）和 Crawl 路径（extract_data_with_rules）共同调用。
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
    /// 供 Scrape 路径（handle_scrape_success）和 Crawl 路径（extract_data_with_rules）共同调用。
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
