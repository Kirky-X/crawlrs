# Rate Limiting Examples

限流配置示例，演示如何使用 crawlrs 的速率限制功能。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_rate_limit.rs` | 基础限流配置示例 |
| `team_concurrency.rs` | 团队并发控制示例 |
| `circuit_breaker.rs` | 熔断器示例 |

## 核心功能

### API级限流
- 请求频率限制
- 并发请求限制
- 桶算法实现

### 团队级限流
- 团队配额管理
- 公平调度
- 优先级控制

### 熔断保护
- 故障检测
- 自动熔断
- 恢复策略

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rate_limiter = RateLimiter::builder()
        .requests_per_second(10)
        .max_concurrent(100)
        .build()?;
    
    let permit = rate_limiter.acquire().await?;
    // 执行请求
    Ok(())
}
```

## 前置条件

- 限流使用 limiteron MemoryStorage（无需外部服务）
- 根据需要配置适当的限制值

## 相关示例

- 代理示例：`../proxy/`
- 缓存示例：`../cache/`
- 高级功能：`../advanced/`
