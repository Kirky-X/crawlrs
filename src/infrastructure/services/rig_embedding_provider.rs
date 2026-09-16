// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! rig 远端嵌入 provider（`rag-remote` feature）
//!
//! 通过 rig 的 OpenAI 兼容客户端访问远端嵌入服务（OpenAI / 硅基流动 /
//! Ollama `/v1` 等）：`Client::builder().api_key(..).base_url(..)` +
//! `embedding_model_with_ndims(..).embed_texts(..)`。
//!
//! 与 rig 的取舍：rig 0.42 的 rerank 原生仅支持 Voyage，重排方向由
//! [`crate::infrastructure::services::http_rerank_provider`]（通用 HTTP 适配，
//! 无 feature 要求）承担，本模块只做嵌入。

use rig_core::client::embeddings::EmbeddingsClient;
use rig_core::embeddings::EmbeddingModel;

use crate::config::rag::RagSettings;
use crate::domain::services::rag_strategy::EmbeddingProvider;

/// rig 远端嵌入 provider
pub struct RigEmbeddingProvider {
    model: rig_core::providers::openai::EmbeddingModel,
    dimensions: usize,
}

impl RigEmbeddingProvider {
    /// 从配置构造 provider
    ///
    /// * `remote_api_base` 为空时使用 rig 默认官方端点；API key 显式传入
    ///   （不读 `OPENAI_API_KEY` 环境变量，crawlrs 配置链是唯一事实来源）
    pub fn new(settings: &RagSettings) -> Result<Self, String> {
        let api_key = settings.remote_api_key().unwrap_or_default();
        if api_key.is_empty() {
            return Err(
                "rag.remote_api_key is required for the remote embedding provider".to_string(),
            );
        }

        let mut builder =
            rig_core::providers::openai::Client::builder().api_key(api_key.to_string());
        let base = settings.remote_api_base.trim();
        if !base.is_empty() {
            if base.parse::<url::Url>().is_err() {
                return Err(format!("rag.remote_api_base is not a valid URL: {base}"));
            }
            builder = builder.base_url(base);
        }
        let client = builder
            .build()
            .map_err(|e| format!("rig openai client build failed: {e}"))?;

        let dimensions = settings.remote_dimensions.max(1);
        let model = client.embedding_model_with_ndims(&settings.remote_embed_model, dimensions);

        log::info!(
            "rig remote embedding ready: model={} base={} dims={}",
            settings.remote_embed_model,
            if base.is_empty() {
                "(rig default)"
            } else {
                base
            },
            dimensions
        );

        Ok(Self { model, dimensions })
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for RigEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let embeddings = self
            .model
            .embed_texts(texts.to_vec())
            .await
            .map_err(|e| anyhow::anyhow!("rig embed_texts failed: {e}"))?;
        if embeddings.len() != texts.len() {
            anyhow::bail!(
                "rig embed_texts returned {} embeddings for {} texts",
                embeddings.len(),
                texts.len()
            );
        }
        Ok(embeddings
            .into_iter()
            .map(|embedding| embedding.vec.into_iter().map(|v| v as f32).collect())
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn settings(base: &str, key: &str) -> RagSettings {
        RagSettings {
            enabled: true,
            embedding_provider: "remote".to_string(),
            remote_embed_model: "text-embedding-3-small".to_string(),
            remote_api_base: base.to_string(),
            remote_api_key: Some(key.to_string()),
            ..RagSettings::default()
        }
    }

    #[test]
    fn test_constructor_requires_api_key() {
        let err = match RigEmbeddingProvider::new(&settings("https://api.openai.com/v1", "")) {
            Err(err) => err,
            Ok(_) => panic!("empty key must fail"),
        };
        assert!(err.contains("remote_api_key"), "got: {err}");
    }

    #[test]
    fn test_constructor_rejects_bad_base_url() {
        let err = match RigEmbeddingProvider::new(&settings("not-a-url", "sk-test")) {
            Err(err) => err,
            Ok(_) => panic!("bad base url must fail"),
        };
        assert!(err.contains("remote_api_base"), "got: {err}");
    }

    #[tokio::test]
    async fn test_embed_texts_via_openai_compatible_endpoint() {
        let server = MockServer::start().await;
        // wiremock 的 path() 匹配完整路径（含 base_url 段 /v1）；
        // rig 因显式 ndims 会额外下发 dimensions 字段，用 partial 匹配契约字段
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_partial_json(serde_json::json!({
                "model": "text-embedding-3-small",
                "input": ["hello world", "goodbye"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [
                    {"object": "embedding", "index": 0, "embedding": [0.25, 0.5, 0.25]},
                    {"object": "embedding", "index": 1, "embedding": [1.0, 0.0, 0.0]}
                ],
                "model": "text-embedding-3-small",
                "usage": {"prompt_tokens": 3, "total_tokens": 3}
            })))
            .mount(&server)
            .await;

        let provider =
            RigEmbeddingProvider::new(&settings(&format!("{}/v1", server.uri()), "sk-test"))
                .expect("provider");

        let vectors = provider
            .embed(&["hello world".to_string(), "goodbye".to_string()])
            .await
            .expect("embed");
        assert_eq!(vectors.len(), 2);
        // rig 返回 f64 向量，provider 必须转换为 f32
        assert_eq!(vectors[0], vec![0.25_f32, 0.5, 0.25]);
        assert_eq!(vectors[1], vec![1.0_f32, 0.0, 0.0]);
        assert_eq!(provider.dimensions(), 1536);
    }

    #[tokio::test]
    async fn test_embed_empty_input_short_circuits() {
        let provider = RigEmbeddingProvider::new(&settings("https://api.openai.com/v1", "sk-test"))
            .expect("provider");
        let vectors = provider.embed(&[]).await.expect("embed");
        assert!(vectors.is_empty());
    }
}
