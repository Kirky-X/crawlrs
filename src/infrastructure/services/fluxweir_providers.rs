// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! fluxweir 承载的嵌入/重排 provider（变更 `fluxweir-llm-gateway` design D5）
//!
//! - [`FluxweirEmbeddingProvider`]：SDK `embed`，`f32` 直通 + 维度显性校验
//! - [`FluxweirRerankProvider`]：SDK `rerank` → [`RerankScore`]（index 越界
//!   显性报错）
//!
//! 两者均为 RAG 降级链的 **secondary**（vecboost/http 为主 primary），由
//! `init_rag_components` 经复合 provider 装配；单独使用亦可。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::config::Settings;
use crate::domain::services::rag_strategy::EmbeddingProvider;
use crate::domain::services::rerank::{RerankError, RerankProvider, RerankScore};

/// fluxweir 抽象嵌入模型名（网关侧按模型路由厂商；未登记目录模型走保守默认）
pub const FLUXWEIR_EMBED_MODEL: &str = "embedding";
/// fluxweir 抽象重排模型名
pub const FLUXWEIR_RERANK_MODEL: &str = "rerank";

/// 从 crawlrs 设置构造 fluxweir SDK 客户端（嵌入/重排/carrier 共用形状）。
///
/// base_url 决议：`embedded` → `http://{listen_addr}`（起服前构造亦安全——
/// 请求发生在网关 readyz 之后）；`external` → `external_base_url`。
/// `enabled=false` 或密钥不可解析 → `None`（调用方按无兜底装配）。
pub fn build_client(settings: &Settings) -> Option<Arc<fluxweir::sdk::FluxweirClient>> {
    let fw = &settings.fluxweir;
    if !fw.enabled {
        return None;
    }
    let base_url = match fw.mode.as_str() {
        "embedded" => format!("http://{}", fw.listen_addr.trim()),
        "external" => fw
            .external_base_url
            .trim()
            .trim_end_matches('/')
            .to_string(),
        other => {
            log::warn!("fluxweir mode \"{other}\" invalid; fluxweir providers disabled");
            return None;
        }
    };
    let api_key = fw.api_key()?;
    let builder = fluxweir::sdk::FluxweirClientBuilder::new()
        .base_url(base_url)
        .api_key(api_key)
        .timeout(Duration::from_secs(fw.request_timeout_secs));
    builder.build().ok().map(Arc::new)
}

/// fluxweir 嵌入 provider（secondary；维度以 `[rag].local_dimensions` 为准）
pub struct FluxweirEmbeddingProvider {
    client: Arc<fluxweir::sdk::FluxweirClient>,
    model: String,
    dimensions: usize,
}

impl FluxweirEmbeddingProvider {
    pub fn new(
        client: Arc<fluxweir::sdk::FluxweirClient>,
        model: impl Into<String>,
        dimensions: usize,
    ) -> Self {
        Self {
            client,
            model: model.into(),
            dimensions,
        }
    }
}

/// SDK embed 响应 → 向量矩阵的纯映射（单测承载：成功 / 维度不符 /
/// base64 编码 / index 越界 / 缺槽 五分支）。全部显性报错，不静默截断补零。
fn map_embed_response(
    response: &fluxweir::sdk::EmbedResponse,
    expected_dimensions: usize,
    total: usize,
) -> Result<Vec<Vec<f32>>> {
    use fluxweir::sdk::EmbeddingValue;

    let mut slots: Vec<Option<Vec<f32>>> = vec![None; total];
    for data in &response.data {
        if data.index >= total {
            return Err(anyhow!(
                "fluxweir embed index out of range: {} (total {total})",
                data.index
            ));
        }
        let vector = match &data.embedding {
            EmbeddingValue::Floats(v) => v.clone(),
            EmbeddingValue::Base64(_) => {
                return Err(anyhow!(
                    "fluxweir embed returned base64 encoding; float expected"
                ))
            }
        };
        if vector.len() != expected_dimensions {
            return Err(anyhow!(
                "fluxweir embed dimension mismatch: got {}, configured {expected_dimensions}",
                vector.len()
            ));
        }
        slots[data.index] = Some(vector);
    }
    if let Some((i, None)) = slots.iter().enumerate().find(|(_, s)| s.is_none()) {
        return Err(anyhow!("fluxweir embed response missing slot {i}"));
    }
    Ok(slots
        .into_iter()
        .map(|s| s.expect("checked above"))
        .collect())
}

#[async_trait::async_trait]
impl EmbeddingProvider for FluxweirEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let request = fluxweir::sdk::EmbedRequest::new(&self.model, texts.to_vec());
        let response = self
            .client
            .embed(&request)
            .await
            .map_err(|e| anyhow!("fluxweir embed request failed: {e}"))?;
        map_embed_response(&response, self.dimensions, texts.len())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// fluxweir 重排 provider（secondary）
pub struct FluxweirRerankProvider {
    client: Arc<fluxweir::sdk::FluxweirClient>,
    model: String,
}

impl FluxweirRerankProvider {
    pub fn new(client: Arc<fluxweir::sdk::FluxweirClient>, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }
}

/// SDK rerank 响应 → [`RerankScore`] 列表的纯映射（index 越界显性报错）
fn map_rerank_results(
    results: &[fluxweir::sdk::RerankResult],
    total: usize,
) -> Result<Vec<RerankScore>, RerankError> {
    results
        .iter()
        .map(|r| {
            if r.index >= total {
                return Err(RerankError::Provider(format!(
                    "fluxweir rerank index out of range: {} (total {total})",
                    r.index
                )));
            }
            Ok(RerankScore {
                index: r.index,
                score: r.relevance_score as f32,
            })
        })
        .collect()
}

#[async_trait::async_trait]
impl RerankProvider for FluxweirRerankProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: Option<usize>,
    ) -> Result<Vec<RerankScore>, RerankError> {
        if query.trim().is_empty() {
            return Err(RerankError::InvalidInput("query is empty".to_string()));
        }
        if documents.is_empty() {
            return Err(RerankError::InvalidInput("documents are empty".to_string()));
        }
        let request = fluxweir::sdk::RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: documents.to_vec(),
            top_n: top_k,
            return_documents: Some(false),
            instruction: None,
            config: fluxweir::sdk::RequestConfig::default(),
        };
        let response =
            self.client.rerank(&request).await.map_err(|e| {
                RerankError::Provider(format!("fluxweir rerank request failed: {e}"))
            })?;
        map_rerank_results(&response.results, documents.len())
    }

    fn name(&self) -> &str {
        "fluxweir"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluxweir::sdk::{EmbedData, EmbedResponse, EmbedUsage, RerankMeta, RerankResponse};

    fn embed_response(vectors: Vec<Vec<f32>>) -> EmbedResponse {
        EmbedResponse {
            model: "embedding".to_string(),
            data: vectors
                .into_iter()
                .enumerate()
                .map(|(index, embedding)| EmbedData {
                    index,
                    embedding: fluxweir::sdk::EmbeddingValue::Floats(embedding),
                })
                .collect(),
            usage: EmbedUsage {
                prompt_tokens: 1,
                total_tokens: 1,
            },
        }
    }

    #[test]
    fn embed_mapping_success() {
        let resp = embed_response(vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        let out = map_embed_response(&resp, 2, 2).expect("ok");
        assert_eq!(out, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
    }

    #[test]
    fn embed_mapping_dimension_mismatch_errors() {
        let resp = embed_response(vec![vec![0.1, 0.2, 0.3]]);
        let err = map_embed_response(&resp, 2, 1).expect_err("must error");
        assert!(err.to_string().contains("dimension mismatch"), "{err}");
    }

    #[test]
    fn embed_mapping_index_out_of_range_errors() {
        let mut resp = embed_response(vec![vec![0.1, 0.2]]);
        resp.data[0].index = 5;
        let err = map_embed_response(&resp, 2, 1).expect_err("must error");
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    #[test]
    fn embed_mapping_missing_slot_errors() {
        let resp = embed_response(vec![vec![0.1, 0.2]]);
        let err = map_embed_response(&resp, 2, 2).expect_err("must error");
        assert!(err.to_string().contains("missing slot 1"), "{err}");
    }

    #[test]
    fn embed_mapping_base64_encoding_errors() {
        let resp = EmbedResponse {
            model: "embedding".to_string(),
            data: vec![EmbedData {
                index: 0,
                embedding: fluxweir::sdk::EmbeddingValue::Base64("AAAA".to_string()),
            }],
            usage: EmbedUsage {
                prompt_tokens: 1,
                total_tokens: 1,
            },
        };
        let err = map_embed_response(&resp, 2, 1).expect_err("must error");
        assert!(err.to_string().contains("base64"), "{err}");
    }

    fn rerank_response(pairs: &[(usize, f64)]) -> RerankResponse {
        RerankResponse {
            id: "r1".to_string(),
            model: "rerank".to_string(),
            results: pairs
                .iter()
                .map(|(index, score)| fluxweir::sdk::RerankResult {
                    index: *index,
                    relevance_score: *score,
                    document: None,
                })
                .collect(),
            meta: RerankMeta {
                vendor: "test".to_string(),
                billed_documents: pairs.len(),
            },
        }
    }

    #[test]
    fn rerank_mapping_success() {
        let resp = rerank_response(&[(1, 0.9), (0, 0.1)]);
        let out = map_rerank_results(&resp.results, 2).expect("ok");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].index, 1);
        assert!((out[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn rerank_mapping_index_out_of_range_errors() {
        let resp = rerank_response(&[(7, 0.5)]);
        let err = map_rerank_results(&resp.results, 2).expect_err("must error");
        assert!(err.to_string().contains("out of range"), "{err}");
    }

    /// 空输入在 provider 侧显性拒绝（InvalidInput），不发请求
    ///（离线构造 client：reqwest build 无 I/O；校验短路先于任何网络调用）
    #[tokio::test]
    async fn rerank_rejects_empty_input_without_request() {
        let client = fluxweir::sdk::FluxweirClientBuilder::new()
            .base_url("http://127.0.0.1:1")
            .api_key("test-key")
            .build()
            .expect("offline client build");
        let provider = FluxweirRerankProvider::new(Arc::new(client), "rerank");
        let err = provider
            .rerank("", &["doc".to_string()], None)
            .await
            .expect_err("empty query must be rejected");
        assert!(err.to_string().contains("query is empty"), "{err}");
        let err = provider
            .rerank("q", &[], None)
            .await
            .expect_err("empty documents must be rejected");
        assert!(err.to_string().contains("documents are empty"), "{err}");
    }

    /// 空文本集嵌入直通空矩阵（不发请求）
    #[tokio::test]
    async fn embed_empty_input_returns_empty_without_request() {
        let client = fluxweir::sdk::FluxweirClientBuilder::new()
            .base_url("http://127.0.0.1:1")
            .api_key("test-key")
            .build()
            .expect("offline client build");
        let provider = FluxweirEmbeddingProvider::new(Arc::new(client), "embedding", 2);
        let out = provider.embed(&[]).await.expect("ok");
        assert!(out.is_empty());
        assert_eq!(provider.dimensions(), 2);
    }
}
