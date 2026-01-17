# Scrape Examples

单页爬取功能示例，演示如何使用 crawlrs 进行单个网页的内容获取和数据提取。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_scrape.rs` | 基础HTML爬取示例 |
| `multi_format_output.rs` | 多格式输出示例 |
| `form_extraction.rs` | 表单数据提取示例 |
| `screenshot.rs` | 页面截图示例 |
| `page_actions.rs` | 页面交互示例 |
| `custom_headers.rs` | 自定义请求头示例 |
| `extraction_rules.rs` | 提取规则示例 |

## 核心功能

### 基础爬取
- HTTP请求发送和响应处理
- 多种输出格式支持（HTML、Markdown、JSON、Screenshot）
- 自定义请求头和User-Agent

### 数据提取
- CSS选择器提取
- 表单数据提取
- 结构化数据提取
- LLM智能提取

### 页面交互
- 等待（Wait）
- 点击（Click）
- 滚动（Scroll）
- 输入（Input）
- 截图（Screenshot）

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = scrape("https://example.com")
        .format("markdown")
        .await?;
    
    println!("{}", result.content);
    Ok(())
}
```

## 前置条件

- 确保已配置好数据库连接
- 根据需要启用相应的引擎特性
- 对于JavaScript渲染，需要启用 `engine-playwright` 特性

## 相关示例

- 浏览器引擎示例：`../browser/`
- 数据提取示例：`../extract/`
- 代理配置示例：`../proxy/`
