// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! API 密钥认证示例
//!
//! 演示如何通过 HTTP API 使用 API Key 进行身份验证：
//! - 通过 `x-api-key` 请求头传递 API Key
//! - 通过 `Authorization: Bearer <key>` 传递
//! - 处理 401/403 认证/授权错误
//!
//! # 前提条件
//!
//! 需要先启动 crawlrs 服务（启用 auth feature）并获取有效 API Key。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example api_key_auth
//! ```

use log::info;
use std::collections::HashMap;

/// 模拟 API 请求（无需额外 HTTP 客户端依赖）
struct ApiRequest {
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
}

impl ApiRequest {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            headers: HashMap::new(),
            body: None,
        }
    }

    fn with_api_key(mut self, key: &str) -> Self {
        self.headers.insert("x-api-key".to_string(), key.to_string());
        self
    }

    fn with_bearer_token(mut self, token: &str) -> Self {
        self.headers
            .insert("Authorization".to_string(), format!("Bearer {}", token));
        self
    }

    fn with_json_body(mut self, body: &str) -> Self {
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(body.to_string());
        self
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 API 密钥认证示例");
    info!("=====================================\n");

    let base_url = "http://localhost:8899";

    // 1. 使用 x-api-key 请求头认证
    info!("1️⃣  通过 x-api-key 请求头认证");
    info!("-----------------------------");

    let request = ApiRequest::new(&format!("{}/v1/scrape", base_url))
        .with_api_key("your-api-key-here")
        .with_json_body(r#"{"url": "https://example.com"}"#);

    info!("  POST /v1/scrape");
    info!("  Headers:");
    for (key, value) in &request.headers {
        let display_value = if key == "x-api-key" {
            // 脱敏显示
            let len = value.len();
            format!("{}...{}", &value[..4], &value[len - 4..])
        } else {
            value.clone()
        };
        info!("    {}: {}", key, display_value);
    }
    info!("  Body: {}", request.body.as_deref().unwrap_or("{}"));
    info!("");
    info!("  💡 crawlrs 支持两种 API Key 传递方式:");
    info!("     - x-api-key: <your-key>  (推荐)");
    info!("     - Authorization: Bearer <your-key>");

    info!("");

    // 2. 使用 Bearer Token 认证
    info!("2️⃣  通过 Bearer Token 认证");
    info!("-----------------------------");

    let request = ApiRequest::new(&format!("{}/v1/scrape", base_url))
        .with_bearer_token("your-api-key-here")
        .with_json_body(r#"{"url": "https://example.com"}"#);

    info!("  POST /v1/scrape");
    info!("  Headers:");
    for (key, value) in &request.headers {
        info!("    {}: {}", key, value);
    }
    info!("");

    // 3. 处理认证错误
    info!("3️⃣  认证错误处理");
    info!("-----------------------------");
    info!("  常见认证错误:");
    info!("");
    info!("  401 Unauthorized — 缺少或无效的 API Key");
    info!("    原因: 未提供 API Key 或 Key 已过期/被撤销");
    info!("    处理: 检查 Key 是否正确，联系管理员重新签发");
    info!("");
    info!("  403 Forbidden — Key 有效但权限不足");
    info!("    原因: API Key 的 scope 不包含请求的操作");
    info!("    处理: 联系管理员提升 scope（read/write/admin）");
    info!("");

    // 4. API Key 管理
    info!("4️⃣  API Key 管理（需 admin 权限）");
    info!("-----------------------------");
    info!("  创建 API Key:");
    info!("    POST /v1/admin/api-keys");
    info!("    {{\"team_id\": \"<uuid>\", \"name\": \"my-key\", \"scope\": {{\"read\": true, \"write\": true}}}}");
    info!("");
    info!("  撤销 API Key:");
    info!("    DELETE /v1/admin/api-keys/<key_id>");
    info!("");
    info!("  💡 最佳实践:");
    info!("     - 每个应用/服务使用独立的 API Key");
    info!("     - 遵循最小权限原则配置 scope");
    info!("     - 定期轮换 API Key");
    info!("     - 不要在客户端代码中硬编码 Key，使用环境变量");

    info!("\n=====================================");
    info!("✨ API 密钥认证示例完成");
}
