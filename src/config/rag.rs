// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! RAG 能力配置（嵌入 + 重排）
//!
//! 两条 provider 链路，独立选择、可单独启用：
//! - 嵌入：`local`（vecboost 进程内推理，需 `rag-local` feature）或
//!   `remote`（rig OpenAI 兼容端点，需 `rag-remote` feature）
//! - 重排：`local`（vecboost，需 `rag-local`）或 `http`（通用 /rerank REST
//!   适配，兼容 Cohere/Jina/自建端点，仅 reqwest，无需 feature）
//!
//! 消费方：搜索结果精准重排（[`crate::domain::services::rerank`]）与
//! schema 抽取的 RAG 上下文（`extract_with_rag_auto`）。

use serde::{Deserialize, Serialize};

/// RAG 配置设置
///
/// # 字段说明
///
/// * `remote_api_key` / `rerank_api_key` - 敏感信息，仅 crate 可见，
///   外部模块应使用 [`RagSettings::remote_api_key`] / [`RagSettings::rerank_api_key`]
///
/// # 安全提示
///
/// 两个 api_key 字段包含外部服务凭据，Debug 输出一律 `[REDACTED]`。
#[derive(Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__RAG__")]
pub struct RagSettings {
    /// 总开关：`false` 时全部 provider 为 None，零运行时开销
    #[config(default = false)]
    pub enabled: bool,

    /// 嵌入 provider：`""`（禁用）| `local` | `remote`
    #[config(default = String::new())]
    pub embedding_provider: String,

    /// 重排 provider：`""`（禁用）| `local` | `http`
    #[config(default = String::new())]
    pub rerank_provider: String,

    // --- local（vecboost）---
    /// 本地模型目录（须含 config.json / tokenizer.json / model.safetensors）。
    /// 指向不存在的路径时按 HF repo id 在线拉取（需网络）。
    /// 模型获取：vecboost 仓库 `cargo run -p vecboost-examples --bin download_model -- --small`
    #[config(default = "models/BAAI-bge-small-en-v1.5".to_string())]
    pub local_model_path: String,

    /// 本地模型标识（HF repo id 语义，仅作元数据与日志）
    #[config(default = "BAAI/bge-small-en-v1.5".to_string())]
    pub local_model_name: String,

    /// 嵌入维度（bge-small / all-MiniLM = 384）
    #[config(default = 384)]
    pub local_dimensions: usize,

    /// 本地推理嵌入缓存条数（0 = 关闭）
    #[config(default = 256)]
    pub local_cache_size: usize,

    // --- remote（rig，OpenAI 兼容）---
    /// 远端嵌入模型名
    #[config(default = "text-embedding-3-small".to_string())]
    pub remote_embed_model: String,

    /// 远端嵌入维度（text-embedding-3-small = 1536；显式声明可让
    /// rig `embedding_model_with_ndims` 跳过探测并作元数据透出）
    #[config(default = 1536)]
    pub remote_dimensions: usize,

    /// OpenAI 兼容基础 URL（如 `https://api.openai.com/v1`、Ollama `http://localhost:11434/v1`）；
    /// 空则用 rig 默认官方端点
    #[config(default = String::new())]
    pub remote_api_base: String,

    /// 远端嵌入 API 密钥（敏感信息）
    pub(crate) remote_api_key: Option<String>,

    // --- http（通用 rerank REST）---
    /// 重排端点完整 URL（如 `https://api.jina.ai/v1/rerank`、TEI `http://host/rerank`）
    #[config(default = String::new())]
    pub rerank_endpoint: String,

    /// 重排模型名（TEI 格式不需要）
    #[config(default = String::new())]
    pub rerank_model: String,

    /// 重排线格式：`cohere`（兼容 Cohere v2 / Jina / 多数自建）| `tei`（text-embeddings-inference）
    #[config(default = "cohere".to_string())]
    pub rerank_format: String,

    /// 重排端点 Bearer 密钥（敏感信息；TEI 常无鉴权，留空）
    pub(crate) rerank_api_key: Option<String>,

    /// 重排请求超时（秒）
    #[config(default = 10)]
    pub rerank_timeout_seconds: u64,

    // --- 行为 ---
    /// 搜索重排送入 provider 的最大文档数（按当前顺序取前 N，防止长尾延迟）
    #[config(default = 25)]
    pub search_rerank_top_n: usize,

    /// RAG 抽取检索的 top-K 分块数
    #[config(default = 5)]
    pub rag_top_k: usize,
}

impl std::fmt::Debug for RagSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RagSettings")
            .field("enabled", &self.enabled)
            .field("embedding_provider", &self.embedding_provider)
            .field("rerank_provider", &self.rerank_provider)
            .field("local_model_path", &self.local_model_path)
            .field("local_model_name", &self.local_model_name)
            .field("local_dimensions", &self.local_dimensions)
            .field("local_cache_size", &self.local_cache_size)
            .field("remote_embed_model", &self.remote_embed_model)
            .field("remote_dimensions", &self.remote_dimensions)
            .field("remote_api_base", &self.remote_api_base)
            .field("remote_api_key", &"[REDACTED]")
            .field("rerank_endpoint", &self.rerank_endpoint)
            .field("rerank_model", &self.rerank_model)
            .field("rerank_format", &self.rerank_format)
            .field("rerank_api_key", &"[REDACTED]")
            .field("rerank_timeout_seconds", &self.rerank_timeout_seconds)
            .field("search_rerank_top_n", &self.search_rerank_top_n)
            .field("rag_top_k", &self.rag_top_k)
            .finish()
    }
}

impl RagSettings {
    /// 获取远端嵌入 API 密钥
    pub fn remote_api_key(&self) -> Option<&str> {
        self.remote_api_key.as_deref()
    }

    /// 获取重排端点 API 密钥
    pub fn rerank_api_key(&self) -> Option<&str> {
        self.rerank_api_key.as_deref()
    }

    /// 启动期 provider 配置校验（fail-fast）
    ///
    /// 除字段合法性外，还校验"配置的 provider 是否已编译进二进制"：
    /// `local` 需要 `rag-local` feature、`remote` 需要 `rag-remote` feature，
    /// 缺失时给出带重建指引的错误，而非运行到一半才失败。
    pub fn validate_providers(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        match self.embedding_provider.as_str() {
            "" => {}
            "local" => {
                #[cfg(not(feature = "rag-local"))]
                return Err(
                    "rag.embedding_provider = \"local\" 需要 rag-local feature（vecboost）；\
                     请以 --features rag-local 重新构建，或改用 \"remote\""
                        .to_string(),
                );
            }
            "remote" => {
                #[cfg(not(feature = "rag-remote"))]
                return Err(
                    "rag.embedding_provider = \"remote\" 需要 rag-remote feature（rig-core）；\
                     请以 --features rag-remote 重新构建"
                        .to_string(),
                );
                #[cfg(feature = "rag-remote")]
                if self.remote_embed_model.trim().is_empty() {
                    return Err("rag.remote_embed_model 不能为空".to_string());
                }
            }
            other => {
                return Err(format!(
                    "rag.embedding_provider 取值非法：\"{other}\"（支持 \"\"/local/remote）"
                ));
            }
        }

        match self.rerank_provider.as_str() {
            "" => {}
            "local" => {
                #[cfg(not(feature = "rag-local"))]
                return Err(
                    "rag.rerank_provider = \"local\" 需要 rag-local feature（vecboost）；\
                     请以 --features rag-local 重新构建，或改用 \"http\""
                        .to_string(),
                );
            }
            "http" => {
                if self.rerank_endpoint.trim().is_empty() {
                    return Err(
                        "rag.rerank_provider = \"http\" 时 rag.rerank_endpoint 不能为空"
                            .to_string(),
                    );
                }
                match self.rerank_format.as_str() {
                    "cohere" => {
                        if self.rerank_model.trim().is_empty() {
                            return Err(
                                "rag.rerank_format = \"cohere\" 时 rag.rerank_model 不能为空"
                                    .to_string(),
                            );
                        }
                    }
                    "tei" => {}
                    other => {
                        return Err(format!(
                            "rag.rerank_format 取值非法：\"{other}\"（支持 cohere/tei）"
                        ));
                    }
                }
            }
            other => {
                return Err(format!(
                    "rag.rerank_provider 取值非法：\"{other}\"（支持 \"\"/local/http）"
                ));
            }
        }

        if self.search_rerank_top_n == 0 || self.search_rerank_top_n > 100 {
            return Err("rag.search_rerank_top_n 须在 1..=100".to_string());
        }
        if self.rag_top_k == 0 || self.rag_top_k > 50 {
            return Err("rag.rag_top_k 须在 1..=50".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rag_default_all_disabled() {
        let settings = RagSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.embedding_provider, "");
        assert_eq!(settings.rerank_provider, "");
        assert_eq!(settings.local_dimensions, 384);
        assert_eq!(settings.rerank_format, "cohere");
        assert_eq!(settings.search_rerank_top_n, 25);
        assert_eq!(settings.rag_top_k, 5);
        // 全关配置必须通过校验
        settings
            .validate_providers()
            .expect("default must be valid");
    }

    #[test]
    fn test_rag_debug_redacts_keys() {
        let settings = RagSettings {
            enabled: true,
            remote_api_key: Some("sk-secret".to_string()),
            rerank_api_key: Some("jina-secret".to_string()),
            ..RagSettings::default()
        };
        let debug = format!("{settings:?}");
        assert!(debug.contains("[REDACTED]"), "Debug must redact api keys");
        assert!(!debug.contains("sk-secret"));
        assert!(!debug.contains("jina-secret"));
    }

    #[test]
    fn test_rag_disabled_skips_provider_validation() {
        // disabled 时即使配置残缺也不报错（不会走到初始化）
        let settings = RagSettings {
            enabled: false,
            embedding_provider: "bogus".to_string(),
            rerank_endpoint: String::new(),
            ..RagSettings::default()
        };
        settings
            .validate_providers()
            .expect("disabled skips checks");
    }

    #[test]
    fn test_rag_enabled_rejects_unknown_provider() {
        let settings = RagSettings {
            enabled: true,
            embedding_provider: "bogus".to_string(),
            ..RagSettings::default()
        };
        let err = settings
            .validate_providers()
            .expect_err("unknown provider must fail");
        assert!(err.contains("embedding_provider"), "got: {err}");
    }

    #[test]
    fn test_rag_http_rerank_requires_endpoint_and_model() {
        let settings = RagSettings {
            enabled: true,
            rerank_provider: "http".to_string(),
            ..RagSettings::default()
        };
        let err = settings
            .validate_providers()
            .expect_err("http rerank without endpoint must fail");
        assert!(err.contains("rerank_endpoint"), "got: {err}");

        let settings = RagSettings {
            enabled: true,
            rerank_provider: "http".to_string(),
            rerank_endpoint: "https://api.jina.ai/v1/rerank".to_string(),
            ..RagSettings::default()
        };
        let err = settings
            .validate_providers()
            .expect_err("cohere format without model must fail");
        assert!(err.contains("rerank_model"), "got: {err}");
    }

    #[test]
    fn test_rag_http_rerank_tei_needs_no_model() {
        let settings = RagSettings {
            enabled: true,
            rerank_provider: "http".to_string(),
            rerank_endpoint: "http://localhost:8080/rerank".to_string(),
            rerank_format: "tei".to_string(),
            ..RagSettings::default()
        };
        settings.validate_providers().expect("tei needs no model");
    }

    #[test]
    fn test_rag_local_provider_feature_gated() {
        let settings = RagSettings {
            enabled: true,
            embedding_provider: "local".to_string(),
            ..RagSettings::default()
        };
        #[cfg(feature = "rag-local")]
        settings
            .validate_providers()
            .expect("local provider valid when rag-local compiled");
        #[cfg(not(feature = "rag-local"))]
        {
            // 无 rag-local 编译面时必须报"缺 feature"
            let err = settings.validate_providers().expect_err("must fail");
            assert!(err.contains("rag-local"), "got: {err}");
        }
    }

    #[test]
    fn test_rag_top_n_bounds() {
        let settings = RagSettings {
            enabled: true,
            search_rerank_top_n: 0,
            ..RagSettings::default()
        };
        assert!(settings.validate_providers().is_err());
    }
}
