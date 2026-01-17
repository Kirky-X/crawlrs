# Webhooks Examples

Webhook配置示例，演示如何使用 crawlrs 配置和管理任务事件通知。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_webhook.rs` | 基础Webhook配置示例 |
| `task_events.rs` | 任务事件订阅示例 |
| `retry_logic.rs` | 重试逻辑示例 |

## 核心功能

### Webhook配置
- Webhook端点注册
- 事件类型选择
- 负载配置

### 事件类型
- 爬取开始事件
- 爬取完成事件
- 爬取失败事件
- 进度更新事件

### 重试机制
- 自动重试策略
- 重试间隔配置
- 最大重试次数

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let webhook = Webhook::create()
        .url("https://your-server.com/webhook")
        .events(["crawl.completed", "crawl.failed"])
        .await?;
    
    println!("Webhook ID: {}", webhook.id);
    Ok(())
}
```

## 前置条件

- 确保有可用的Webhook接收端点
- 根据需要配置TLS/SSL

## 相关示例

- 任务管理示例：`../crawl/`
- 高级功能：`../advanced/`
