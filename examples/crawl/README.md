# Crawl Examples

整站爬取功能示例，演示如何使用 crawlrs 进行全站递归爬取和数据采集。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_crawl.rs` | 基础整站爬取示例 |
| `depth_control.rs` | 深度控制爬取示例 |
| `filter_patterns.rs` | URL过滤模式示例 |
| `robots_compliance.rs` | robots.txt合规示例 |
| `status_tracking.rs` | 爬取状态跟踪示例 |
| `result_pagination.rs` | 结果分页示例 |

## 核心功能

### 基础爬取
- 全站递归爬取
- 起始URL配置
- 链接发现和跟踪

### 爬取控制
- 深度限制（Depth Limit）
- URL模式过滤（Include/Exclude）
- 爬取速率控制
- 并发控制

### 合规性
- robots.txt 协议支持
- 请求频率限制
- 域名限制

### 状态管理
- 爬取任务创建和监控
- 进度跟踪
- 结果获取和分页
- 任务取消

## 快速开始

```rust
use crawlrs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crawl = crawl("https://example.com")
        .max_depth(3)
        .max_pages(100)
        .await?;
    
    println!("Crawl ID: {}", crawl.id);
    Ok(())
}
```

## 前置条件

- 确保已配置好数据库连接
- 分布式缓存由 oxcache 自动管理
- 建议配置适当的请求延迟以避免被封禁

## 相关示例

- 搜索示例：`../search/`
- 缓存示例：`../cache/`
- 限流示例：`../rate-limiting/`
