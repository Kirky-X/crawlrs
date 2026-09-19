// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! vecboost 本地推理 provider（`rag-local` feature）
//!
//! 进程内嵌入 + 重排：[`VecBoostLibrary`] 在构造时加载 Candle CPU 模型，
//! 一个 bi-encoder 模型同时覆盖 [`EmbeddingProvider`] 与 [`RerankProvider`]
//! （vecboost 引擎的默认 rerank 实现 = embed query/docs → 余弦 → sigmoid）。

use std::path::PathBuf;

use vecboost::config::model::{DeviceType, EngineType, ModelConfig};
use vecboost::{LibraryConfig, VecBoostLibrary};

use crate::config::rag::RagSettings;
use crate::domain::services::rag_strategy::EmbeddingProvider;
use crate::domain::services::rerank::{RerankError, RerankProvider, RerankScore};

/// vecboost 本地推理 provider
pub struct VecboostProvider {
    library: VecBoostLibrary,
    dimensions: usize,
}

impl VecboostProvider {
    /// 从配置构造 provider（同步加载模型，耗时数秒；由 DI 在启动期调用）
    pub async fn new(settings: &RagSettings) -> Result<Self, String> {
        // 先初始化 i18n：未初始化时 VecboostError 的 Display 会退化为原始 key，
        // 掩盖模型加载失败的真实原因
        vecboost::i18n::init();

        let model_config = build_model_config(settings);
        let library = VecBoostLibrary::new(LibraryConfig {
            model_config,
            cache_size: settings.local_cache_size,
            rerank_config: None,
        })
        .await
        .map_err(|e| {
            format!(
                "vecboost library init failed (model: {}): {e}",
                settings.local_model_name
            )
        })?;

        log::info!(
            "vecboost local inference ready: model={} dims={} cache={}",
            settings.local_model_name,
            settings.local_dimensions,
            settings.local_cache_size
        );

        Ok(Self {
            library,
            dimensions: settings.local_dimensions,
        })
    }
}

/// 配置 → vecboost ModelConfig 映射（纯函数，便于测试）
fn build_model_config(settings: &RagSettings) -> ModelConfig {
    ModelConfig {
        name: settings.local_model_name.clone(),
        engine_type: EngineType::Candle,
        model_path: PathBuf::from(&settings.local_model_path),
        tokenizer_path: None,
        device: DeviceType::Cpu,
        max_batch_size: 32,
        pooling_mode: None,
        expected_dimension: Some(settings.local_dimensions),
        memory_limit_bytes: None,
        // 库模式无 GPU 管理器，OOM 降级交给宿主进程
        oom_fallback_enabled: false,
        model_sha256: None,
        quantized: false,
    }
}

fn validate_rerank_input(query: &str, documents: &[String]) -> Result<(), RerankError> {
    if query.trim().is_empty() {
        return Err(RerankError::InvalidInput(
            "query must not be empty".to_string(),
        ));
    }
    if documents.is_empty() {
        return Err(RerankError::InvalidInput(
            "documents must not be empty".to_string(),
        ));
    }
    Ok(())
}

#[async_trait::async_trait]
impl EmbeddingProvider for VecboostProvider {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .library
            .embed_batch(texts)
            .await
            .map_err(|e| anyhow::anyhow!("vecboost embed_batch failed: {e}"))?;
        if response.embeddings.len() != texts.len() {
            anyhow::bail!(
                "vecboost embed_batch returned {} embeddings for {} texts",
                response.embeddings.len(),
                texts.len()
            );
        }
        Ok(response
            .embeddings
            .into_iter()
            .map(|result| result.embedding)
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[async_trait::async_trait]
impl RerankProvider for VecboostProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: Option<usize>,
    ) -> Result<Vec<RerankScore>, RerankError> {
        validate_rerank_input(query, documents)?;
        let response = self
            .library
            .rerank(query, documents, top_k)
            .await
            .map_err(|e| RerankError::Provider(format!("vecboost rerank failed: {e}")))?;
        Ok(response
            .results
            .into_iter()
            .map(|result| RerankScore {
                index: result.index,
                score: result.score,
            })
            .collect())
    }

    fn name(&self) -> &str {
        "vecboost"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_model_config_maps_settings() {
        let settings = RagSettings {
            local_model_path: "/tmp/some-model".to_string(),
            local_model_name: "BAAI/bge-small-zh-v1.5".to_string(),
            local_dimensions: 384,
            local_cache_size: 128,
            ..RagSettings::default()
        };
        let config = build_model_config(&settings);
        assert_eq!(config.name, "BAAI/bge-small-zh-v1.5");
        assert_eq!(config.model_path, PathBuf::from("/tmp/some-model"));
        assert_eq!(config.engine_type, EngineType::Candle);
        assert_eq!(config.device, DeviceType::Cpu);
        assert_eq!(config.expected_dimension, Some(384));
        assert!(!config.oom_fallback_enabled);
        assert!(config.tokenizer_path.is_none());
        assert!(!config.quantized);
    }

    /// 真实本地推理冒烟（默认 ignore）：
    /// `cargo test --features rag-local --lib vecboost_provider -- --ignored`
    /// 需要本地模型文件（默认路径 ../vecboost/models/BAAI-bge-small-en-v1.5）。
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires local model weights"]
    async fn test_local_embed_and_rerank_smoke() {
        let model_dir = std::env::var("CRAWLRS_TEST_MODEL_DIR")
            .unwrap_or_else(|_| "../vecboost/models/BAAI-bge-small-en-v1.5".to_string());
        // vecboost 对本地路径做遍历攻击检测，含 ".." 直接拒绝——先归一为绝对路径
        let model_dir = std::fs::canonicalize(&model_dir)
            .expect("model dir must exist; set CRAWLRS_TEST_MODEL_DIR")
            .to_string_lossy()
            .to_string();
        if !std::path::Path::new(&model_dir)
            .join("config.json")
            .exists()
        {
            panic!("model dir {model_dir} not available; set CRAWLRS_TEST_MODEL_DIR");
        }
        let settings = RagSettings {
            enabled: true,
            embedding_provider: "local".to_string(),
            rerank_provider: "local".to_string(),
            local_model_path: model_dir,
            ..RagSettings::default()
        };
        let provider = VecboostProvider::new(&settings).await.expect("provider");

        let vectors = provider
            .embed(&["hello world".to_string(), "goodbye".to_string()])
            .await
            .expect("embed");
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), provider.dimensions());

        let scores = provider
            .rerank(
                "greeting",
                &["hello world".to_string(), "tax filing".to_string()],
                Some(2),
            )
            .await
            .expect("rerank");
        assert_eq!(scores.len(), 2);
        // bi-encoder rerank：greeting 应与 hello world 更相关
        let top = scores
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .unwrap();
        assert_eq!(top.index, 0);
    }

    #[tokio::test]
    async fn test_rerank_validates_empty_input() {
        // 不加载模型：直接构造 provider 字段不可行（library 非空），
        // 输入校验在 library 调用之前，走 RerankProvider 默认路径需实例。
        // 这里通过独立校验函数覆盖逻辑。
        assert!(validate_rerank_input("", &["a".to_string()]).is_err());
        assert!(validate_rerank_input("q", &[]).is_err());
        assert!(validate_rerank_input("q", &["a".to_string()]).is_ok());
    }
}
