# crawlrs Examples

crawlrs 项目示例集合，包含所有功能特性的使用示例。

## 目录结构

```
examples/
├── search/              # 搜索功能示例
│   ├── test_google.rs
│   ├── test_bing.rs
│   ├── test_baidu.rs
│   ├── test_sogou.rs
│   ├── test_unified_search.rs
│   └── test_smart_search.rs
├── scrape/              # 单页爬取示例
│   ├── basic_scrape.rs
│   ├── multi_format_output.rs
│   ├── form_extraction.rs
│   ├── screenshot.rs
│   ├── page_actions.rs
│   ├── custom_headers.rs
│   └── extraction_rules.rs
├── crawl/               # 整站爬取示例
│   ├── basic_crawl.rs
│   ├── depth_control.rs
│   ├── filter_patterns.rs
│   ├── robots_compliance.rs
│   ├── status_tracking.rs
│   └── result_pagination.rs
├── extract/             # 数据提取示例
│   ├── css_selector.rs
│   ├── xpath_extraction.rs
│   ├── llm_extraction.rs
│   ├── structured_data.rs
│   ├── multi_rules.rs
│   └── output_formats.rs
├── map/                 # 数据可视化示例
│   └── README.md
├── browser/             # 浏览器引擎示例
│   ├── test_playwright_basic.rs
│   ├── test_google_homepage.rs
│   ├── engine_router_demo.rs
│   └── test_flaresolverr_direct.rs
├── auth/                # 认证授权示例
│   ├── api_key_auth.rs
│   ├── bearer_token.rs
│   ├── team_isolation.rs
│   └── scope_validation.rs
├── teams/               # 团队管理示例
│   ├── basic_teams.rs
│   ├── geo_restrictions.rs
│   └── credits_management.rs
├── webhooks/            # Webhook示例
│   ├── basic_webhook.rs
│   ├── task_events.rs
│   └── retry_logic.rs
├── cache/               # 缓存示例
│   ├── redis_cache.rs
│   ├── cache_configuration.rs
│   └── ttl_management.rs
├── rate-limiting/       # 限流示例
│   ├── basic_rate_limit.rs
│   ├── team_concurrency.rs
│   └── circuit_breaker.rs
├── proxy/               # 代理示例
│   ├── http_proxy.rs
│   ├── rotate_proxies.rs
│   └── proxy_auth.rs
├── advanced/            # 高级功能示例
│   ├── async_batch.rs
│   ├── error_handling.rs
│   ├── metrics_export.rs
│   ├── async_streams.rs
│   └── custom_engines.rs
├── text_encoding/       # 文本编码示例
│   └── text_encoding_integration_demo.rs
├── README.md            # 本文件
└── QUICKSTART.md        # 快速开始指南
```

## 功能特性对应示例

| 功能 | 目录 | 示例数量 |
|-----|------|---------|
| 搜索 | `search/` | 6 |
| 爬取 | `scrape/` | 7 |
| 爬取 | `crawl/` | 6 |
| 提取 | `extract/` | 6 |
| 可视化 | `map/` | 1+ |
| 浏览器 | `browser/` | 5 |
| 认证 | `auth/` | 4 |
| 团队 | `teams/` | 3 |
| Webhook | `webhooks/` | 3 |
| 缓存 | `cache/` | 3 |
| 限流 | `rate-limiting/` | 3 |
| 代理 | `proxy/` | 3 |
| 高级 | `advanced/` | 5 |
| 编码 | `text_encoding/` | 1 |
| **总计** | | **56+** |

## 使用方法

### 运行单个示例

```bash
# 运行搜索示例
cargo run --example test_google

# 运行爬取示例
cargo run --example basic_scrape

# 运行所有示例（需要full特性）
cargo run --features full --examples
```

### 运行示例组

```bash
# 运行所有搜索相关示例
cargo run --example search

# 运行所有浏览器示例
cargo run --example browser
```

## 先决条件

### 必需
- Rust 1.70+
- PostgreSQL 14+ 或 SQLite 3.35+

### 可选
- Redis 7+（用于缓存和限流）
- Chrome/Chromium（用于Playwright）
- LLM API密钥（用于LLM提取）

## 快速开始

建议按照以下顺序学习：

1. **初学者**
   - [QUICKSTART.md](./QUICKSTART.md)
   - `scrape/basic_scrape.rs`
   - `search/test_google.rs`

2. **中级用户**
   - `crawl/basic_crawl.rs`
   - `extract/css_selector.rs`
   - `browser/test_playwright_basic.rs`

3. **高级用户**
   - `advanced/async_batch.rs`
   - `advanced/custom_engines.rs`
   - 所有企业功能示例

## 贡献指南

### 添加新示例

1. 确定示例所属的功能目录
2. 参考同类示例的代码风格
3. 添加完整的文档字符串
4. 创建对应的README条目（如果需要）
5. 在 `examples/mod.rs` 中导出模块

### 示例规范

- 每个示例应该是独立的
- 包含Apache 2.0许可证头
- 使用 `tracing` 进行日志记录
- 包含错误处理
- 文档字符串说明用途和用法

## 相关文档

- [项目README](../README.md)
- [API文档](../docs/API_REFERENCE.md)
- [用户指南](../docs/USER_GUIDE.md)
- [架构文档](../docs/ARCHITECTURE.md)

## 许可证

所有示例均遵循 Apache License 2.0。
