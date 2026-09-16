// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 语义重排（rerank）能力
//!
//! 领域层只定义 [`RerankProvider`] 抽象与 [`SearchReranker`] 编排；具体实现：
//! - `local`：vecboost 进程内推理（`rag-local` feature，
//!   `infrastructure::services::vecboost_provider`）
//! - `http`：通用 /rerank REST 适配（无条件编译，
//!   `infrastructure::services::http_rerank_provider`）
//!
//! 搜索管线中的挂点：`SearchService::perform_search` 结果映射之后——
//! RRF/启发式融合给出统计排序，重排补上 query-doc 语义相关性，score 透出 API。

use std::sync::Arc;

use crate::domain::services::search_service::SearchResult;

/// 单条重排评分：`index` 为文档在输入切片中的下标
#[derive(Debug, Clone, PartialEq)]
pub struct RerankScore {
    pub index: usize,
    pub score: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum RerankError {
    /// provider 侧失败（网络、上游 5xx、响应解析等）
    #[error("rerank provider error: {0}")]
    Provider(String),
    /// 输入不合法（空 query / 空文档集等）
    #[error("invalid rerank input: {0}")]
    InvalidInput(String),
}

/// 重排 provider 抽象：query 与候选文档 → 按相关性打分
#[async_trait::async_trait]
pub trait RerankProvider: Send + Sync {
    /// 对 `documents` 按 `query` 相关性评分。
    ///
    /// `top_k` 为 `Some(n)` 时仅要求返回前 n 条（provider 可自由返回更多，
    /// 由调用方截断）；返回的 `index` 必须落在 `0..documents.len()`。
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: Option<usize>,
    ) -> Result<Vec<RerankScore>, RerankError>;

    /// provider 标识（日志用）：`vecboost` / `http` 等
    fn name(&self) -> &str;
}

/// 搜索结果重排器（领域层编排，无条件编译）
///
/// `provider` 为 `None` 时完全直通（RAG 能力未启用 / 未配置），零开销。
/// 重排失败 fail-open：保留原序返回，不让检索可用性绑在重排上（与
/// robots 遵从的 fail-open 语义一致）。
pub struct SearchReranker {
    provider: Option<Arc<dyn RerankProvider>>,
    max_documents: usize,
}

impl SearchReranker {
    /// `max_documents`：送入 provider 的文档数上限——只重排当前顺序的前 N 条，
    /// 防止大结果集把重排延迟拖爆；其余条目原序追加在重排子集之后。
    pub fn new(provider: Option<Arc<dyn RerankProvider>>, max_documents: usize) -> Self {
        Self {
            provider,
            max_documents: max_documents.clamp(1, 100),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.provider.is_some()
    }

    pub fn provider(&self) -> Option<&Arc<dyn RerankProvider>> {
        self.provider.as_ref()
    }

    /// 对搜索结果做语义重排；写入 `SearchResult.score`（0..1 相关性）。
    ///
    /// 排序规则：送排的前 N 条按 score 降序置顶，其余按原序追加（score 为
    /// `None`）——保证尾部结果不会被丢弃。
    pub async fn rerank_results(
        &self,
        query: &str,
        results: Vec<SearchResult>,
    ) -> Vec<SearchResult> {
        let Some(provider) = self.provider.as_ref() else {
            return results;
        };
        if results.len() < 2 || query.trim().is_empty() {
            return results;
        }

        let head_len = results.len().min(self.max_documents);
        let documents: Vec<String> = results[..head_len].iter().map(result_to_document).collect();

        let started = std::time::Instant::now();
        let scored = match provider.rerank(query, &documents, Some(head_len)).await {
            Ok(scores) => scores,
            Err(err) => {
                log::warn!(
                    "search rerank failed (fail-open, keeping original order): \
                     provider={} results={} err={err}",
                    provider.name(),
                    results.len()
                );
                return results;
            }
        };

        // index 越界/重复防御：只取合法下标，缺失者保持原位（排在有分者之后）
        let mut valid: Vec<(usize, f32)> = scored
            .into_iter()
            .filter(|s| s.index < head_len)
            .map(|s| (s.index, s.score))
            .collect();
        valid.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut seen = vec![false; head_len];
        let mut reranked: Vec<SearchResult> = Vec::with_capacity(results.len());
        for (index, score) in &valid {
            seen[*index] = true;
            let mut item = results[*index].clone();
            item.score = Some(*score as f64);
            reranked.push(item);
        }
        for (position, mut item) in results.into_iter().enumerate() {
            let scored_here = position < head_len && seen[position];
            if !scored_here {
                item.score = None;
                reranked.push(item);
            }
        }

        log::info!(
            "search rerank applied: provider={} docs={head_len} scored={} elapsed_ms={}",
            provider.name(),
            valid.len(),
            started.elapsed().as_millis()
        );
        reranked
    }
}

/// 结果 → 重排文档文本：标题为主，description 有值时拼接（重排模型按全文理解）
fn result_to_document(result: &SearchResult) -> String {
    match result.description.as_deref() {
        Some(desc) if !desc.trim().is_empty() => {
            format!("{}. {}", result.title.trim(), desc.trim())
        }
        _ => result.title.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定打分 mock：`fail=true` 模拟 provider 故障（fail-open 路径）
    struct MockProvider {
        fail: bool,
        scores: Vec<RerankScore>,
    }

    #[async_trait::async_trait]
    impl RerankProvider for MockProvider {
        async fn rerank(
            &self,
            _query: &str,
            _documents: &[String],
            _top_k: Option<usize>,
        ) -> Result<Vec<RerankScore>, RerankError> {
            if self.fail {
                return Err(RerankError::Provider("mock down".to_string()));
            }
            // 不做合法性断言：越界/缺失 index 的防御是被测对象（SearchReranker）的职责
            Ok(self.scores.clone())
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    fn result(title: &str, description: Option<&str>) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: format!("https://example.org/{title}"),
            description: description.map(str::to_string),
            engine: "mock".to_string(),
            score: None,
        }
    }

    #[tokio::test]
    async fn test_disabled_reranker_passthrough() {
        let reranker = SearchReranker::new(None, 25);
        assert!(!reranker.is_enabled());

        let results = vec![result("a", None), result("b", None)];
        let out = reranker.rerank_results("q", results.clone()).await;
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.score.is_none()));
    }

    #[tokio::test]
    async fn test_single_result_short_circuits() {
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: false,
                scores: vec![RerankScore {
                    index: 0,
                    score: 0.9,
                }],
            })),
            25,
        );
        let out = reranker
            .rerank_results("q", vec![result("only", None)])
            .await;
        assert_eq!(out.len(), 1);
        assert!(
            out[0].score.is_none(),
            "single result must not be re-scored"
        );
    }

    #[tokio::test]
    async fn test_reorder_by_score_desc() {
        // 原序 a/b/c；provider 认为 b(0.9) > c(0.7) > a(0.1)
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: false,
                scores: vec![
                    RerankScore {
                        index: 0,
                        score: 0.1,
                    },
                    RerankScore {
                        index: 1,
                        score: 0.9,
                    },
                    RerankScore {
                        index: 2,
                        score: 0.7,
                    },
                ],
            })),
            25,
        );
        let results = vec![result("a", None), result("b", None), result("c", None)];
        let out = reranker.rerank_results("rust rerank", results).await;

        let titles: Vec<&str> = out.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["b", "c", "a"]);
        assert!((out[0].score.unwrap() - 0.9).abs() < 1e-6);
        assert!((out[1].score.unwrap() - 0.7).abs() < 1e-6);
        assert!((out[2].score.unwrap() - 0.1).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_fail_open_keeps_original_order() {
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: true,
                scores: vec![],
            })),
            25,
        );
        let results = vec![result("a", None), result("b", None), result("c", None)];
        let out = reranker.rerank_results("q", results).await;
        let titles: Vec<&str> = out.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["a", "b", "c"], "fail-open must keep order");
        assert!(out.iter().all(|r| r.score.is_none()));
    }

    #[tokio::test]
    async fn test_max_documents_caps_head_and_keeps_tail_in_order() {
        // top_n=2：只重排 a/b，c/d 原序追加且无分
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: false,
                scores: vec![
                    RerankScore {
                        index: 1,
                        score: 0.8,
                    },
                    RerankScore {
                        index: 0,
                        score: 0.2,
                    },
                ],
            })),
            2,
        );
        let results = vec![
            result("a", None),
            result("b", None),
            result("c", None),
            result("d", None),
        ];
        let out = reranker.rerank_results("q", results).await;

        let titles: Vec<&str> = out.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["b", "a", "c", "d"]);
        assert!((out[0].score.unwrap() - 0.8).abs() < 1e-6);
        assert!((out[1].score.unwrap() - 0.2).abs() < 1e-6);
        assert!(out[2].score.is_none());
        assert!(out[3].score.is_none());
    }

    #[tokio::test]
    async fn test_out_of_range_and_missing_indices_are_defended() {
        // provider 返回越界 index(99)，且漏掉 index 1
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: false,
                scores: vec![
                    RerankScore {
                        index: 99,
                        score: 1.0,
                    },
                    RerankScore {
                        index: 0,
                        score: 0.5,
                    },
                ],
            })),
            25,
        );
        let results = vec![result("a", None), result("b", None), result("c", None)];
        let out = reranker.rerank_results("q", results).await;

        // 只有 a 有分且置顶；b/c（未评分）原序追加
        let titles: Vec<&str> = out.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["a", "b", "c"]);
        assert!((out[0].score.unwrap() - 0.5).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_empty_query_short_circuits() {
        let reranker = SearchReranker::new(
            Some(Arc::new(MockProvider {
                fail: false,
                scores: vec![],
            })),
            25,
        );
        let results = vec![result("a", None), result("b", None)];
        let out = reranker.rerank_results("   ", results).await;
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn test_document_text_includes_description() {
        let with_desc = result("Title", Some("  Some description.  "));
        assert_eq!(result_to_document(&with_desc), "Title. Some description.");

        let blank_desc = result("Title", Some("   "));
        assert_eq!(result_to_document(&blank_desc), "Title");

        let no_desc = result("Title", None);
        assert_eq!(result_to_document(&no_desc), "Title");
    }
}
