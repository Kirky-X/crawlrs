// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 通用 HTTP rerank provider（无条件编译，仅依赖 reqwest）
//!
//! 对接事实标准 `/rerank` REST 契约，覆盖主流远端重排服务：
//! - `RerankFormat::Cohere`：`POST {model, query, documents, top_n}` →
//!   `{results: [{index, relevance_score}]}`。兼容 Cohere v2、Jina
//!   `/v1/rerank` 及多数自建推理服务（Xinference/vLLM 等）
//! - `RerankFormat::Tei`：`POST {query, texts}` →
//!   `[{index, score}]`。HuggingFace text-embeddings-inference
//!
//! 与 rig 的取舍：rig 0.42 rerank 原生仅支持 Voyage；通用 HTTP 适配让
//! Cohere/Jina/自建端点无需逐个写 provider。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::services::rerank::{RerankError, RerankProvider, RerankScore};

/// 重排请求线格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankFormat {
    /// Cohere v2 / Jina / 多数自建端点
    Cohere,
    /// text-embeddings-inference
    Tei,
}

impl RerankFormat {
    pub fn parse(format: &str) -> Result<Self, RerankError> {
        match format.trim().to_lowercase().as_str() {
            "cohere" => Ok(Self::Cohere),
            "tei" => Ok(Self::Tei),
            other => Err(RerankError::InvalidInput(format!(
                "unknown rerank format \"{other}\" (supported: cohere, tei)"
            ))),
        }
    }
}

/// 通用 HTTP rerank provider
pub struct HttpRerankProvider {
    http: reqwest::Client,
    endpoint: String,
    /// cohere/tei 格式必填（tei 端点无 model 概念时留空）
    model: Option<String>,
    format: RerankFormat,
    /// Bearer 鉴权；`None`/空字符串不带 Authorization 头
    api_key: Option<String>,
}

#[derive(Serialize)]
struct CohereRerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    top_n: Option<usize>,
}

#[derive(Deserialize)]
struct CohereRerankResponse {
    #[serde(default)]
    results: Vec<CohereRerankResult>,
}

#[derive(Deserialize)]
struct CohereRerankResult {
    index: usize,
    relevance_score: f64,
}

#[derive(Serialize)]
struct TeiRerankRequest<'a> {
    query: &'a str,
    texts: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    return_text: Option<bool>,
}

#[derive(Deserialize)]
struct TeiRerankResult {
    index: usize,
    score: f64,
}

impl HttpRerankProvider {
    /// 构造 provider
    ///
    /// * `endpoint` - 完整重排 URL（如 `https://api.jina.ai/v1/rerank`）
    /// * `model` - 重排模型名；cohere 格式必填，tei 可为 `None`
    /// * `format` - 线格式
    /// * `api_key` - Bearer 密钥；空字符串视为未配置
    pub fn new(
        endpoint: &str,
        model: Option<&str>,
        format: RerankFormat,
        api_key: Option<&str>,
        timeout_seconds: u64,
    ) -> Result<Self, RerankError> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            return Err(RerankError::InvalidInput(
                "rerank endpoint must not be empty".to_string(),
            ));
        }
        if endpoint.parse::<url::Url>().is_err() {
            return Err(RerankError::InvalidInput(format!(
                "rerank endpoint is not a valid URL: {endpoint}"
            )));
        }
        let model = model.map(str::trim).filter(|m| !m.is_empty());
        if format == RerankFormat::Cohere && model.is_none() {
            return Err(RerankError::InvalidInput(
                "cohere rerank format requires a model name".to_string(),
            ));
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()
            .map_err(|e| RerankError::Provider(format!("failed to build http client: {e}")))?;

        Ok(Self {
            http,
            endpoint: endpoint.to_string(),
            model: model.map(str::to_string),
            format,
            api_key: api_key
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string),
        })
    }

    fn validate_input(&self, query: &str, documents: &[String]) -> Result<(), RerankError> {
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

    async fn request(&self, body: serde_json::Value) -> Result<reqwest::Response, RerankError> {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        request
            .send()
            .await
            .map_err(|e| RerankError::Provider(format!("rerank request failed: {e}")))?
            .error_for_status()
            .map_err(|e| RerankError::Provider(format!("rerank endpoint returned error: {e}")))
    }
}

#[async_trait::async_trait]
impl RerankProvider for HttpRerankProvider {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: Option<usize>,
    ) -> Result<Vec<RerankScore>, RerankError> {
        self.validate_input(query, documents)?;

        let scores: Vec<RerankScore> = match self.format {
            RerankFormat::Cohere => {
                let body = CohereRerankRequest {
                    model: self.model.as_deref().unwrap_or_default(),
                    query,
                    documents,
                    top_n: top_k,
                };
                let response: CohereRerankResponse = self
                    .request(serde_json::to_value(body).expect("serialize cohere request"))
                    .await?
                    .json()
                    .await
                    .map_err(|e| {
                        RerankError::Provider(format!("invalid cohere rerank response: {e}"))
                    })?;
                response
                    .results
                    .into_iter()
                    .map(|r| RerankScore {
                        index: r.index,
                        score: r.relevance_score as f32,
                    })
                    .collect()
            }
            RerankFormat::Tei => {
                let body = TeiRerankRequest {
                    query,
                    texts: documents,
                    return_text: Some(false),
                };
                let response: Vec<TeiRerankResult> = self
                    .request(serde_json::to_value(body).expect("serialize tei request"))
                    .await?
                    .json()
                    .await
                    .map_err(|e| {
                        RerankError::Provider(format!("invalid tei rerank response: {e}"))
                    })?;
                response
                    .into_iter()
                    .map(|r| RerankScore {
                        index: r.index,
                        score: r.score as f32,
                    })
                    .collect()
            }
        };

        if scores.is_empty() {
            return Err(RerankError::Provider(
                "rerank response contained no results".to_string(),
            ));
        }
        Ok(scores)
    }

    fn name(&self) -> &str {
        "http"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn docs() -> Vec<String> {
        vec![
            "doc-a".to_string(),
            "doc-b".to_string(),
            "doc-c".to_string(),
        ]
    }

    #[tokio::test]
    async fn test_cohere_format_request_and_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/rerank"))
            .and(header("Authorization", "Bearer test-key"))
            .and(body_json(serde_json::json!({
                "model": "rerank-v2",
                "query": "q",
                "documents": docs(),
                "top_n": 2
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"index": 2, "relevance_score": 0.9},
                    {"index": 0, "relevance_score": 0.1}
                ]
            })))
            .mount(&server)
            .await;

        let provider = HttpRerankProvider::new(
            &format!("{}/v1/rerank", server.uri()),
            Some("rerank-v2"),
            RerankFormat::Cohere,
            Some("test-key"),
            5,
        )
        .expect("provider");

        let scores = provider
            .rerank("q", &docs(), Some(2))
            .await
            .expect("rerank");
        assert_eq!(scores.len(), 2);
        assert_eq!(scores[0].index, 2);
        assert!((scores[0].score - 0.9).abs() < 1e-6);
        assert_eq!(scores[1].index, 0);
    }

    #[tokio::test]
    async fn test_tei_format_request_and_response() {
        let server = MockServer::start().await;
        // TEI：请求用 texts/score 字段，响应是顶层数组
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .and(body_json(serde_json::json!({
                "query": "q",
                "texts": docs(),
                "return_text": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"index": 1, "score": 0.77}
            ])))
            .mount(&server)
            .await;

        let provider = HttpRerankProvider::new(
            &format!("{}/rerank", server.uri()),
            None,
            RerankFormat::Tei,
            None,
            5,
        )
        .expect("provider");

        let scores = provider.rerank("q", &docs(), None).await.expect("rerank");
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].index, 1);
        assert!((scores[0].score - 0.77).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_upstream_error_maps_to_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let provider =
            HttpRerankProvider::new(&server.uri(), Some("m"), RerankFormat::Cohere, None, 5)
                .expect("provider");

        let err = provider
            .rerank("q", &docs(), None)
            .await
            .expect_err("must fail");
        assert!(matches!(err, RerankError::Provider(_)));
    }

    #[tokio::test]
    async fn test_empty_results_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .mount(&server)
            .await;

        let provider =
            HttpRerankProvider::new(&server.uri(), Some("m"), RerankFormat::Cohere, None, 5)
                .expect("provider");

        let err = provider
            .rerank("q", &docs(), None)
            .await
            .expect_err("must fail");
        assert!(err.to_string().contains("no results"));
    }

    #[test]
    fn test_constructor_rejects_bad_input() {
        assert!(HttpRerankProvider::new("", Some("m"), RerankFormat::Cohere, None, 5).is_err());
        assert!(
            HttpRerankProvider::new("not a url", Some("m"), RerankFormat::Cohere, None, 5).is_err()
        );
        // cohere 格式必须有 model
        assert!(HttpRerankProvider::new(
            "https://x.dev/rerank",
            None,
            RerankFormat::Cohere,
            None,
            5
        )
        .is_err());
        // tei 无 model 合法
        assert!(
            HttpRerankProvider::new("https://x.dev/rerank", None, RerankFormat::Tei, None, 5)
                .is_ok()
        );
    }

    #[test]
    fn test_format_parse() {
        assert_eq!(RerankFormat::parse("cohere").unwrap(), RerankFormat::Cohere);
        assert_eq!(RerankFormat::parse(" TEI ").unwrap(), RerankFormat::Tei);
        assert!(RerankFormat::parse("bogus").is_err());
    }
}
