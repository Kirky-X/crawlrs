<div align="center">

<img src="docs/image/logo.png" alt="Crawlrs Logo" width="200">

[![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=build)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/release/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/releases) [![License](https://img.shields.io/github/license/Kirky-X/crawlrs)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE) ![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)

**中文** | [English](README_EN.md)

**使用 Rust 构建的企业级网页数据采集平台**

[✨ 功能特性](#-功能特性) • [🚀 快速开始](#-快速开始) • [📚 文档](#-文档) • [💻 示例](#-示例) • [🤝 参与贡献](#-参与贡献)

</div>

---

<div align="center">

### 🎯 一句话下发任务，五引擎接力执行

通过统一 REST API 派发任务，引擎调度、反爬与限流由 crawlrs 自动接管：

<table style="width:100%; border-collapse: collapse">
<tr>
<td align="center" width="25%">📮<br><b>任务接入</b><br><span style="color:#64748B">同步 · 异步 · 批量</span></td>
<td align="center" width="25%">🚂<br><b>自动选路</b><br><span style="color:#64748B">混合调度 · 竞速 · 降级</span></td>
<td align="center" width="25%">🛡️<br><b>反爬升级</b><br><span style="color:#64748B">探测 · 伪装 · 重试 · 代理</span></td>
<td align="center" width="25%">🧠<br><b>AI 增强</b><br><span style="color:#64748B">RAG · 图谱 · DRL</span></td>
</tr>
</table>

</div>

---

## 📋 目录

- [✨ 功能特性](#-功能特性)
- [🚀 快速开始](#-快速开始)
- [🎨 特性标志](#-特性标志)
- [📚 文档](#-文档)
- [💻 示例](#-示例)
- [🚢 部署](#-部署)
- [🏗️ 架构](#️-架构)
- [🧪 测试](#-测试)
- [📊 性能](#-性能)
- [🔒 安全](#-安全)
- [🗺️ 开发路线图](#️-开发路线图)
- [🤝 参与贡献](#-参与贡献)
- [📋 更新日志](#-更新日志)
- [📄 许可证](#-许可证)
- [🙏 致谢](#-致谢)
- [📞 联系与支持](#-联系与支持)
- [⭐ Star 历史](#-star-历史)

---

## ✨ 功能特性

<table style="width:100%; border-collapse: collapse">
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🔍 <b>统一搜索</b><br><span style="color:#64748B">Google、Bing、百度、搜狗多引擎聚合，自动去重、结果统一格式、支持 A/B 测试</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🎯 <b>单页抓取</b><br><span style="color:#64748B">静态 HTML 与 JS 渲染页面统一抓取，支持截图、表单交互、自定义请求头与同步等待</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🕷️ <b>深度爬取</b><br><span style="color:#64748B">URL 过滤链 + 复合评分器 + 优先级队列 + 自适应停止条件，robots.txt 合规</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📊 <b>数据提取</b><br><span style="color:#64748B">CSS 规则、正文提取器（Trafilatura / DomSmoothie）、LLM 与 RAG 增强多种提取模式</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🚂 <b>五引擎智能路由</b><br><span style="color:#64748B">Reqwest / Playwright / FlareSolverr / Wreq（TLS 指纹）/ MLLM（视觉 LLM 导航），SmartHybrid、竞速与顺序降级三种路由策略</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🛡️ <b>反爬对抗</b><br><span style="color:#64748B">三层反爬检测、SPA 空壳探测升级、UA 池一致性伪装、智能重试、代理轮换、请求合并</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🏢 <b>企业平台能力</b><br><span style="color:#64748B">多租户隔离、garrison 认证（RBAC + JWT + 防暴力破解）、limiteron 限流熔断、Webhook 事件通知</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📈 <b>可观测性</b><br><span style="color:#64748B">Prometheus 指标导出（队列深度、引擎成功率、缓存命中等）、inklog 结构化日志、审计日志</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🧠 <b>智能增强</b><br><span style="color:#64748B">RAG 增强提取、知识图谱覆盖感知爬取、DRL 自适应策略（ONNX 推理 + 启发式退化）</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🧩 <b>特性门控裁剪</b><br><span style="color:#64748B">20+ Cargo features 按需组合，<code>agent-lib</code> 提供嵌入式最小库面，全部业务能力可关闭并注入 Noop 实现</span></td>
</tr>
</table>

除上述核心能力外，crawlrs 还提供站点映射、异步任务队列（Worker 模式）、多层缓存与代理支持等能力；引擎级增强模块（反爬对抗、智能增强等）的设计与代码位置详见 [🏗️ 架构文档 · 爬取能力增强模块](docs/ARCHITECTURE.md)。

---

## 🚀 快速开始

### 📦 安装

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

# 默认特性构建（platform 全套业务能力 + db-postgres）
cargo build --release

# 生产推荐：默认 + Playwright 引擎 + 指标 + 内容处理
cargo build --release --features standard

# 全部功能（standard + FlareSolverr + 正文提取 + LLM）
cargo build --release --features full
```

要求 Rust 1.97 及以上（`Cargo.toml` 的 `rust-version`，最新稳定版即可）；默认数据库后端为 PostgreSQL 16+（`db-postgres`）；Docker 20+ 用于集成测试（testcontainers）与容器化部署。

### 💡 最小示例

以下示例改编自 [`config/default.toml`](config/default.toml) 配置模板与 [📖 用户指南](docs/USER_GUIDE.md) 的启动流程：

```bash
# 1. 准备配置（完整模板见 config/default.toml，环境变量见 .env.example）
cat > config/default.toml <<'EOF'
[database]
url = "postgresql://user:password@localhost/crawlrs"

[server]
host = "0.0.0.0"
port = 8899

[search]
default_engine = "baidu"
EOF

# 2. 初始化数据库
cargo run --bin crawlrs -- migrate

# 3. 启动服务（API 模式；worker 模式：cargo run --bin crawlrs worker）
cargo run --bin crawlrs
```

```bash
# 4. 验证安装
curl http://localhost:8899/health
# {"status":"healthy","version":"0.2.0"}
```

调用抓取接口（认证与全部端点详见 [📘 API 参考](docs/API_REFERENCE.md)）：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer <garrison_key_id>.<garrison_secret>" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

### 🧭 核心概念

- **引擎与路由**：`EngineClient` 是抓取操作唯一公开入口，内部 `EngineRouter` 持有 `Vec<Arc<dyn ScraperEngine>>`，按 `SmartHybrid`（默认）/ `RaceMode` / `SequentialFallback` 策略选择引擎。
- **双运行模式**：同一二进制两种形态——`crawlrs` 启动 API 服务，`crawlrs worker` 以 Worker 模式消费任务队列。
- **DDD 四层**：`presentation → application → domain → infrastructure`，DI 由 trait-kit 的 `AppModule` 装配。
- **配置管理**：基于 confers，TOML 文件 + `CRAWLRS__` 前缀环境变量（`__` 分隔嵌套，如 `CRAWLRS__DATABASE__URL`）。
- **特性门控**：业务能力（teams/auth/rate-limit/webhook）关闭时自动注入 Noop 实现，单租户/无认证部署零业务逻辑改动。

---

## 🎨 特性标志

### 📦 功能预设

| 预设 | 构建命令 | 包含内容 | 适用场景 |
|------|----------|----------|----------|
| 默认 | `cargo build --release` | `platform`（teams + auth + rate-limit + webhook + metrics + content）+ `db-postgres` | 开箱即用的完整平台 |
| `standard` | `cargo build --release --features standard` | 默认 + `engine-playwright` | 生产推荐（JS 渲染 + 指标 + 内容处理） |
| `full` | `cargo build --release --features full` | `standard` + `engine-flaresolverr` + `extractors` + `llm` | 全部功能 |
| `no-default` | `cargo build --release --no-default-features` | 纯核心抓取栈 | 单租户/无认证部署、嵌入集成 |
| `agent-lib` | `cargo build --release --no-default-features --features agent-lib` | `content` + `trafilatura` + `dom-smoothie` | agent/嵌入式最小库面 |

### 📋 功能矩阵

下表对应 `Cargo.toml` 的 `[features]` 定义，`default = ["platform", "db-postgres"]`；核心抓取栈（oxcache / dbnexus / confers / sdforge / inklog / trait-kit + scraper / reqwest / robotstxt）始终编译。

| 特性 | 说明 | 默认 |
|------|------|------|
| `teams` | 多租户隔离（隐含 `auth`）；关闭时归属 `DEFAULT_TEAM_ID` 单租户 | ✅ |
| `auth` | garrison v0.9 认证（RBAC + JWT + 防暴力破解）；关闭时注入固定身份 | ✅ |
| `rate-limit` | limiteron 限流熔断；关闭时注入 `NoopRateLimitingService` | ✅ |
| `webhook` | Webhook 投递；关闭时移除 `/v1/webhooks` 路由并注入 Noop | ✅ |
| `metrics` | Prometheus 指标导出（含 sysinfo 内存感知） | ✅（platform） |
| `content` | 反爬检测（aho-corasick）+ HTML→Markdown（htmd） | ✅（platform） |
| `db-postgres` / `db-sqlite` / `db-mysql` | 数据库驱动三选一（dbnexus 编译期互斥强制） | ✅ postgres |
| `engine-playwright` | chromiumoxide 浏览器自动化引擎 | ❌ |
| `engine-flaresolverr` | FlareSolverr 反爬引擎（Full/Cdp/Tls 三模式） | ❌ |
| `engine-tls-fingerprint` | WreqEngine TLS 指纹伪装（BoringSSL JA3/JA4） | ❌ |
| `engine-mllm` | MLLM 视觉 LLM 自主导航引擎（隐含 `engine-playwright` + `llm`） | ❌ |
| `trafilatura` / `dom-smoothie` / `extractors` | 正文提取主路径 / 性能回退 / 全启用 | ❌ |
| `llm` | genai LLM 抽取 | ❌ |
| `test-mocks` | 测试专用 mock 门控（集成测试需显式启用） | ❌ |

> 特性组合的编译矩阵（22 组合）与验证方式见 [🧪 测试场景矩阵](docs/TEST_SCENARIOS.md)；特性门控的实现模式见 [🏗️ 架构文档 · Feature Gate Architecture](docs/ARCHITECTURE.md)。

---

## 📚 文档

| 文档 | 说明 |
|------|------|
| [📖 用户指南](docs/USER_GUIDE.md) | 认证、抓取、爬取、搜索、提取、Webhook、团队等全部功能的完整教程 |
| [📘 API 参考](docs/API_REFERENCE.md) | 全部 REST 端点、请求/响应格式、错误码与 SDK |
| [🏗️ 架构文档](docs/ARCHITECTURE.md) | DDD 分层、引擎路由、增强模块、安全模型与部署拓扑 |
| [⚡ 性能指南](docs/PERFORMANCE.md) | 基准套件、性能观测与调优建议 |
| [🔒 安全文档](docs/SECURITY.md) | 安全设计、供应链门禁、漏洞报告流程与生产加固清单 |
| [❓ FAQ](docs/FAQ.md) | 常见问题解答 |
| [🧪 测试场景矩阵](docs/TEST_SCENARIOS.md) | 测试套件穷举、特性组合矩阵与运行手册 |
| [🤝 贡献指南](docs/CONTRIBUTING.md) | 环境搭建、开发工作流与提交规范 |
| [📋 更新日志](docs/CHANGELOG.md) | 每个版本的变更记录 |
| [📦 Releases](https://github.com/Kirky-X/crawlrs/releases) | 版本发布页面 |

---

## 💻 示例

全部 65 个可运行示例位于 [`examples/`](examples/) 独立 workspace，按功能域分为 search、scrape、crawl、extract、auth、teams、webhooks、cache、config、database、proxy、rate-limiting、sdk、tasks、browser、advanced 等类别（每个示例是 `[[bin]]` 目标，如 `basic_scrape`）。分类清单与说明见 [examples/README.md](examples/README.md) 与 [examples/QUICKSTART.md](examples/QUICKSTART.md)。

```bash
cd examples
cargo run --bin basic_scrape          # 基础抓取
cargo run --bin api_key_auth          # garrison API Key 认证
cargo run --bin async_batch           # 异步批量抓取
cargo build                           # 编译全部示例
```

---

## 🚢 部署

```bash
# 构建镜像（主 Dockerfile 位于 docker/ 目录）
docker build -t crawlrs:latest -f docker/Dockerfile .

# Docker Compose 一键启动（crawlrs + PostgreSQL 16 + FlareSolverr + Chrome + Prometheus）
docker compose -f docker/docker-compose.yml up -d
```

服务默认监听 `8899`（`CRAWLRS__SERVER__PORT` 可调）；单实例与 Kubernetes 多实例拓扑、Worker 池与外部依赖关系详见 [🏗️ 架构文档 · 部署架构](docs/ARCHITECTURE.md)，生产环境安全加固清单见 [🔒 安全文档 · 生产部署加固](docs/SECURITY.md)。

---

## 🏗️ 架构

crawlrs 遵循领域驱动设计（DDD），采用 `presentation → application → domain → infrastructure` 四层架构：Axum 处理 HTTP 与中间件，Use Case 编排业务逻辑，领域层以 `ScraperEngine` 等 trait 定义能力契约，基础设施层经 dbnexus（Sea-ORM）对接 PostgreSQL、经 oxcache 提供多层缓存。抓取数据通路为：请求经 SSRF 前置校验 → `EngineRouter` 按策略选路（反爬检测可动态改派浏览器引擎）→ Worker 池执行 → 结果落库并触发 Webhook。

分层职责、引擎路由细节、全部增强模块、队列/缓存/限流设计与部署拓扑详见 [🏗️ 架构文档](docs/ARCHITECTURE.md)。

---

## 🧪 测试

### 🎯 测试策略

测试体系覆盖七层：`src/` 内联单元测试、`tests/unit/` 按模块组织的单元测试、`tests/main.rs` 表驱动机（mock 注入）、SDK API 测试（`sdk_api_test`）、真实 PostgreSQL 集成测试（`integration_tests`，含 garrison 认证端到端）、Python API/性能测试与 E2E 质量保障套件（`tests/e2e/e2e-suite.sh`：22 组特性编译矩阵 → 静态检查 → 单元/集成测试 → 基准 → 报告）。各层场景穷举与文件映射见 [🧪 测试场景矩阵](docs/TEST_SCENARIOS.md)。

### ▶️ 运行命令（与 CI 一致）

```bash
# 单元测试（CI test job：standard / full 各跑一轮；需 PostgreSQL 16，本地由 testcontainers 自动拉起）
cargo test --features "standard" --lib
cargo test --features "full" --lib

# 集成 + 全部测试目标（CI integration-test job）
cargo test --features "full,test-mocks" --tests --no-fail-fast

# Lint 与格式门禁
cargo fmt --all -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings

# 覆盖率门禁：行覆盖率不低于 80%（CI coverage job，结果上传 Codecov）
cargo llvm-cov --features "full" --fail-under-lines 80

# 供应链检查（advisories / licenses / bans / sources）
cargo deny check

# 基准测试（Criterion，9 组）
cargo bench

# E2E 质量保障套件（特性矩阵 + 静态 + 测试 + 集成 + 基准 + 报告）
./tests/e2e/e2e-suite.sh

# Python API / 性能测试（本地）
./scripts/run-tests.sh local

# 提交前完整检查（fmt → clippy → check → build → 私钥扫描）
scripts/pre-commit-check.sh all
```

### 📊 测试规模

截至 0.2.0：`src/` 内联测试约 5700+ 个（372 个源文件），`tests/` 目录约 950 个（含 71 个单元测试文件与真实 PostgreSQL 集成套件），Criterion 基准 9 组，Python 测试 5 个套件；覆盖率门禁为行覆盖率不低于 80%，由 CI 执行。逐项统计见 [🧪 测试场景矩阵 · 测试规模](docs/TEST_SCENARIOS.md)。

---

## 📊 性能

性能工程内建于架构：AIMD 自适应并发、内存感知调度、TabPool 复用、请求合并（singleflight）、WaitFor 条件等待与 Hedge 副本控制器；可复现的基准为 `benches/benchmark.rs` 的 9 组 Criterion 基准（任务创建/状态迁移、JSON 序列化、URL 解析与校验、SSRF 检测、RegexCache、引擎路由等），E2E 套件 Stage 5 以 `e2e-baseline` 做回归对比。旧版宣传的 Node.js 对比数字未附测量口径，待复测后更新。基准说明与调优建议见 [⚡ 性能指南](docs/PERFORMANCE.md)。

---

## 🔒 安全

### 🛡️ 安全设计

安全设计覆盖请求全链路：SSRF 在 handler 前置与引擎路由双重校验；认证由 garrison 接管（HS256 JWT、RBAC、IP 级防暴力破解、审计落库）；Webhook 经 Standard Webhooks 签名验证（subtle 恒时比较）；JWT secret 以 zeroize 防内存残留；安全响应头与可信代理中间件默认加固。逐项机制详见 [🔒 安全文档](docs/SECURITY.md) 与 [🏗️ 架构文档 · 安全模型](docs/ARCHITECTURE.md)。

### ⛓️ 供应链与门禁

CI 固定执行 `cargo deny check`（advisories / licenses / bans / sources）、CodeQL 静态分析、clippy `-D warnings` 与 RSA 代码路径前提校验；本地 pre-commit 脚本含私钥扫描。完整门禁清单见 [🔒 安全文档 · 供应链与门禁](docs/SECURITY.md)。

### 🚨 报告安全漏洞

请勿通过公开 issue 报告安全漏洞，请发送邮件至 [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)。完整漏洞处理流程见 [SECURITY.md](docs/SECURITY.md)。

---

## 🗺️ 开发路线图

<table style="width:100%; border-collapse: collapse">
<tr><th style="text-align:center">状态</th><th style="text-align:left">方向</th><th style="text-align:left">条目</th></tr>
<tr><td align="center">✅</td><td>核心平台（0.1.0）</td><td>搜索/抓取/爬取/提取/映射五大能力 API、DDD 四层、多租户与限流、Webhook 通知</td></tr>
<tr><td align="center">✅</td><td>平台加固与智能引擎（0.2.0）</td><td>garrison 认证、反爬检测、TLS 指纹与 MLLM 引擎、RAG/KG/DRL 智能增强、Prometheus 可观测性、E2E 质量套件</td></tr>
<tr><td align="center">🚧</td><td>数据库多驱动</td><td><code>db-sqlite</code> / <code>db-mysql</code> 已实现编译级覆盖，运行时 schema 供给待补（当前 <code>migrations/*.sql</code> 为 PG 专用 DDL）</td></tr>
<tr><td align="center">📋</td><td>架构演进</td><td>事件驱动内部总线、WebSocket 实时任务状态、Redis 共享缓存层（多实例部署）</td></tr>
<tr><td align="center">📋</td><td>性能看护</td><td>Criterion 基线（e2e-baseline）回归对比常态化、性能口径复测</td></tr>
</table>

---

## 🤝 参与贡献

详细的贡献流程与代码规范请参阅 [🤝 贡献指南](docs/CONTRIBUTING.md)。

### 🛠️ 开发环境

工具链要求见 `Cargo.toml` 的 `rust-version`（1.97），构建 sdforge 需要 protoc；提交前运行 `scripts/pre-commit-check.sh all`（fmt → clippy → check → build → 私钥扫描），提交信息遵循 Conventional Commits（`type(scope): subject`）。开发工作流（TDD）、分支与测试要求见 [🤝 贡献指南 · 开发工作流](docs/CONTRIBUTING.md)。

### 💖 贡献方式

<table style="width:100%; border-collapse: collapse">
<tr>
<td width="33%" align="center" style="padding: 16px">

### 🐛 报告 Bug

发现问题？<br>
<a href="https://github.com/Kirky-X/crawlrs/issues/new">创建 Issue</a>

</td>
<td width="33%" align="center" style="padding: 16px">

### 💡 功能建议

有好想法？<br>
<a href="https://github.com/Kirky-X/crawlrs/issues/new">发起讨论</a>

</td>
<td width="33%" align="center" style="padding: 16px">

### 🔧 提交 PR

想贡献代码？<br>
<a href="https://github.com/Kirky-X/crawlrs/pulls">Fork 并提交 PR</a>

</td>
</tr>
</table>

---

## 📋 更新日志

完整版本历史见 [📋 更新日志](docs/CHANGELOG.md)（遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) 格式，语义化版本）。

| 版本 | 日期 | 要点 |
|------|------|------|
| Unreleased | - | TLS 指纹引擎（WreqEngine）、MLLM 视觉导航引擎、RAG 增强提取、知识图谱覆盖感知、DRL 自适应策略、5 个新 Prometheus 指标 |
| 0.2.0 | 2026-07-29 | garrison RBAC 认证集成、bootstrap admin key、`DELETE /v1/crawl/{id}`、暴力破解防护加固 |
| 0.1.0 | 2026-07-22 | 首个公开版本：五大能力 API、DDD 四层架构、多租户与限流、统一搜索 |

---

## 📄 许可证

本项目采用 [Apache License 2.0](LICENSE) 许可证。Copyright © 2025 Kirky.X。

---

## 🙏 致谢

### 🌟 核心依赖

crawlrs 站在以下优秀开源项目的肩膀上：

| 依赖 | 用途 |
|------|------|
| [tokio](https://crates.io/crates/tokio) | 异步运行时 |
| [axum](https://crates.io/crates/axum) | Web 框架 |
| [reqwest](https://crates.io/crates/reqwest) | HTTP 客户端 |
| [chromiumoxide](https://crates.io/crates/chromiumoxide) | Chrome CDP 浏览器自动化 |
| [wreq](https://crates.io/crates/wreq) | BoringSSL TLS 指纹伪装 |
| [scraper](https://crates.io/crates/scraper) | HTML 解析 |
| [genai](https://crates.io/crates/genai) | 多模型 LLM 接入 |
| [htmd](https://crates.io/crates/htmd) | HTML→Markdown 转换 |
| [rs-trafilatura](https://crates.io/crates/rs-trafilatura) / [dom_smoothie](https://crates.io/crates/dom_smoothie) | 正文提取 |
| [criterion](https://crates.io/crates/criterion) | 基准测试 |
| [testcontainers](https://crates.io/crates/testcontainers) | 集成测试基础设施 |

同作者自研基座组件：dbnexus（数据库抽象）、confers（配置管理）、garrison（认证框架）、limiteron（限流熔断）、oxcache（缓存）、inklog（结构化日志）、sdforge（SDK 生成）、trait-kit（依赖注入）。

### 💝 特别感谢

感谢 Rust 社区与所有[贡献者](https://github.com/Kirky-X/crawlrs/graphs/contributors)。

---

## 📞 联系与支持

<table style="width:100%; max-width: 600px">
<tr>
<td align="center" width="33%">
<a href="https://github.com/Kirky-X/crawlrs/issues"><b style="color:#991B1B">Issues</b></a><br>
<span style="color:#64748B">报告问题和 Bug</span>
</td>
<td align="center" width="33%">
<a href="mailto:Kirky-X@outlook.com"><b style="color:#1E40AF">邮箱</b></a><br>
<span style="color:#64748B">Kirky-X@outlook.com</span>
</td>
<td align="center" width="33%">
<a href="https://github.com/Kirky-X/crawlrs"><b style="color:#1E293B">GitHub</b></a><br>
<span style="color:#64748B">查看源代码</span>
</td>
</tr>
</table>

---

## ⭐ Star 历史

[![Star History Chart](https://api.star-history.com/svg?repos=Kirky-X/crawlrs&type=Date)](https://star-history.com/#Kirky-X/crawlrs&Date)

如果这个项目对您有帮助，请考虑给它一个 ⭐️！

**由 Kirky.X 构建**

---

<sub>© 2025 Kirky.X. 保留所有权利。</sub>
