// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 视觉模型适配器
//!
//! 经 fluxweir SDK（`FluxweirClient::chat`）承载多模态调用：base64 截图
//! 编码为 `data:image/jpeg;base64,` URL 块 + 文本 prompt 构造 multipart
//! 消息（OpenAI wire `image_url` 语义），发送给视觉模型并返回文本响应
//!（变更 `fluxweir-llm-gateway` design D6）。

use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "gateway")]
use std::collections::BTreeMap;
#[cfg(feature = "gateway")]
use std::sync::Arc;

/// 视觉模型适配 trait — 抽象视觉请求的发送与响应
///
/// 允许 MllmEngine 在测试中替换为 mock 实现。
#[async_trait]
pub trait VisionAdapterTrait: Send + Sync {
    /// 发送视觉分析请求
    ///
    /// # 参数
    ///
    /// * `screenshot_b64` - 页面截图的 base64 编码
    /// * `prompt` - 分析提示词（指导模型如何分析截图）
    /// * `system_prompt` - 系统提示词（设定模型角色与约束）
    /// * `model` - 模型标识符（`openai:`/`gemini:` 前缀被剥除，取裸名作
    ///   fluxweir 抽象模型名）
    ///
    /// # 返回值
    ///
    /// * `Ok(String)` - 模型的文本响应
    /// * `Err(anyhow::Error)` - 调用失败
    async fn send_vision_request(
        &self,
        screenshot_b64: &str,
        prompt: &str,
        system_prompt: &str,
        model: &str,
    ) -> Result<String>;
}

/// fluxweir 承载的视觉模型适配器
///
/// `client = None`（`[fluxweir].enabled = false`）时调用显性报错——vision
/// 请求经网关承载是 engine-mllm 的唯一路径（`engine-mllm` feature 显式依赖
/// `gateway`，无单独编译态），不静默降级、不直连上游。
#[cfg(feature = "gateway")]
pub struct FluxweirVisionAdapter {
    /// fluxweir 客户端（嵌入式回环或外部实例）；None = 网关未启用
    client: Option<Arc<fluxweir::sdk::FluxweirClient>>,
}

#[cfg(feature = "gateway")]
impl FluxweirVisionAdapter {
    /// `client = None` 时所有请求返回带配置指引的结构化错误
    pub fn new(client: Option<Arc<fluxweir::sdk::FluxweirClient>>) -> Self {
        Self { client }
    }
}

#[cfg(feature = "gateway")]
#[async_trait]
impl VisionAdapterTrait for FluxweirVisionAdapter {
    async fn send_vision_request(
        &self,
        screenshot_b64: &str,
        prompt: &str,
        system_prompt: &str,
        model: &str,
    ) -> Result<String> {
        let Some(client) = &self.client else {
            anyhow::bail!(
                "vision request requires the fluxweir gateway: set [fluxweir].enabled=true \
                 (mode embedded/external) — engine-mllm carries vision via the gateway"
            );
        };

        // genai 时代的适配器前缀（openai:/gemini:）剥除，取裸名作 fluxweir
        // 抽象模型名（网关按模型路由厂商）
        let model_name = model.rsplit(':').next().unwrap_or(model);

        // multipart 用户消息：截图（data URL）+ 文本 prompt；输入已是 base64，
        // 直接拼 data URL（避免解码-再编码损耗）
        let image_part = fluxweir::sdk::ChatContentPart::ImageUrl {
            image_url: fluxweir::sdk::ImageUrlBody {
                url: format!("data:image/jpeg;base64,{screenshot_b64}"),
            },
        };
        let text_part = fluxweir::sdk::ChatContentPart::text(prompt);
        let user_message =
            fluxweir::sdk::ChatMessage::user(fluxweir::sdk::ChatContent::Parts(vec![
                image_part, text_part,
            ]));
        let system_message = fluxweir::sdk::ChatMessage::system(system_prompt.to_string());

        let chat_req = fluxweir::sdk::ChatRequest {
            model: model_name.to_string(),
            messages: vec![system_message, user_message],
            config: fluxweir::sdk::RequestConfig::default(),
            tools: Vec::new(),
            tool_choice: None,
            vendor_extensions: BTreeMap::new(),
        };

        let response = client
            .chat(&chat_req)
            .await
            .map_err(|e| anyhow::anyhow!("Vision LLM call failed for model {}: {:?}", model, e))?;

        if response.content.trim().is_empty() {
            anyhow::bail!("Vision LLM returned empty content");
        }
        Ok(response.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// 网关未启用：调用返回结构化错误并携带配置指引（不 panic、不静默）
    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn disabled_gateway_returns_structured_error() {
        let adapter = FluxweirVisionAdapter::new(None);
        let err = adapter
            .send_vision_request("b64", "prompt", "system", "openai:gw-model")
            .await
            .expect_err("must error without client");
        let rendered = format!("{err:#}");
        assert!(rendered.contains("[fluxweir].enabled=true"), "{rendered}");
        assert!(rendered.contains("fluxweir gateway"), "{rendered}");
    }

    /// 模型前缀剥除语义（rsplit 取末段：适配器前缀与带冒号模型名都安全）
    #[test]
    fn model_name_strips_adapter_prefix() {
        let prefixed = "openai:gw-model";
        assert_eq!(prefixed.rsplit(':').next().unwrap_or(prefixed), "gw-model");
        let plain = "qwen3-vl";
        assert_eq!(plain.rsplit(':').next().unwrap_or(plain), "qwen3-vl");
        let namespaced = "vendor:family:model";
        assert_eq!(namespaced.rsplit(':').next().unwrap_or(namespaced), "model");
    }

    /// Mock 视觉适配器 — 返回预设响应
    struct MockVisionAdapter {
        response: String,
        call_count: Arc<AtomicU32>,
    }

    impl MockVisionAdapter {
        fn new(response: &str) -> (Self, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            (
                Self {
                    response: response.to_string(),
                    call_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl VisionAdapterTrait for MockVisionAdapter {
        async fn send_vision_request(
            &self,
            _screenshot_b64: &str,
            _prompt: &str,
            _system_prompt: &str,
            _model: &str,
        ) -> Result<String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn test_mock_vision_adapter_returns_response() {
        let json_response = r##"{"action": "click", "selector": "#btn", "reasoning": "test"}"##;
        let (adapter, count) = MockVisionAdapter::new(json_response);

        let result = adapter
            .send_vision_request("base64data", "analyze", "system", "test-model")
            .await
            .unwrap();

        assert_eq!(result, json_response);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_mock_vision_adapter_call_count() {
        let (adapter, count) = MockVisionAdapter::new(r#"{"action": "done"}"#);

        for _ in 0..3 {
            let _ = adapter
                .send_vision_request("img", "prompt", "sys", "model")
                .await;
        }

        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn test_fluxweir_vision_adapter_construction() {
        // None 形态可构造（调用期显性报错）；Some 形态由装配面注入 client
        let disabled = FluxweirVisionAdapter::new(None);
        let _ = disabled;
    }

    /// 验证 trait object 可用性
    #[tokio::test]
    async fn test_vision_adapter_trait_object() {
        let (adapter, _) = MockVisionAdapter::new(r#"{"action": "extract"}"#);
        let boxed: Box<dyn VisionAdapterTrait> = Box::new(adapter);

        let result = boxed
            .send_vision_request("img", "prompt", "sys", "model")
            .await
            .unwrap();

        assert!(result.contains("extract"));
    }
}
