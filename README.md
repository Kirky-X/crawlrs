<div align="center">

![Logo](docs/image/logo.png)

### 🚀 使用 Rust 构建的企业级网页数据采集平台

**高性能 • 可扩展 • 类型安全**

[![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=build)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/releases) [![License](https://img.shields.io/github/license/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE) ![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

</div>

## 📖 目录

- [概述](#概述)
- [性能基准](#性能基准)
- [核心特性](#核心特性)
- [安装](#安装)
- [快速开始](#快速开始)
- [配置](#配置)
- [API 文档](#api-文档)
- [架构](#架构)
- [部署](#部署)
- [测试](#测试)
- [贡献](#贡献)
- [许可证](#许可证)
- [支持](#支持)

---

## 📝 概述 <span id="概述"></span>

**crawlrs** 是一个面向开发者的高性能企业级网页数据采集平台，提供全面的数据采集能力：

| 能力 | 描述 |
|------------|-------------|
| 🔍 **搜索** | 统一的 Google、Bing、百度和搜狗搜索 |
| 🎯 **抓取** | 从单个网页提取数据 |
| 🕷️ **爬取** | 自动发现并爬取多个页面 |
| 📊 **提取** | 从 HTML 解析和结构化数据 |
| 🗺️ **映射** | 可视化和组织爬取的数据 |

采用 Rust 构建，crawlrs 提供卓越的性能：

| 指标 | 提升幅度 |
|--------|-------------|
| **吞吐量** | 相比 Node.js 提升 3-5 倍 |
| **P99 延迟** | 降低 50% |
| **内存使用** | 降低 75% |
| **CPU 使用** | 降低 59% |

---

## 📊 性能基准 <span id="性能基准"></span>

与 Node.js 实现相比：

| 指标 | Node.js 版本 | Rust 版本 (crawlrs) | 提升 |
|--------|----------------|----------------------|------|
| 吞吐量 | 1,200 请求/秒 | 4,500 请求/秒 | **3.75x** |
| P99 延迟 | 450ms | 180ms | **60%** |
| 内存使用 | 512 MB | 128 MB | **75%** |
| CPU 使用 | 85% | 35% | **59%** |

---

## ✨ 核心特性 <span id="核心特性"></span>

### 🚀 高性能

| 特性 | 优势 |
|---------|---------|
| 3-5 倍吞吐量提升 | 更快的数据采集 |
| 50% 的 P99 延迟降低 | 实时响应时间 |
| 零成本抽象 | Rust 的安全性保证无额外开销 |
| 内存效率 | 相比 Node.js 降低 75% 的内存使用 |

### 🔍 多引擎支持

| 引擎 | 用例 | 性能 | 成本 |
|--------|----------|------------|-------|
| **Reqwest** | 静态 HTML、API 响应 | ⚡ 最快 | 💰 最低 |
| **chromiumoxide** | JavaScript 密集的 SPA、交互 | 🐢 较慢 | 💳 较高 |
| **FlareSolverr** | 反爬虫保护网站（Full/Cdp/Tls 三种模式） | 🚀 可变 | 💎 可变 |

### 🔎 统一搜索

| 能力 | 描述 |
|------------|-------------|
| 多引擎支持 | Google、Bing、百度、搜狗 |
| A/B 测试 | 跨引擎比较结果 |
| 自动去重 | 删除重复结果 |
| 结果聚合 | 统一的输出格式 |

### 📊 企业级功能

| 特性 | 描述 |
|---------|-------------|
| **速率限制** | 每团队并发和 RPM 控制（基于 limiteron，支持分布式限流与熔断） |
| **缓存** | 基于 oxcache 的多层缓存（L1 内存 moka 后端），支持 search/dns/regex 分类型 TTL |
| **指标与监控** | Prometheus 兼容的导出 |
| **Webhooks** | 事件驱动的任务完成通知 |
| **API Key 认证** | 作用域访问控制和团队隔离 |
| **审计日志** | 完整的请求跟踪 |
| **代理支持** | 统一出站代理配置 |
| **LLM 抽取** | 基于 genai 的 LLM 内容抽取 |

### 🏗️ 架构

| 层次 | 技术 | 用途 |
|--------|------------|---------|
| 表现层 | Axum | HTTP 处理器、中间件 |
| 应用层 | Use Cases | 业务逻辑编排 |
| 领域层 | Traits | 核心实体和服务 |
| 基础设施层 | Postgres | 外部集成 |

---

## 📦 安装 <span id="安装"></span>

### 前置要求

| 要求 | 最低版本 | 推荐版本 |
|-------------|------------------|---------------|
| Rust | 1.70+ | 最新稳定版 |
| PostgreSQL | 14+ | 最新稳定版 |
| Docker | 20+ | 最新版 |

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/YOUR_ORG/crawlrs.git
cd crawlrs

# 使用 standard 预设安装（核心栈 + engine-playwright + metrics）
cargo build --release --features standard

# 安装所有特性（standard + engine-flaresolverr）
cargo build --release --features full

# 使用自定义特性安装
cargo build --release --features "engine-playwright,metrics"
```

### 特性标志

> **注意：** `default = []` — 默认不启用任何特性。使用预设（`standard` / `full`）或显式列出所需特性。

> **核心栈为非可选。** 核心依赖（oxcache 0.3 / dbnexus 0.4 / confers 0.4 / limiteron 0.2 / sdforge 0.4 / inklog 0.1 / trait-kit 0.3 + scraper / chardetng / encoding_rs / robotstxt）与 HTTP 抓取栈始终编译，不再以 feature 形式暴露。

| 特性 | 描述 | 默认 |
|---------|-------------|----------|
| `engine-playwright` | 基于 chromiumoxide 的浏览器自动化 | ❌ 否 |
| `engine-flaresolverr` | FlareSolverr 反爬虫保护（FlareSolverrMode 枚举区分 Full/Cdp/Tls 三模式） | ❌ 否 |
| `metrics` | Prometheus 指标导出 | ❌ 否 |
| `genai-llm` | 基于 genai 的 LLM 抽取 | ❌ 否 |
| `browser-download` | 自动下载 Playwright 浏览器 | ❌ 否 |
| `test-mocks` | 测试专用 mock 模块（integration test 需显式启用） | ❌ 否 |
| `admin-tools` | 运维 CLI 工具（如 add_credits） | ❌ 否 |

> **说明：** `openapi` 不是 Cargo feature——它是 `sdforge_macros` 的 `#[forge]` 宏生成的 cfg 标记，用于 OpenAPI 规范输出。用户无需显式启用；sdforge 总是编译，openapi 自动生效。

### 预设与编译体积

本项目通过 Cargo 特性控制可选功能。核心栈（oxcache / dbnexus / confers / limiteron / sdforge / inklog / trait-kit + scraper / chardetng / encoding_rs / robotstxt + HTTP 抓取栈）始终编译，不再以 feature 形式暴露。

| 预设 | 特性组合 | 二进制大小 | 适用场景 |
|-----|---------|-----------|---------|
| standard | `engine-playwright, metrics` | ~35MB | 需要 JS 渲染（核心栈默认包含） |
| full | `standard + engine-flaresolverr` | ~52MB | 所有功能 |

> **注意：** `default = []` 不出现在预设表中，因为它不启用任何可选特性，仅编译核心栈（约 ~30MB）；用于按需显式启用场景。

### 自定义组合

```bash
# 自定义组合：核心栈始终编译，仅需指定可选特性
cargo build --release --features "engine-playwright,metrics,genai-llm"

# 仅核心栈（无任何可选特性）
cargo build --release --no-default-features
```

### 特性参考

| 特性 | 描述 | 影响 |
|------|------|------|
| `engine-playwright` | chromiumoxide JS 渲染引擎 | +8MB |
| `engine-flaresolverr` | FlareSolverr 引擎（通过 FlareSolverrMode 枚举区分 Full/Cdp/Tls 三种模式） | - |
| `metrics` | 指标监控 | - |
| `genai-llm` | genai LLM 抽取 | - |
| `browser-download` | 自动下载 Playwright 浏览器 | - |
| `test-mocks` | 测试 mock 模块（`#[cfg(any(test, feature = "test-mocks"))]`） | - |
| `admin-tools` | 运维 CLI 工具（`cargo run --bin add_credits --features admin-tools`） | - |

---

## 🚀 快速开始 <span id="快速开始"></span>

5 分钟内启动并运行！

### 1️⃣ 配置

创建配置文件 `config/default.toml`：

```toml
# config/default.toml
[database]
url = "postgresql://user:password@localhost/crawlrs"
max_connections = 20

[server]
host = "0.0.0.0"
port = 8899

[cors]
allowed_origins = "*"

[rate_limiting]
enabled = true
default_rpm = 60
default_limit = 60
burst_size = 20

[cache]
enabled = true

[cache.memory]
capacity = 10000
ttl_seconds = 300

[cache.types.search]
ttl_seconds = 300
max_size = 10000

[cache.types.dns]
ttl_seconds = 3600
max_size = 1000

[cache.types.regex]
ttl_seconds = 86400
max_size = 5000

[search]
default_engine = "baidu"
[search.engines]
google_enabled = true
bing_enabled = true
baidu_enabled = true
sogou_enabled = true
```

### 2️⃣ 数据库设置

```bash
# 使用内置 CLI 运行迁移
cargo run --bin crawlrs -- migrate

# 或使用 SQLx CLI
sqlx database create
sqlx migrate run
```

### 3️⃣ 运行服务器

```bash
# 开发模式
cargo run --bin crawlrs

# 生产模式
./target/release/crawlrs
```

### 4️⃣ 验证安装

```bash
# 健康检查
curl http://localhost:8899/health

# 预期响应：
# {"status":"healthy","version":"0.1.0"}
```

---

## ⚙️ 配置 <span id="配置"></span>

crawlrs 使用 confers 管理配置，支持 TOML 文件和 `CRAWLRS__` 前缀的环境变量（`__` 用于嵌套层级）。默认配置文件：`config/default.toml`。

### 环境变量

| 环境变量 | 描述 | 默认值 | 必需 |
|-------------|----------|--------|------|
| `CRAWLRS__DATABASE__URL` | PostgreSQL 连接字符串 | - | 是 |
| `CRAWLRS__SERVER__HOST` | 服务器绑定地址 | 0.0.0.0 | 否 |
| `CRAWLRS__SERVER__PORT` | 服务器端口 | 8899 | 否 |
| `CRAWLRS__CONCURRENCY__DEFAULT_TEAM_LIMIT` | 每团队默认并发限制 | 10 | 否 |
| `CRAWLRS__CACHE__MEMORY__CAPACITY` | 内存缓存容量 | 10000 | 否 |
| `CRAWLRS__CACHE__MEMORY__TTL_SECONDS` | 内存缓存 TTL | 300 | 否 |
| `CRAWLRS__WEBHOOK__TIMEOUT_SECONDS` | Webhook 调用超时 | 10 | 否 |
| `CRAWLRS__WORKERS__COUNT` | Worker 数量（"auto" 或数字） | auto | 否 |
| `CRAWLRS__PROXY__URL` | 出站代理 URL | - | 否 |
| `CRAWLRS__LLM__API_KEY` | LLM 服务 API 密钥 | - | 否 |
| `CRAWLRS__ENGINES__FLARESOLVERR__URL` | FlareSolverr 服务 URL | http://localhost:8191/v1 | 否 |
| `CRAWLRS__LOG_LEVEL` | 日志级别 | info | 否 |
| `CRAWLRS__DATABASE__PASSWORD` | 数据库密码（Docker 模式） | - | 否 |

### 配置参考

| 配置段 | 描述 | 关键字段 |
|--------|------|---------|
| `[server]` | 服务器绑定 | `host`, `port`, `enable_port_detection` |
| `[cors]` | CORS 跨域 | `allowed_origins`（逗号分隔，`*` 通配） |
| `[database]` | 数据库连接 | `url`, `max_connections`, `min_connections`, `connect_timeout` |
| `[rate_limiting]` | 速率限制 | `enabled`, `default_rpm`, `default_limit`, `burst_size` |
| `[cache]` | 缓存控制 | `enabled`, `[cache.memory]` (capacity/ttl), `[cache.types.*]` (search/dns/regex) |
| `[concurrency]` | 并发控制 | `default_team_limit`, `task_lock_duration_seconds` |
| `[search]` | 搜索配置 | `default_engine`, `ab_test_enabled`, `timeout_seconds` |
| `[webhook]` | Webhook | `timeout_seconds`, `max_retries`, `secret`, `batch_size` |
| `[proxy]` | 出站代理 | `url`, `enabled` |
| `[llm]` | LLM 抽取 | `api_key`, `model`, `api_base_url` |
| `[workers]` | Worker 池 | `count`（`"auto"` 或数字） |
| `[engines.flaresolverr]` | FlareSolverr | `enabled`, `url`, `timeout_seconds` |
| `[logging]` | 日志输出 | `[logging.console]`, `[logging.file]` (path/max_file_size/file_count) |
| `[trusted_proxies]` | 可信代理 | `enabled`, `proxies`（CIDR 列表） |

---

## 📚 API 文档 <span id="api-文档"></span>

> **完整 API 参考:** [API_REFERENCE.md](docs/API_REFERENCE.md) | **用户指南:** [USER_GUIDE.md](docs/USER_GUIDE.md)

### 🔑 认证

所有受保护的端点都需要在 `Authorization` 头中提供 API 密钥：

```bash
# 格式
Authorization: Bearer YOUR_API_KEY

# 示例 curl
curl -H "Authorization: Bearer crawlrs_sk_abc123" \
  http://localhost:8899/v1/scrape
```

> **⚠️ 安全提示:** 永远不要将 API 密钥提交到版本控制系统。使用环境变量。

### 📡 公开端点

| 端点 | 方法 | 描述 |
|----------|--------|-------------|
| `/health` | GET | 健康检查（liveness probe） |
| `/metrics` | GET | Prometheus 指标 |
| `/v1/version` | GET | 版本号 |

### 📡 核心受保护端点

| 端点 | 方法 | 描述 |
|----------|--------|-------------|
| `/v1/scrape` | POST | 创建抓取任务 |
| `/v1/scrape/{id}` | GET | 获取任务详情 |
| `/v1/scrape/{id}/_cancel` | POST | 取消抓取任务 |
| `/v1/crawl` | POST | 创建爬取任务 |
| `/v1/crawl/{id}` | GET | 获取爬取状态 |
| `/v1/crawl/{id}` | DELETE | 取消爬取任务 |
| `/v1/crawl/{id}/_cancel` | POST | 取消爬取任务 |
| `/v1/crawl/{id}/results` | GET | 获取爬取结果 |
| `/v1/search` | POST | 使用指定引擎搜索 |
| `/v1/extract` | POST | 从 HTML 提取数据 |
| `/v1/webhooks` | POST | 创建 webhook |
| `/v1/webhooks` | GET | 列出 webhook |
| `/v1/teams/me` | GET | 获取当前团队信息 |
| `/v1/teams/me/usage` | GET | 获取团队使用量 |
| `/v1/teams/geo-restrictions` | GET | 获取团队地理限制 |
| `/v1/teams/geo-restrictions` | PUT | 更新团队地理限制 |
| `/v1/tasks/_query` | POST | 复杂查询任务 |
| `/v1/tasks/_cancel` | POST | 批量取消任务 |
| `/v1/audit/logs` | GET | 获取审计日志 |
| `/v1/audit/denied` | GET | 获取被拒绝的请求 |

### 📡 SDK 端点

| 端点 | 方法 | 描述 |
|----------|--------|-------------|
| `/api/v1/sdk/search` | POST | SDK 搜索 |
| `/api/v1/sdk/tasks` | POST | SDK 创建任务 |
| `/api/v1/sdk/scrape` | POST | SDK 创建抓取 |
| `/api/v1/sdk/crawl` | POST | SDK 创建爬取 |

---

## 🏗️ 架构 <span id="架构"></span>

crawlrs 遵循领域驱动设计（DDD）原则，采用清晰的四层架构：

```mermaid
flowchart TB
    subgraph Presentation [表现层 - Axum]
        A[HTTP 处理器]
        B[中间件]
        C[路由]
    end

    subgraph Application [应用层]
        D[Use Cases]
        E[DTOs]
        F[请求验证]
    end

    subgraph Domain [领域层]
        G[Models]
        H[Services]
        I[Repository 接口]
    end

    subgraph Infrastructure [基础设施层]
        J[Database]
        K[Cache]
        L[Storage]
        M[External APIs]
    end

    Presentation --> Application --> Domain --> Infrastructure
```

> **详细架构:** [ARCHITECTURE.md](docs/ARCHITECTURE.md)

### 引擎架构

- **EngineClient**: 唯一的公开入口点，封装所有抓取操作的统一 API
- **EngineRouter**: 引擎调度核心，使用 `Vec<Arc<dyn ScraperEngine>>` 存储引擎实例，通过配置策略选择最佳引擎
- **路由策略**: 默认使用 `SmartHybrid` 策略（智能混合），可选 `RaceMode`（并发竞速）/ `SequentialFallback`（顺序降级）

### 技术栈

| 组件 | 技术 | 版本 |
|-----------|------------|---------|
| Web 框架 | Axum | 0.8 |
| 异步运行时 | Tokio | 1.52 |
| 数据库 ORM | Sea-ORM 2.0.0-rc.43（通过 dbnexus 0.4） | - |
| 数据库 | PostgreSQL | 14+ |
| 缓存 | oxcache (moka) | 0.3 |
| HTTP 客户端 | Reqwest | 0.13 |
| 浏览器自动化 | chromiumoxide | 0.9 |
| 结构化日志 | inklog | 0.1 |
| API SDK | sdforge | 0.4 |
| 多后端缓存 | oxcache | 0.3 |
| 速率限制 | limiteron | 0.2 |
| 配置管理 | confers | 0.4 |
| DI 框架 | trait-kit | 0.3 |
| HTML 解析 | scraper | 0.27 |

---

## 🚢 部署 <span id="部署"></span>

### Docker 部署

```bash
# 构建 Docker 镜像
docker build -t crawlrs:latest .

# 使用 Docker 运行
docker run -d \
  -p 8899:8899 \
  -e CRAWLRS__DATABASE__URL="postgresql://user:pass@db:5432/crawlrs" \
  crawlrs:latest

# 使用 Docker Compose 运行
docker-compose up -d
```

### 生产环境检查清单

- [ ] 设置强密码 API 密钥和密钥
- [ ] 配置适当的数据库连接池
- [ ] 为生产环境配置 oxcache 缓存（search/dns/regex 分类型 TTL）
- [ ] 根据容量设置适当的速率限制（`default_limit` / `burst_size`）
- [ ] 配置 CORS 为具体来源（非 `*` 通配符）
- [ ] 配置指标导出到 Prometheus
- [ ] 启用分布式追踪（inklog HTTP sink）
- [ ] 设置日志聚合（ELK、CloudWatch 等）
- [ ] 配置任务通知的 Webhook 端点
- [ ] 审查和调整并发设置（`concurrency.default_team_limit`）
- [ ] 配置可信代理（`trusted_proxies`）防止 IP 伪造
- [ ] 启用 SSL/TLS 终止
- [ ] 配置健康检查端点
- [ ] 设置备份和灾难恢复

---

## 🧪 测试 <span id="测试"></span>

```bash
# 运行单元测试
cargo test --features default --lib --verbose

# 运行集成测试（需要 Docker：PostgreSQL + Redis via testcontainers）
cargo test --test integration_tests --features full

# 运行 SDK API 测试
cargo test --features test-mocks --test sdk_api_test

# 运行完整主测试入口
cargo test --features standard,test-mocks --test main

# 运行覆盖率测试
cargo tarpaulin --out Html

# 运行基准测试
cargo bench

# 运行 clippy（linter）
cargo clippy --features default -- -D warnings

# 完整 clippy 检查（全部特性）
cargo clippy --features full -- -D warnings

# 格式化代码
cargo fmt --all -- --check

# 依赖安全检查
cargo deny check

# Pre-commit 完整检查
scripts/pre-commit-check.sh all
```

---

## 🤝 贡献 <span id="贡献"></span>

欢迎贡献！请参阅 [CONTRIBUTING.md](CONTRIBUTING.md) 了解指南。

### 开发工作流程

1. Fork 仓库
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'feat: 添加惊人功能'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 开启 Pull Request

### 代码风格

- 遵循 Rust 命名约定
- 为公共 API 添加文档注释
- 为新功能编写测试
- 保持函数聚焦且简短

---

## 📄 许可证 <span id="许可证"></span>

本项目在 Apache License 2.0 下获得许可 - 详见 [LICENSE](LICENSE) 文件。

```
Copyright 2025 Kirky.X

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

---

## 💬 支持 <span id="支持"></span>

| 资源 | 链接 |
|----------|------|
| 📖 文档 | [docs/](docs/) |
| 📚 API 参考 | [API_REFERENCE.md](docs/API_REFERENCE.md) |
| 👤 用户指南 | [USER_GUIDE.md](docs/USER_GUIDE.md) |
| 🏗️ 架构 | [ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| 🐛 问题追踪 | [GitHub Issues](https://github.com/YOUR_ORG/crawlrs/issues) |
| 📧 邮箱 | [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com) |

---

## 🙏 致谢

- 使用 [Rust](https://www.rust-lang.org/) 构建
- Web 框架由 [Axum](https://github.com/tokio-rs/axum) 驱动
- 数据库 ORM 来自 [Sea-ORM](https://www.sea-ql.org/)
- 灵感来源于对高性能网页爬取解决方案的需求

---

<div align="center">

**使用 ❤️ 在 Rust 中构建**

[⬆ 返回顶部](#概述)

</div>
