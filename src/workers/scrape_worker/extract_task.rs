// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Extract 任务处理方法
//!
//! 从 `ScrapeWorker` 提取的 extract 任务处理相关方法（partial impl block）。
//! 包含任务分发、规则/Prompt/Schema 提取和结果保存。

use super::ScrapeWorker;
use super::{build_extract_request_fn, parse_extract_payload};
use crate::domain::models::{ScrapeResult, Task, TaskStatus};
use crate::engines::engine_client::ScrapeResponse;
use anyhow::Result;
use chrono::Utc;
use log::{debug, info, warn};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::workers::scrape_executor::process_text_encoding;

impl ScrapeWorker {
    pub(super) async fn process_extract_task(&self, mut task: Task) -> Result<()> {
        info!("Processing extract task {}", task.id);

        // 1. 解析 Payload
        let (payload, url) = parse_extract_payload(&task)?;
        debug!("has_rules: {}", payload.rules.is_some());
        if let Some(ref rules) = payload.rules {
            debug!("rules_count: {}", rules.len());
        }

        // 2. 构建并执行 Scrape 请求
        let scrape_req =
            build_extract_request_fn(&url, self.settings.timeouts.engines.default_timeout_seconds);
        let scrape_resp = self.engine_client.scrape(&scrape_req).await?;

        // 3. 文本编码处理
        let processed_content = match process_text_encoding(&task, &scrape_resp).await {
            Ok(content) => content.into_owned(),
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                scrape_resp.content.clone()
            }
        };

        let processed_scrape_resp = ScrapeResponse {
            content: processed_content,
            ..scrape_resp
        };

        // 4. 根据不同的提取方式处理
        if let Some(rules) = payload.rules {
            return self
                .handle_rules_extraction(&mut task, &processed_scrape_resp, &rules, &url)
                .await;
        }

        if let Some(prompt) = payload.prompt {
            return self
                .handle_prompt_extraction(&mut task, &processed_scrape_resp, prompt, &url)
                .await;
        }

        if let Some(schema) = payload.schema {
            return self
                .handle_schema_extraction(&mut task, &processed_scrape_resp, &schema, &url)
                .await;
        }

        // Fallback: 无提取规则时保存原始结果
        self.save_extract_result(&mut task, &processed_scrape_resp, None, &url)
            .await
    }

    /// 处理基于规则的提取
    pub(super) async fn handle_rules_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        rules: &HashMap<String, crate::domain::services::extraction_service::ExtractionRule>,
        url: &str,
    ) -> Result<()> {
        debug!("rules: {:?}", rules);

        let (extracted_data, usage) = self
            .extraction_service
            .extract(&response.content, rules, Some(url))
            .await?;

        self.deduct_token_credits(
            task.team_id,
            task.id,
            &usage,
            "Tokens used for extraction rules",
        )
        .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 处理基于 Prompt 的提取
    pub(super) async fn handle_prompt_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        prompt: String,
        url: &str,
    ) -> Result<()> {
        let mut rules = HashMap::with_capacity(1);
        rules.insert(
            "extracted_data".to_string(),
            crate::domain::services::extraction_service::ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some(prompt),
                output_format: None,
            },
        );

        let (extracted_data, usage) = self
            .extraction_service
            .extract(&response.content, &rules, Some(url))
            .await?;

        self.deduct_token_credits(task.team_id, task.id, &usage, "Tokens used for extraction")
            .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 处理基于 Schema 的提取
    pub(super) async fn handle_schema_extraction(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        schema: &serde_json::Value,
        url: &str,
    ) -> Result<()> {
        let (extracted_data, usage) = self
            .extraction_service
            .extract_with_schema(&response.content, schema)
            .await?;

        self.deduct_token_credits(
            task.team_id,
            task.id,
            &usage,
            "Tokens used for schema extraction",
        )
        .await;

        self.save_extract_result(task, response, Some(extracted_data), url)
            .await
    }

    /// 保存提取结果
    pub(super) async fn save_extract_result(
        &self,
        task: &mut Task,
        response: &ScrapeResponse,
        extracted_data: Option<Value>,
        url: &str,
    ) -> Result<()> {
        let meta_data = extracted_data
            .map(|data| json!({ "extracted_data": data }))
            .unwrap_or(json!({}));

        let scrape_result = ScrapeResult {
            id: Uuid::new_v4(),
            task_id: task.id,
            url: url.to_string(),
            status_code: response.status_code as i32,
            content: response.content.clone(),
            content_type: response.content_type.clone(),
            headers: serde_json::to_value(&response.headers).unwrap_or(json!({})),
            meta_data,
            screenshot: None,
            response_time_ms: response.response_time_ms as i64,
            created_at: Utc::now(),
        };

        self.result_repository.save(scrape_result).await?;

        task.status = TaskStatus::Completed;
        self.repository.update(task).await?;

        self.trigger_webhook(task, None).await;

        Ok(())
    }
}
