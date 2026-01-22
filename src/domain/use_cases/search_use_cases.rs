// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search-related use cases

use crate::application::dto::search_request::SearchRequestDto;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::services::credits_service::CreditsService;
use crate::search::engine_trait::SearchRequest;
use std::sync::Arc;
use uuid::Uuid;

/// 搜索请求（应用层）
pub struct UseCaseSearchRequest {
    pub team_id: Uuid,
    pub request: SearchRequestDto,
    pub engine: Option<String>,
}

/// 搜索响应
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchEngineResult>,
    pub total_results: u64,
    pub search_engine: String,
    pub response_time_ms: u64,
}

/// 搜索引擎结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchEngineResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub engine: String,
}

/// 搜索用例
pub struct SearchUseCase<R: CreditsRepository> {
    credits_service: Arc<CreditsService<R>>,
}

impl<R: CreditsRepository> SearchUseCase<R> {
    pub fn new(credits_service: Arc<CreditsService<R>>) -> Self {
        Self { credits_service }
    }

    pub async fn execute<E: crate::search::SearchEngine>(
        &self,
        request: UseCaseSearchRequest,
        engine: &E,
    ) -> Result<SearchResponse, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // 构建搜索请求
        let search_request = SearchRequest::new(&request.request.query)
            .with_limit(request.request.limit.unwrap_or(10));

        // 执行搜索
        let engine_response = engine
            .search(&search_request)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // 转换结果
        let results: Vec<SearchEngineResult> = engine_response
            .items
            .into_iter()
            .map(|r| SearchEngineResult {
                title: r.title,
                url: r.url,
                snippet: r.description,
                engine: engine.name().to_string(),
            })
            .collect();

        let response_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(SearchResponse {
            query: request.request.query.clone(),
            results,
            total_results: engine_response.total_results.unwrap_or(0) as u64,
            search_engine: engine.name().to_string(),
            response_time_ms,
        })
    }
}

/// 多引擎搜索请求
pub struct MultiEngineSearchRequest {
    pub team_id: Uuid,
    pub query: String,
    pub engines: Vec<String>,
    pub limit: Option<u32>,
}

/// 多引擎搜索响应
pub struct MultiEngineSearchResponse {
    pub query: String,
    pub results: Vec<SearchEngineResult>,
    pub total_results: u64,
    pub response_time_ms: u64,
}

/// 多引擎搜索用例
pub struct MultiEngineSearchUseCase<R: CreditsRepository> {
    credits_service: Arc<CreditsService<R>>,
}

impl<R: CreditsRepository> MultiEngineSearchUseCase<R> {
    pub fn new(credits_service: Arc<CreditsService<R>>) -> Self {
        Self { credits_service }
    }

    pub async fn execute<E: crate::search::SearchEngine>(
        &self,
        request: MultiEngineSearchRequest,
        engine: &E,
    ) -> Result<MultiEngineSearchResponse, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // 构建搜索请求
        let search_request =
            SearchRequest::new(&request.query).with_limit(request.limit.unwrap_or(10));

        // 执行搜索
        let engine_response = engine
            .search(&search_request)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // 转换结果
        let results: Vec<SearchEngineResult> = engine_response
            .items
            .into_iter()
            .map(|r| SearchEngineResult {
                title: r.title,
                url: r.url,
                snippet: r.description,
                engine: engine.name().to_string(),
            })
            .collect();

        let total_results = results.len() as u64;
        let response_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(MultiEngineSearchResponse {
            query: request.query,
            results,
            total_results,
            response_time_ms,
        })
    }
}
