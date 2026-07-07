// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM智能提取示例
//!
//! 演示如何使用LLM进行智能数据提取。
//!
//! # 功能特点
//!
//! - 自然语言规则定义
//! - 上下文理解
//! - 复杂结构识别
//! - JSON输出
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example llm_extraction
//! ```
//!
//! # 前置条件
//!
//! 需要配置LLM API密钥才能使用此功能。

use log::info;

/// LLM提取配置
#[derive(Debug)]
struct LlmExtractionConfig {
    pub model: String,
    pub api_key: Option<String>,
    pub prompt: String,
    pub output_format: String,
}

impl Default for LlmExtractionConfig {
    fn default() -> Self {
        Self {
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            prompt: "从以下内容中提取信息".to_string(),
            output_format: "json".to_string(),
        }
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始LLM智能提取示例");
    info!("=====================================\n");

    // 1. LLM提取介绍
    info!("1️⃣  LLM智能提取介绍");
    info!("-----------------------------");
    info!("");
    info!("📖 LLM智能提取使用大型语言模型进行数据提取");
    info!("   相比传统CSS/XPath选择器，具有以下优势:");
    info!("   - 不需要了解HTML结构");
    info!("   - 可以理解语义内容");
    info!("   - 适应页面结构变化");
    info!("   - 支持复杂的数据提取需求");
    info!("");

    // 2. 配置示例
    info!("2️⃣  LLM提取配置");
    info!("-----------------------------");

    let config = LlmExtractionConfig {
        model: "gpt-4".to_string(),
        api_key: Some("sk-...".to_string()),
        prompt: "从以下商品页面中提取：商品名称、价格、描述、评分".to_string(),
        output_format: "json".to_string(),
    };

    info!("📋 提取配置:");
    info!("   模型: {}", config.model);
    info!(
        "   API密钥: {}***",
        &config.api_key.as_deref().unwrap_or("")[..3]
    );
    info!("   提示词: {}", config.prompt);
    info!("   输出格式: {}", config.output_format);
    info!("");

    // 3. 使用场景
    info!("3️⃣  使用场景示例");
    info!("-----------------------------");
    info!("");

    info!("📝 场景1: 商品信息提取");
    let product_prompt = r#"
从以下电商商品页面中提取:
- 商品名称
- 价格 (数字)
- 商品描述
- 评分 (1-5)
- 是否有库存

输出JSON格式:
{
  "name": "...",
  "price": 0.0,
  "description": "...",
  "rating": 0.0,
  "in_stock": true/false
}
"#;
    info!("   提示词长度: {} 字符", product_prompt.len());
    info!("");

    info!("📝 场景2: 文章信息提取");
    let article_prompt = r#"
从以下文章页面中提取:
- 文章标题
- 作者
- 发布时间
- 阅读量
- 标签列表

输出JSON格式:
{
  "title": "...",
  "author": "...",
  "published_at": "...",
  "read_count": 0,
  "tags": [...]
}
"#;
    info!("   提示词长度: {} 字符", article_prompt.len());
    info!("");

    info!("📝 场景3: 联系信息提取");
    let contact_prompt = r#"
从以下页面中提取所有联系信息:
- 电话号码
- 邮箱地址
- 社交媒体链接
- 物理地址

输出JSON格式:
{
  "phones": [...],
  "emails": [...],
  "social_media": [...],
  "address": "..."
}
"#;
    info!("   提示词长度: {} 字符", contact_prompt.len());
    info!("");

    // 4. 实际提取示例
    info!("4️⃣  实际提取示例");
    info!("-----------------------------");

    let sample_html = r#"
    <html>
    <body>
        <div class="product">
            <h1>Apple iPhone 15 Pro Max</h1>
            <p class="price">$1,199.00</p>
            <div class="description">
                The most advanced iPhone ever with A17 Pro chip.
            </div>
            <div class="rating">4.8 out of 5 stars</div>
            <div class="stock">In Stock</div>
        </div>
    </body>
    </html>
    "#;

    info!("📄 示例HTML片段:");
    info!("   商品名: Apple iPhone 15 Pro Max");
    info!("   价格: $1,199.00");
    info!("   描述: The most advanced iPhone ever with A17 Pro chip.");
    info!("   评分: 4.8/5");
    info!("   库存: 有");
    info!("");

    info!("🔄 使用LLM提取:");
    info!("   提示词: 从商品页面中提取商品名称、价格、描述、评分、库存状态");
    info!("   模型: gpt-4");
    info!("");

    // 模拟LLM响应
    info!("📊 模拟LLM响应:");
    info!("   {{");
    info!("     \"name\": \"Apple iPhone 15 Pro Max\",");
    info!("     \"price\": 1199.00,");
    info!("     \"description\": \"The most advanced iPhone ever with A17 Pro chip.\",");
    info!("     \"rating\": 4.8,");
    info!("     \"in_stock\": true");
    info!("   }}");
    info!("");

    // 5. 最佳实践
    info!("5️⃣  LLM提取最佳实践");
    info!("-----------------------------");
    info!("");
    info!("✅ 建议:");
    info!("   - 提供清晰的提取指令");
    info!("   - 指定期望的输出格式");
    info!("   - 提供示例帮助LLM理解");
    info!("   - 处理LLM可能出现的错误");
    info!("");
    info!("❌ 避免:");
    info!("   - 过于模糊的指令");
    info!("   - 过长的HTML输入");
    info!("   - 依赖不稳定的页面结构");
    info!("   - 忽略错误处理");
    info!("");

    info!("💰 成本考虑:");
    info!("   - LLM调用需要API费用");
    info!("   - 建议缓存提取结果");
    info!("   - 对简单任务使用传统方法");

    info!("\n=====================================");
    info!("✨ LLM智能提取示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - LLM提取适合复杂/非结构化数据");
    info!("   - 简单任务建议使用CSS选择器");
    info!("   - 记得处理API错误和重试");
    info!("   - 考虑添加提取结果验证");
}
