# Proxy Examples

代理配置示例，演示如何使用 crawlrs 配置和使用HTTP代理。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `http_proxy.rs` | HTTP代理配置示例 |
| `rotate_proxies.rs` | 代理轮换示例 |
| `proxy_auth.rs` | 代理认证示例 |

## 核心功能

### 代理配置
- HTTP代理
- HTTPS代理
- SOCKS代理
- 代理认证

### 代理轮换
- 代理池管理
- 轮换策略
- 故障切换

### 认证管理
- Basic认证
- Digest认证
- 凭证安全存储

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = scrape("https://example.com")
        .proxy("http://proxy.example.com:8080")
        .await?;
    
    println!("Content length: {}", result.content.len());
    Ok(())
}
```

## 前置条件

- 准备可用的代理服务器
- 根据需要配置代理认证信息

## 相关示例

- 爬取示例：`../scrape/`
- 限流示例：`../rate-limiting/`
- 高级功能：`../advanced/`
