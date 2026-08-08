// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Fallback 搜索引擎装饰器
//!
//! `FallbackSearchEngine` 实现 Decorator 模式：包装 primary 引擎和 fallback 引擎列表。
//! 搜索时先尝试 primary，失败或结果为空时顺序尝试 fallback。
//! 不修改任何现有引擎代码，纯新增。

use std::sync::Arc;

use async_trait::async_trait;
use log::{info, warn};

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// Fallback 搜索引擎装饰器
///
/// 包装 primary 引擎（通常是 `SearchAggregator`）和一组 fallback 引擎。
/// 搜索策略：
/// 1. 先调用 primary
/// 2. 若 primary 返回成功且结果非空 → 直接返回
/// 3. 若 primary 返回错误或空结果 → 按顺序尝试 fallback 引擎
/// 4. 第一个返回成功且非空结果的 fallback 立即返回
/// 5. 所有 fallback 都失败 → 返回 `SearchError::AllEnginesFailed`
pub struct FallbackSearchEngine {
    primary: Arc<dyn SearchEngine>,
    fallbacks: Vec<Arc<dyn SearchEngine>>,
}

impl FallbackSearchEngine {
    /// 创建 FallbackSearchEngine
    ///
    /// # 参数
    ///
    /// * `primary` - 主搜索引擎（通常是 SearchAggregator）
    /// * `fallbacks` - fallback 引擎列表，按声明顺序尝试
    pub fn new(primary: Arc<dyn SearchEngine>, fallbacks: Vec<Arc<dyn SearchEngine>>) -> Self {
        Self { primary, fallbacks }
    }
}

#[async_trait]
impl SearchEngine for FallbackSearchEngine {
    fn name(&self) -> &'static str {
        "fallback"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Auto
    }

    fn health(&self) -> EngineHealth {
        self.primary.health()
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // 1. 尝试主引擎
        match self.primary.search(request).await {
            Ok(response) if !response.items.is_empty() => {
                return Ok(response);
            }
            Ok(_) => {
                info!(
                    "Primary search returned empty results, trying {} fallback engine(s)",
                    self.fallbacks.len()
                );
            }
            Err(e) => {
                warn!("Primary search failed: {}, trying fallback engines", e);
            }
        }

        // 2. 顺序尝试 fallback 引擎
        for fallback in &self.fallbacks {
            match fallback.search(request).await {
                Ok(response) if !response.items.is_empty() => {
                    info!(
                        "Fallback engine '{}' succeeded with {} results",
                        fallback.name(),
                        response.items.len()
                    );
                    return Ok(response);
                }
                Ok(_) => {
                    info!(
                        "Fallback engine '{}' returned empty results, trying next",
                        fallback.name()
                    );
                    continue;
                }
                Err(e) => {
                    warn!("Fallback engine '{}' failed: {}", fallback.name(), e);
                    continue;
                }
            }
        }

        // 3. 全部失败
        Err(SearchError::AllEnginesFailed)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::response::ResponseItem;
    use crate::search::types::SearchEngineType;

    /// Mock 引擎：可配置返回成功/失败/空结果
    struct MockEngine {
        name: &'static str,
        behavior: MockBehavior,
    }

    enum MockBehavior {
        /// 返回指定数量的结果
        Success(usize),
        /// 返回空结果
        Empty,
        /// 返回错误
        Error(String),
    }

    impl MockEngine {
        fn success(name: &'static str, count: usize) -> Self {
            Self {
                name,
                behavior: MockBehavior::Success(count),
            }
        }

        fn empty(name: &'static str) -> Self {
            Self {
                name,
                behavior: MockBehavior::Empty,
            }
        }

        fn error(name: &'static str, msg: &'static str) -> Self {
            Self {
                name,
                behavior: MockBehavior::Error(msg.to_string()),
            }
        }
    }

    #[async_trait]
    impl SearchEngine for MockEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Auto
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            match &self.behavior {
                MockBehavior::Success(count) => {
                    let items = (0..*count)
                        .map(|i| ResponseItem {
                            title: format!("{} Result {}", self.name, i + 1),
                            url: format!("https://{}.example.com/{}", self.name, i + 1),
                            description: format!("Description from {}", self.name),
                            engine: SearchEngineType::Auto,
                        })
                        .collect();
                    Ok(Response {
                        items,
                        total_results: Some(*count as u64),
                        engine: SearchEngineType::Auto,
                    })
                }
                MockBehavior::Empty => Ok(Response {
                    items: vec![],
                    total_results: Some(0),
                    engine: SearchEngineType::Auto,
                }),
                MockBehavior::Error(msg) => Err(SearchError::EngineFailed(msg.clone())),
            }
        }
    }

    fn make_request() -> SearchRequest {
        SearchRequest::new("test query").with_limit(5)
    }

    // ========== primary 成功不触发 fallback ==========

    #[tokio::test]
    async fn test_primary_success_no_fallback() {
        let primary = Arc::new(MockEngine::success("Primary", 3));
        let fallback = Arc::new(MockEngine::success("Fallback", 2));
        let engine = FallbackSearchEngine::new(primary, vec![fallback]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 3);
        assert!(response.items[0].title.contains("Primary"));
    }

    // ========== primary 返回空结果触发 fallback ==========

    #[tokio::test]
    async fn test_primary_empty_triggers_fallback() {
        let primary = Arc::new(MockEngine::empty("Primary"));
        let fallback = Arc::new(MockEngine::success("Fallback", 2));
        let engine = FallbackSearchEngine::new(primary, vec![fallback]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 2);
        assert!(response.items[0].title.contains("Fallback"));
    }

    // ========== primary 报错触发 fallback ==========

    #[tokio::test]
    async fn test_primary_error_triggers_fallback() {
        let primary = Arc::new(MockEngine::error("Primary", "network error"));
        let fallback = Arc::new(MockEngine::success("Fallback", 1));
        let engine = FallbackSearchEngine::new(primary, vec![fallback]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 1);
        assert!(response.items[0].title.contains("Fallback"));
    }

    // ========== 第一个 fallback 成功立即返回 ==========

    #[tokio::test]
    async fn test_first_fallback_success_stops_chain() {
        let primary = Arc::new(MockEngine::error("Primary", "down"));
        let fallback1 = Arc::new(MockEngine::success("Fallback1", 2));
        let fallback2 = Arc::new(MockEngine::success("Fallback2", 3));
        let engine = FallbackSearchEngine::new(primary, vec![fallback1, fallback2]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 2);
        assert!(response.items[0].title.contains("Fallback1"));
    }

    // ========== fallback 失败继续尝试下一个 ==========

    #[tokio::test]
    async fn test_fallback_failure_continues_to_next() {
        let primary = Arc::new(MockEngine::error("Primary", "down"));
        let fallback1 = Arc::new(MockEngine::error("Fallback1", "also down"));
        let fallback2 = Arc::new(MockEngine::success("Fallback2", 1));
        let engine = FallbackSearchEngine::new(primary, vec![fallback1, fallback2]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 1);
        assert!(response.items[0].title.contains("Fallback2"));
    }

    // ========== 所有 fallback 失败返回 AllEnginesFailed ==========

    #[tokio::test]
    async fn test_all_fallbacks_fail_returns_all_engines_failed() {
        let primary = Arc::new(MockEngine::error("Primary", "down"));
        let fallback1 = Arc::new(MockEngine::error("Fallback1", "also down"));
        let fallback2 = Arc::new(MockEngine::error("Fallback2", "still down"));
        let engine = FallbackSearchEngine::new(primary, vec![fallback1, fallback2]);

        let result = engine.search(&make_request()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SearchError::AllEnginesFailed));
    }

    // ========== 空 fallback 列表时 primary 失败直接返回错误 ==========

    #[tokio::test]
    async fn test_empty_fallback_list_primary_failure() {
        let primary = Arc::new(MockEngine::error("Primary", "down"));
        let engine = FallbackSearchEngine::new(primary, vec![]);

        let result = engine.search(&make_request()).await;
        assert!(matches!(result.unwrap_err(), SearchError::AllEnginesFailed));
    }

    // ========== 空 fallback 列表时 primary 空结果也返回错误 ==========

    #[tokio::test]
    async fn test_empty_fallback_list_primary_empty() {
        let primary = Arc::new(MockEngine::empty("Primary"));
        let engine = FallbackSearchEngine::new(primary, vec![]);

        let result = engine.search(&make_request()).await;
        assert!(matches!(result.unwrap_err(), SearchError::AllEnginesFailed));
    }

    // ========== fallback 返回空结果继续尝试下一个 ==========

    #[tokio::test]
    async fn test_fallback_empty_continues_to_next() {
        let primary = Arc::new(MockEngine::empty("Primary"));
        let fallback1 = Arc::new(MockEngine::empty("Fallback1"));
        let fallback2 = Arc::new(MockEngine::success("Fallback2", 1));
        let engine = FallbackSearchEngine::new(primary, vec![fallback1, fallback2]);

        let response = engine.search(&make_request()).await.unwrap();
        assert_eq!(response.items.len(), 1);
        assert!(response.items[0].title.contains("Fallback2"));
    }

    // ========== 委托方法 ==========

    #[test]
    fn test_name_returns_fallback() {
        let primary = Arc::new(MockEngine::success("Primary", 1));
        let engine = FallbackSearchEngine::new(primary, vec![]);
        assert_eq!(engine.name(), "fallback");
    }

    #[test]
    fn test_health_delegates_to_primary() {
        let primary = Arc::new(MockEngine::success("Primary", 1));
        let engine = FallbackSearchEngine::new(primary, vec![]);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_engine_type_is_auto() {
        let primary = Arc::new(MockEngine::success("Primary", 1));
        let engine = FallbackSearchEngine::new(primary, vec![]);
        assert_eq!(engine.engine_type(), SearchEngineType::Auto);
    }
}
