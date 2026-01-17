# Extract Examples

数据提取功能示例，演示如何使用 crawlrs 从网页内容中提取结构化数据。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `css_selector.rs` | CSS选择器提取示例 |
| `xpath_extraction.rs` | XPath提取示例 |
| `llm_extraction.rs` | LLM智能提取示例 |
| `structured_data.rs` | 结构化数据提取示例 |
| `multi_rules.rs` | 多提取规则示例 |
| `output_formats.rs` | 输出格式示例 |

## 核心功能

### CSS选择器提取
- 元素选择
- 属性提取
- 文本提取
- 嵌套选择

### XPath提取
- 路径表达式
- 条件筛选
- 属性匹配

### LLM智能提取
- 自然语言规则定义
- 上下文理解
- 复杂结构识别
- JSON输出

### 多规则提取
- 批量提取规则
- 规则优先级
- 冲突处理

### 输出格式
- JSON格式
- CSV格式
- 自定义格式

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html = "<html><body><h1>Title</h1></body></html>";
    
    let extracted = extract(html)?
        .select("h1")
        .text()
        .await?;
    
    println!("Extracted: {}", extracted);
    Ok(())
}
```

## 前置条件

- 对于LLM提取，需要配置LLM API密钥
- 确保已安装必要的依赖

## 相关示例

- 爬取示例：`../scrape/`
- 高级功能：`../advanced/`
