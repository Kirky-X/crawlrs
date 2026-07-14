# Cache Examples

缓存配置示例，演示如何使用 crawlrs 的 oxcache 缓存功能。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `oxcache_cache.rs` | oxcache 缓存示例 |
| `cache_configuration.rs` | 缓存配置示例 |
| `ttl_management.rs` | TTL管理示例 |

## 核心功能

### oxcache 集成
- 内存缓存配置
- TTL 管理
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
    let cache = Cache::builder()
        .default_ttl(3600)
        .build()?;
    
    let result = cache.get("key").await?;
    Ok(())
}
```

## 前置条件

- oxcache 使用内存后端（moka），无需外部服务

## 相关示例

- 爬取示例：`../scrape/`
- 爬取示例：`../crawl/`
- 基础架构示例：`../rate-limiting/`
