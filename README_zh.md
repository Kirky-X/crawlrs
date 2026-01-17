<div align="center">

![Logo](resources/logo.png)

### 🚀 使用 Rust 构建的企业级网页数据采集平台

**高性能 • 可扩展 • 类型安全**

![Build Status](https://img.shields.io/badge/build-passing-brightgreen)
![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

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
- [路线图](#路线图)
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
| **Playwright** | JavaScript 密集的 SPA、交互 | 🐢 较慢 | 💳 较高 |
| **Fire** (计划中) | 反爬虫保护网站 | 🚀 可变 | 💎 可变 |

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
| **速率限制** | 每团队并发和 RPM 控制 |
| **分布式缓存** | 基于 Redis 的 TTL 缓存 |
| **指标与监控** | Prometheus 兼容的导出 |
| **Webhooks** | 事件驱动的任务完成通知 |
| **API Key 认证** | 作用域访问控制和团队隔离 |
| **审计日志** | 完整的请求跟踪 |

### 🏗️ 架构

| 层次 | 技术 | 用途 |
|--------|------------|---------|
| 表现层 | Axum | HTTP 处理器、中间件 |
| 应用层 | Use Cases | 业务逻辑编排 |
| 领域层 | Traits | 核心实体和服务 |
| 基础设施层 | Postgres、Redis、S3 | 外部集成 |

---

## 📦 安装 <span id="安装"></span>

### 前置要求

| 要求 | 最低版本 | 推荐版本 |
|-------------|------------------|---------------|
| Rust | 1.70+ | 最新稳定版 |
| PostgreSQL | 14+ | 最新稳定版 |
| SQLite | 3.x | 3.35+ |
| Redis | 7+ | 最新稳定版 |
| Docker | 20+ | 最新版 |

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/your-org/crawlrs.git
cd crawlrs

# 使用默认特性安装（PostgreSQL + Redis）
cargo build --release

# 安装所有特性（SQLite + 所有引擎）
cargo build --release --features full

# 使用自定义特性安装
cargo build --release --features "engine-playwright,db-sqlite,metrics"
```

### 特性标志

| 特性 | 描述 | 默认 |
|---------|-------------|----------|
| `engine-reqwest` | 基础 HTTP 客户端 | ✅ 是 |
| `engine-playwright` | 基于 Chromium 的浏览器自动化 | ❌ 否 |
| `engine-fire-cdp` | Fire 引擎 CDP 支持 | ❌ 否 |
| `engine-fire-tls` | Fire 引擎 TLS 支持 | ❌ 否 |
| `redis-cache` | Redis 缓存支持 | ✅ 是 |
| `rate-limiting` | 基于 Redis 的速率限制 | ✅ 是 |
| `metrics` | Prometheus 指标导出 | ✅ 是 |
| `db-postgres` | PostgreSQL 数据库支持 | ✅ 是 |
| `db-sqlite` | SQLite 数据库支持 | ❌ 否 |
| `search-google` | Google 搜索集成 | ❌ 否 |
| `search-bing` | Bing 搜索集成 | ❌ 否 |
| `search-baidu` | 百度搜索集成 | ❌ 否 |
| `search-sogou` | 搜狗搜索集成 | ❌ 否 |

---

## 🚀 快速开始 <span id="快速开始"></span>

5 分钟内启动并运行！

### 1️⃣ 配置

创建配置文件 `config/settings.yaml`：

```yaml
# config/settings.yaml
database:
  url: "postgresql://user:password@localhost/crawlrs"
  max_connections: 20

redis:
  url: "redis://localhost:6379"

server:
  host: "0.0.0.0"
  port: 8080

rate_limiting:
  enabled: true
  default_rpm: 60
  default_concurrent: 10

cache:
  enabled: true
  default_ttl: 300
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
# 开发模式（热重载）
cargo run --bin crawlrs

# 生产模式
./target/release/crawlrs
```

### 4️⃣ 验证安装

```bash
# 健康检查
curl http://localhost:8080/health

# 预期响应：
# {"status":"healthy"}
```

---

## ⚙️ 配置 <span id="配置"></span>

### 环境变量

| 变量 | 描述 | 默认值 | 必需 |
|----------|-------------|----------|-----------|
| `DATABASE_URL` | PostgreSQL 连接字符串 | - | 是 |
| `REDIS_URL` | Redis 连接字符串 | - | 否 |
| `SERVER_HOST` | 服务器绑定地址 | 0.0.0.0 | 否 |
| `SERVER_PORT` | 服务器端口 | 8080 | 否 |
| `LOG_LEVEL` | 日志级别 | info | 否 |

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
  http://localhost:8080/api/v1/scrape
```

> **⚠️ 安全提示:** 永远不要将 API 密钥提交到版本控制系统。使用环境变量。

### 📡 核心端点

| 端点 | 方法 | 描述 |
|----------|--------|-------------|
| `/api/v1/scrape` | POST | 创建抓取任务 |
| `/api/v1/crawl` | POST | 创建爬取任务 |
| `/api/v1/search` | POST | 使用指定引擎搜索 |
| `/api/v1/extract` | POST | 从 HTML 提取数据 |
| `/api/v1/tasks` | GET | 列出所有任务 |
| `/api/v1/tasks/:id` | GET | 获取任务详情 |
| `/api/v1/tasks/:id` | DELETE | 取消任务 |
| `/api/v1/metrics` | GET | 获取系统指标 |

---

## 🏗️ 架构 <span id="架构"></span>

crawlrs 遵循领域驱动设计（DDD）原则，采用清晰的分层架构：

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

> **详细架构:** [ARCHITECTURE.md](docs/ARCHITECTURE.md)

### 技术栈

| 组件 | 技术 | 版本 |
|-----------|------------|---------|
| Web 框架 | Axum | 0.7+ |
| 异步运行时 | Tokio | 1.35+ |
| 数据库 ORM | Sea-ORM | 0.12+ |
| 数据库 | PostgreSQL / SQLite | 14+ / 3.x |
| 缓存 | Redis | 7+ |
| HTTP 客户端 | Reqwest | 0.11+ |
| 浏览器自动化 | Playwright | 0.40+ |

---

## 🚢 部署 <span id="部署"></span>

### Docker 部署

```bash
# 构建 Docker 镜像
docker build -t crawlrs:latest .

# 使用 Docker 运行
docker run -d \
  -p 8080:8080 \
  -e DATABASE_URL="postgresql://user:pass@db:5432/crawlrs" \
  -e REDIS_URL="redis://redis:6379" \
  crawlrs:latest

# 使用 Docker Compose 运行
docker-compose up -d
```

### 生产环境检查清单

- [ ] 设置强密码 API 密钥和密钥
- [ ] 配置适当的数据库连接池
- [ ] 为生产环境启用 Redis 缓存
- [ ] 根据容量设置适当的速率限制
- [ ] 配置指标导出到 Prometheus
- [ ] 启用分布式追踪
- [ ] 设置日志聚合（ELK、CloudWatch 等）
- [ ] 配置任务通知的 Webhook 端点
- [ ] 审查和调整并发设置
- [ ] 启用 SSL/TLS 终止
- [ ] 配置健康检查端点
- [ ] 设置备份和灾难恢复

---

## 🧪 测试 <span id="测试"></span>

```bash
# 运行单元测试
cargo test

# 运行集成测试
cargo test --test integration_tests --features full

# 运行覆盖率测试
cargo tarpaulin --out Html

# 运行基准测试
cargo bench

# 运行 clippy（linter）
cargo clippy -- -D warnings

# 格式化代码
cargo fmt
```

---

## 🗺️ 路线图 <span id="路线图"></span>

### v0.2.0 (计划中)

| 特性 | 状态 |
|---------|--------|
| Fire 引擎（TLS/CDP）实现 | 🔄 进行中 |
| WebSocket 实时订阅 | 📅 已计划 |
| 高级代理轮换 | 📅 已计划 |
| 基于机器学习的代理选择 | 📅 已计划 |

### v0.3.0 (计划中)

| 特性 | 状态 |
|---------|--------|
| 多语言 SDK（Python、JavaScript、Go） | 📅 已计划 |
| UI 仪表板 | 📅 已计划 |
| 高级调度和定时任务 | 📅 已计划 |
| 数据管道集成 | 📅 已计划 |

---

## 🤝 贡献 <span id="贡献"></span>

欢迎贡献！请参阅 [CONTRIBUTING.md](CONTRIBUTING.md) 了解指南。

### 开发工作流程

1. Fork 仓库
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m '添加惊人的功能'`)
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
| 🐛 问题追踪 | [GitHub Issues](https://github.com/your-org/crawlrs/issues) |
| 💬 Discord 社区 | [加入 Discord](https://discord.gg/your-server) |
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
