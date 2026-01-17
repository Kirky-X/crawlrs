# Cache Examples

缓存配置示例，演示如何使用 crawlrs 的Redis缓存功能。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `redis_cache.rs` | Redis缓存示例 |
| `cache_configuration.rs` | 缓存配置示例 |
| `ttl_management.rs` | TTL管理示例 |

## 核心功能

### Redis集成
- 连接配置
- 连接池管理
- 故障恢复

### 缓存策略
- 页面缓存
- 结果缓存
- 请求缓存

### TTL管理
- 全局TTL设置
- 动态TTL
- TTL刷新策略

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache = Cache::redis()
        .url("redis://localhost:6379")
        .default_ttl(3600)
        .build()?;
    
    let result = cache.get("key").await?;
    Ok(())
}
```

## 前置条件

- 确保Redis服务可用
- 根据需要配置Redis集群

## 相关示例

- 爬取示例：`../scrape/`
- 爬取示例：`../crawl/`
- 基础架构示例：`../rate-limiting/`
