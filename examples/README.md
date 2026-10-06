# crawlrs Examples

crawlrs 项目示例集合，包含所有功能特性的使用示例。

## 示例 package

本目录是 crawlrs 根 workspace 的成员 package（主 `Cargo.toml` 的 `members = ["examples"]`），作为独立 crate 编译与运行；在 `examples/` 目录下执行 cargo 命令即可。

### 运行示例

```bash
# 在 examples 目录下运行（示例为根 workspace 的成员 package，绝大多数已在 examples/Cargo.toml 注册为 [[bin]]，须用 --bin 而非 --example）
cd examples

# 运行单个示例
cargo run --bin basic_scrape

# 运行依赖浏览器/引擎特性的示例（examples/Cargo.toml 提供 engine-playwright / engine-flaresolverr / engine-tls-fingerprint）
cargo run --bin mllm_scrape

# 编译所有示例
cargo build
```

## 目录结构

```
examples/
├── advanced/            # 高级功能示例
│   ├── async_batch.rs
│   ├── async_streams.rs
│   ├── custom_engines.rs
│   ├── error_handling.rs
│   ├── metrics_export.rs
│   └── mllm_scrape.rs
├── auth/                # 认证授权示例
│   ├── api_key_auth.rs
│   ├── bearer_token.rs
│   ├── scope_validation.rs
│   └── team_isolation.rs
├── browser/             # 浏览器引擎示例（需 Playwright / FlareSolverr / 真网）
│   ├── test_all_engines.rs
│   ├── test_flaresolverr_direct.rs
│   ├── test_google_homepage.rs
│   ├── test_google_preview.rs
│   └── test_playwright_basic.rs
├── cache/               # 缓存示例
│   ├── cache_configuration.rs
│   ├── oxcache_cache.rs
│   └── ttl_management.rs
├── config/              # 配置示例
│   ├── basic_config.rs
│   ├── env_override.rs
│   └── validation.rs
├── crawl/               # 整站爬取示例
│   ├── basic_crawl.rs
│   ├── depth_control.rs
│   ├── filter_patterns.rs
│   ├── result_pagination.rs
│   ├── robots_compliance.rs
│   └── status_tracking.rs
├── database/            # 数据库示例（需 PostgreSQL）
│   ├── basic_crud.rs
│   ├── migration.rs
│   └── transaction.rs
├── extract/             # 数据提取示例
│   ├── css_selector.rs
│   ├── llm_extraction.rs
│   ├── multi_rules.rs
│   ├── output_formats.rs
│   ├── structured_data.rs
│   └── xpath_extraction.rs
├── map/                 # 数据可视化示例（当前仅 README.md）
│   └── README.md
├── proxy/               # 代理示例
│   ├── http_proxy.rs
│   ├── proxy_auth.rs
│   └── rotate_proxies.rs
├── rate-limiting/       # 限流示例
│   ├── basic_rate_limit.rs
│   ├── circuit_breaker.rs
│   └── team_concurrency.rs
├── scrape/              # 单页抓取示例
│   ├── basic_scrape.rs
│   ├── custom_headers.rs
│   ├── extraction_prompt.rs
│   ├── extraction_rules.rs
│   ├── form_extraction.rs
│   ├── multi_format_output.rs
│   ├── page_actions.rs
│   └── screenshot.rs
├── sdk/                 # SDK 接入示例
│   ├── basic_sdk.rs
│   └── custom_middleware.rs
├── search/              # 搜索功能示例（需真实搜索引擎 / API Key）
│   ├── test_baidu.rs
│   ├── test_bing.rs
│   ├── test_fallback_engines.rs
│   ├── test_smart_search.rs
│   ├── test_smart_search_demo.rs
│   ├── test_sogou.rs
│   └── test_unified_search.rs
├── tasks/               # 任务管理示例
│   └── task_management.rs
├── teams/               # 团队管理示例
│   ├── basic_teams.rs
│   ├── credits_management.rs
│   └── geo_restrictions.rs
├── webhooks/            # Webhook 示例
│   ├── basic_webhook.rs
│   ├── retry_logic.rs
│   └── task_events.rs
├── src/                 # 示例共享辅助代码
├── README.md            # 本文件
└── QUICKSTART.md        # 快速开始指南
```

## 功能特性对应示例

| 功能 | 目录 | 示例数量 |
|-----|------|---------|
| 高级 | `advanced/` | 6 |
| 认证 | `auth/` | 4 |
| 浏览器 | `browser/` | 5 |
| 缓存 | `cache/` | 3 |
| 配置 | `config/` | 3 |
| 整站爬取 | `crawl/` | 6 |
| 数据库 | `database/` | 3 |
| 提取 | `extract/` | 6 |
| 可视化 | `map/` | 0（仅 README） |
| 代理 | `proxy/` | 3 |
| 限流 | `rate-limiting/` | 3 |
| 单页抓取 | `scrape/` | 8 |
| SDK | `sdk/` | 2 |
| 搜索 | `search/` | 7 |
| 任务 | `tasks/` | 1 |
| 团队 | `teams/` | 3 |
| Webhook | `webhooks/` | 3 |
| **总计** | | **66 个示例（全部已在 `Cargo.toml` 注册为 `[[bin]]`）** |

## 使用方法

### 运行单个示例

```bash
# 运行搜索示例（需真实搜索引擎 / API Key）
cargo run --bin test_unified_search

# 运行抓取示例
cargo run --bin basic_scrape

# 编译并检查所有示例目标
cargo build --all-targets
```

### 按目录运行示例组

示例为扁平的 `[[bin]]` 目标，按 bin 名单逐个运行，例如搜索类：

```bash
for b in test_baidu test_bing test_sogou test_unified_search; do
  cargo run --bin "$b"
done
```

## 先决条件

### 必需
- Rust 1.97+（与主项目 `Cargo.toml` 的 `rust-version` 一致）
- PostgreSQL 16+ 或 SQLite 3.35+（视示例所需）

### 可选
- Chrome/Chromium（用于Playwright）
- LLM API密钥（用于LLM提取）

## 快速开始

建议按照以下顺序学习：

1. **初学者**
   - [QUICKSTART.md](./QUICKSTART.md)
   - `scrape/basic_scrape.rs`
   - `search/test_unified_search.rs`

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
4. 在 `Cargo.toml` 中添加新的 `[[bin]]` 入口

### 示例规范

- 每个示例应该是独立的
- 包含Apache 2.0许可证头
- 使用 `log::` facade 进行日志记录（对齐主项目，非 `tracing`）
- 包含错误处理
- 文档字符串说明用途和用法

## 相关文档

- [项目README](../README.md)
- [API文档](../docs/API_REFERENCE.md)
- [用户指南](../docs/USER_GUIDE.md)
- [架构文档](../docs/ARCHITECTURE.md)

## 许可证

所有示例均遵循 Apache License 2.0。
