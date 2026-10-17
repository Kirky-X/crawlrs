// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 视觉模型适配器
//!
//! 封装 genai crate 的多模态（vision）调用，将 base64 截图 + 文本 prompt
//! 构造为 `ContentPart::Binary` + `ContentPart::Text` 的 multipart 消息，
//! 发送给视觉大模型并返回文本响应。

use anyhow::Result;
use async_trait::async_trait;

/// 解析视觉调用的自定义 OpenAI 兼容端点（F12 运维要领）
///
/// 模型带 `openai:` 前缀且 `CRAWLRS__MLLM__VISION_API_BASE_URL` 非空时，
/// 返回 `Some((base_url, api_key))`——视觉调用改指自托管/第三方网关
/// （如企业 OpenAI 兼容聚合端点）；否则 `None` 走 provider 原生路由
/// （gemini/ollama 等按各自默认端点与密钥环境变量）。
fn custom_vision_endpoint(model: &str) -> Option<(String, String)> {
    if !model.starts_with("openai:") {
        return None;
    }
    let base = std::env::var("CRAWLRS__MLLM__VISION_API_BASE_URL").ok()?;
    if base.is_empty() {
        return None;
    }
    let key = std::env::var("CRAWLRS__MLLM__VISION_API_KEY").unwrap_or_default();
    Some((base, key))
}

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
    /// * `model` - 模型标识符（如 "gemini:gemini-2.0-flash"）
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

/// genai 视觉模型适配器
///
/// 使用 `genai` crate 的 `ContentPart::from_binary_base64` 构造视觉消息，
/// 通过 genai Client 发送到指定的视觉模型。
pub struct GenaiVisionAdapter {
    /// genai 客户端（短生命周期，每次请求创建）
    _private: (),
}

impl GenaiVisionAdapter {
    /// 创建新的视觉适配器
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for GenaiVisionAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VisionAdapterTrait for GenaiVisionAdapter {
    async fn send_vision_request(
        &self,
        screenshot_b64: &str,
        prompt: &str,
        system_prompt: &str,
        model: &str,
    ) -> Result<String> {
        use genai::chat::{ChatMessage, ChatRequest};
        use genai::chat::{ContentPart, MessageContent};

        // 构造 multipart 用户消息：截图 + 文本 prompt
        let image_part = ContentPart::from_binary_base64(
            "image/jpeg",
            screenshot_b64,
            Some("screenshot.jpg".to_string()),
        );
        let text_part = ContentPart::from_text(prompt);
        let user_content = MessageContent::from_parts(vec![image_part, text_part]);
        let user_message = ChatMessage::user(user_content);

        // 构造完整请求（system + user）
        let system_message = ChatMessage::system(system_prompt.to_string());
        let chat_req = ChatRequest::new(vec![system_message, user_message]);

        // 自定义 OpenAI 兼容端点：模型带 openai: 前缀且配置了
        // CRAWLRS__MLLM__VISION_API_BASE_URL/API_KEY 时，绑定 OpenAI 适配器
        // 并以 ServiceTargetResolver 全权接管端点与鉴权。绑定 adapter_kind
        // 是必须的——否则 genai 对未识别模型名回退嗅探为 Ollama（POST
        // /api/chat 打到网关必然 404）
        let client = match custom_vision_endpoint(model) {
            Some((base, key)) => genai::Client::builder()
                .with_adapter_kind(genai::adapter::AdapterKind::OpenAI)
                .with_service_target_resolver(
                    genai::resolver::ServiceTargetResolver::from_resolver_fn(
                        move |mut target: genai::ServiceTarget| {
                            target.endpoint = genai::resolver::Endpoint::from_owned(base.clone());
                            target.auth = genai::resolver::AuthData::from_single(key.clone());
                            // 剥掉 openai: 前缀，保留裸模型名
                            let name = target.model.model_name.as_str();
                            let bare = name.strip_prefix("openai:").unwrap_or(name);
                            target.model =
                                genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, bare);
                            Ok(target)
                        },
                    ),
                )
                .build(),
            None => genai::Client::default(),
        };

        let chat_res = client
            .exec_chat(model, chat_req, None)
            .await
            .map_err(|e| anyhow::anyhow!("Vision LLM call failed for model {}: {:?}", model, e))?;

        let content = chat_res
            .first_text()
            .ok_or_else(|| anyhow::anyhow!("Vision LLM returned empty content"))?
            .to_string();

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自定义端点解析：openai: 前缀 + 非空 base_url 才命中；空 base 视为未配置
    #[test]
    fn custom_vision_endpoint_requires_prefix_and_base() {
        std::env::remove_var("CRAWLRS__MLLM__VISION_API_BASE_URL");
        std::env::remove_var("CRAWLRS__MLLM__VISION_API_KEY");
        // 未配置环境变量：任何模型都走 provider 原生路由
        assert!(custom_vision_endpoint("openai:gateway-model").is_none());
        assert!(custom_vision_endpoint("gemini:gemini-2.0-flash").is_none());

        std::env::set_var(
            "CRAWLRS__MLLM__VISION_API_BASE_URL",
            "https://gw.example.com/v1",
        );
        std::env::set_var("CRAWLRS__MLLM__VISION_API_KEY", "sk-test");
        // 非 openai 前缀：即使配置了端点也不走自定义路由（原生 provider 语义不变）
        assert!(custom_vision_endpoint("gemini:gemini-2.0-flash").is_none());
        let (base, key) =
            custom_vision_endpoint("openai:gateway-model").expect("openai 前缀应命中");
        assert_eq!(base, "https://gw.example.com/v1");
        assert_eq!(key, "sk-test");

        // 空 base_url 视为未配置
        std::env::set_var("CRAWLRS__MLLM__VISION_API_BASE_URL", "");
        assert!(custom_vision_endpoint("openai:gateway-model").is_none());

        std::env::remove_var("CRAWLRS__MLLM__VISION_API_BASE_URL");
        std::env::remove_var("CRAWLRS__MLLM__VISION_API_KEY");
    }
    use std::sync::Arc;

    /// Mock 视觉适配器 — 返回预设响应
    struct MockVisionAdapter {
        response: String,
        call_count: Arc<std::sync::atomic::AtomicU32>,
    }

    impl MockVisionAdapter {
        fn new(response: &str) -> (Self, Arc<std::sync::atomic::AtomicU32>) {
            let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
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
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_mock_vision_adapter_call_count() {
        let (adapter, count) = MockVisionAdapter::new(r#"{"action": "done"}"#);

        for _ in 0..3 {
            let _ = adapter
                .send_vision_request("img", "prompt", "sys", "model")
                .await;
        }

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn test_genai_vision_adapter_construction() {
        let adapter = GenaiVisionAdapter::new();
        // 验证可以构造（不实际发送请求）
        let _ = adapter;
    }

    #[test]
    fn test_genai_vision_adapter_default() {
        let adapter = GenaiVisionAdapter::default();
        let _ = adapter;
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
