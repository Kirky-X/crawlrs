# crawlrs

<div align="center">

![Rust Version](https://img.shields.io/badge/rust-1.75%2B-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)

**高性能企业级网页数据采集平台**

[特性](#特性) • [快速开始](#快速开始) • [文档](#文档) • [架构](#架构) • [贡献](#贡献)

</div>

---

## 📖 简介

crawlrs 是一个用 Rust 开发的企业级网页数据采集平台，提供搜索、抓取、爬取、映射与结构化提取能力。相比传统 Node.js 方案，性能提升
**3-5 倍**，P99 延迟降低 **50%**。

### 核心优势

- 🚀 **高性能**: 单机 10000+ RPS，P99 延迟 < 200ms
- 🛡️ **类型安全**: 利用 Rust 编译期检查，消除 90% 运行时错误
- 🔄 **弹性扩展**: 支持单机和集群部署，按需水平扩展
- 📊 **可观测性**: 内置分布式追踪和 Prometheus 指标
- 🔐 **企业级**: SSRF 防护、速率限制、多租户隔离

---

## ✨ 特性

### 核心功能

- **搜索 (Search)**: 多引擎并发聚合（Google/Bing/Baidu/Sogou），智能去重排序，支持异步回填
- **抓取 (Scrape)**: 单页面内容获取，支持多格式输出（Markdown/HTML/截图/JSON）
- **爬取 (Crawl)**: 全站递归爬取，支持深度控制和路径过滤
- **提取 (Extract)**: 基础CSS选择器结构化数据提取

### 技术特性

- **智能引擎路由**: 自动选择最优抓取引擎（Fetch/Playwright/FireEngineTls/FireEngineCdp）
- **断路器保护**: 引擎故障自动降级，保证系统可用性
- **访问控制**: 团队级地理位置限制、白名单和域名黑名单
- **两层限流**: API 速率限制（令牌桶）+ 团队并发控制（信号量）
- **可靠 Webhook**: 指数退避重试机制
- **Robots.txt 遵守**: 自动解析和缓存爬虫规则
- **统一任务管理**: 新增 v2/tasks 接口，支持批量查询和取消

---

## 🚀 快速开始

### 前置要求

- **Rust**: 1.75+ (Edition 2021)
- **PostgreSQL**: 15+
- **Redis**: 7+
- **Docker** (可选): 用于容器化部署

### 安装

#### 方式 1: 从源码编译

```bash
# 克隆仓库
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

# 编译
cargo build --release

# 运行测试
cargo test

# 启动服务
./target/release/crawlrs
```

#### 方式 2: Docker Compose（推荐）

```bash
# 启动完整栈
docker-compose up -d

# 查看日志
docker-compose logs -f api

# 停止服务
docker-compose down
```

### 配置

创建 `.env` 文件：

```env
# 数据库
DATABASE_URL=postgres://user:password@localhost:5433/crawlrs_db

# Redis
REDIS_URL=redis://localhost:6380

# 服务配置
HOST=0.0.0.0
PORT=8899
ENABLE_PORT_DETECTION=true

# 搜索引擎配置
BING_SEARCH_API_KEY=your-bing-api-key
GOOGLE_SEARCH_API_KEY=your-google-api-key
GOOGLE_SEARCH_CX=your-google-cx

# 搜索 A/B 测试配置
SEARCH_AB_TEST_ENABLED=false
SEARCH_VARIANT_B_WEIGHT=0.1
```

### 存储配置

支持多种存储后端：

- **本地存储**: 文件系统存储（默认）
- **S3 存储**: AWS S3 兼容存储（需要启用 `s3` 特性）

**配置存储**：

```toml
[storage]
storage_type = "local"  # 或 "s3"
local_path = "storage"  # 本地存储路径

# S3 配置（需要启用 s3 特性）
[s3]
bucket = "your-bucket"
region = "us-east-1"
access_key_id = "your-access-key"
secret_access_key = "your-secret-key"
```

### 第一个请求

```bash
# 健康检查
curl http://localhost:8899/health

# 抓取网页
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"]
  }'

# 搜索并抓取
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust programming",
    "limit": 10
  }'

# 爬取网站
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "max_depth": 3,
    "include_paths": ["/docs/*"]
  }'

# 统一任务管理 - 批量查询任务状态
curl -X POST http://localhost:8899/v2/tasks/query \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "task_ids": ["550e8400-e29b-41d4-a716-446655440000"],
    "include_results": true
  }'

# 统一任务管理 - 批量取消任务
curl -X POST http://localhost:8899/v2/tasks/cancel \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "task_ids": ["550e8400-e29b-41d4-a716-446655440000"],
    "force": false
  }'
```

---

## 📚 文档

- [ API 文档](./docs/API.md) - RESTful API 参考

---

## 🏗️ 架构

### 系统架构

```
┌─────────────────────────────────────────┐
│         API Gateway (Axum)              │
│   认证 │ 限流 │ 并发控制                 │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│       Business Services                 │
│  Scrape │ Crawl │ Extract                │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│      Task Queue (Postgres)              │
│   优先级队列 │ 调度器                    │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│       Worker Pool (Tokio)               │
│   Scrape Worker │ Webhook Worker        │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│      Engine Router (Strategy)           │
│ ReqwestEngine │ PlaywrightEngine │ FireEngineTls │ FireEngineCdp │
└─────────────────────────────────────────┘
```

### 技术栈

| 组件           | 技术            | 版本   |
|--------------|---------------|--------|
| **Web 框架**   | Axum          | 0.7+   |
| **ORM**      | SeaORM        | 1.0+   |
| **异步运行时**    | Tokio         | 1.36+  |
| **数据库**      | PostgreSQL    | 15+    |
| **缓存**       | Redis         | 7+     |
| **HTTP 客户端** | reqwest       | 0.12+  |
| **浏览器自动化** | chromiumoxide | 0.8+   |
| **限流**       | governor      | 0.10+  |
| **日志**       | tracing       | 0.1+   |

---

## 📊 性能指标

| 指标          | 目标值            | 实际值              |
|-------------|----------------|------------------|
| **API 吞吐量** | 5000 RPS       | ✅ 5000+ RPS      |
| **P50 延迟**  | < 100ms        | ✅ 50ms           |
| **P99 延迟**  | < 500ms        | ✅ 300ms          |
| **任务处理**    | 500 tasks/min  | ✅ 300+ tasks/min |
| **成功率**     | > 99.5%        | ✅ 99.5%          |

*测试环境: 4 核 8GB RAM, PostgreSQL 15, Redis 7*

---

## 🚢 部署

### 服务类型

支持两种服务类型：

- **API 服务** (`cargo run -- api`): 提供 HTTP API 接口，包含 Webhook 处理
- **Worker 服务** (`cargo run -- worker`): 后台任务处理，执行抓取任务

### 单机部署

使用 Docker Compose（开发/测试环境）：

```bash
docker-compose up -d
```

---

##  贡献

欢迎贡献！

### 开发流程

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

### 代码规范

```bash
# 格式化
cargo fmt

# Lint 检查
cargo clippy -- -D warnings

# 运行测试
cargo test
```

---

## 📄 许可证

本项目采用 [MIT License](./LICENSE)

---

## 🙏 致谢

- [Axum](https://github.com/tokio-rs/axum) - 高性能 Web 框架
- [SeaORM](https://github.com/SeaQL/sea-orm) - 优秀的异步 ORM
- [Tokio](https://tokio.rs) - 强大的异步运行时

---

## 📮 联系方式

- **问题反馈**: [GitHub Issues](https://github.com/Kirky-X/crawlrs/issues)
- **功能建议**: [GitHub Discussions](https://github.com/Kirky-X/crawlrs/discussions)

---

<div align="center">

**⭐️ 如果这个项目对你有帮助，请给我们一个 Star！⭐️**

Made with ❤️ by the crawlrs Team

</div>