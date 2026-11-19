[English](README.md) | 中文

<div align="center">

<img src="docs/image/crawlrs.png" alt="Crawlrs Logo" width="240">

[![Rust 2021](https://img.shields.io/badge/Rust-2021-edded?logo=rust&style=for-the-badge)](https://www.rust-lang.org/) [![Rustc 1.97.1+](https://img.shields.io/badge/Rustc-1.97.1+-orange.svg?style=for-the-badge)](https://www.rust-lang.org/) [![release](https://img.shields.io/github/v/release/Kirky-X/crawlrs?style=for-the-badge)](https://github.com/Kirky-X/crawlrs/releases) [![CI](https://img.shields.io/github/actions/workflow/status/Kirky-X/crawlrs/ci.yml?branch=main&label=CI&style=for-the-badge)](https://github.com/Kirky-X/crawlrs/actions/workflows/ci.yml) [![license](https://img.shields.io/github/license/Kirky-X/crawlrs?style=for-the-badge)](https://github.com/Kirky-X/crawlrs/blob/main/LICENSE)

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
<td width="50%" style="vertical-align:top; padding: 12px">🔍 <b>统一搜索</b><br><span style="color:#64748B">Bing、百度、搜狗多引擎聚合，RRF 融合排序 + SimHash 跨引擎去重，Exa / Tavily 按凭据自动注册，SearXNG 公共实例池按配置启用（opt-in），结果统一格式</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🎯 <b>单页抓取</b><br><span style="color:#64748B">静态 HTML 与 JS 渲染页面统一抓取，支持截图、表单交互、自定义请求头与同步等待；Playwright 引擎可捕获页面 XHR/Fetch 后台响应</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🕷️ <b>深度爬取</b><br><span style="color:#64748B">URL 过滤链 + 复合评分器 + 优先级队列 + 自适应停止条件，robots.txt 合规（Crawl-delay 双时钟）；sitemap 流式请求源、软 404 指纹剔除、断点续爬 seen 预热</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📊 <b>数据提取</b><br><span style="color:#64748B">CSS 规则、正文提取器（Trafilatura / DomSmoothie）、LLM 与 RAG 增强多种提取模式；降级阶梯质量门、LLM 长文分块并行、提取结果字符级溯源、间接提示注入净化（零宽字符/隐藏节点/可疑指令，<code>[llm.sanitization]</code>）</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🚂 <b>五引擎智能路由</b><br><span style="color:#64748B">Reqwest / Playwright / FlareSolverr / Wreq（TLS 指纹）/ MLLM（视觉 LLM 导航 + 混合 AX 树快照）/ Moli（可选，内存水位调度下低配机的 JS 渲染回退位），SmartHybrid、竞速与顺序降级三种路由策略</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🛡️ <b>反爬对抗</b><br><span style="color:#64748B">三层反爬检测、SPA 空壳探测升级、UA 池一致性伪装、智能重试、代理轮换、请求合并；Session 身份三件套记分退休、AutoThrottle 自适应限速、429 指数退避</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🏢 <b>企业平台能力</b><br><span style="color:#64748B">多租户隔离（专用代理池、浏览器与 DNS 配额）、garrison 认证（RBAC + JWT + 防暴力破解）、limiteron 限流熔断、Webhook 事件通知</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">📈 <b>可观测性</b><br><span style="color:#64748B">Prometheus 指标导出（队列深度、引擎成功率、缓存命中等）、inklog 结构化日志、审计日志、失败现场快照（错误签名去重存档）</span></td>
</tr>
<tr>
<td width="50%" style="vertical-align:top; padding: 12px">🧠 <b>智能增强</b><br><span style="color:#64748B">RAG 增强提取、知识图谱覆盖感知爬取、DRL 自适应策略（ONNX 推理 + 启发式退化）</span></td>
<td width="50%" style="vertical-align:top; padding: 12px">🧩 <b>特性门控裁剪</b><br><span style="color:#64748B">20+ Cargo features 按需组合，<code>agent-lib</code> 提供嵌入式最小库面，全部业务能力可关闭并注入 Noop 实现</span></td>
</tr>
</table>

除上述核心能力外，crawlrs 还提供站点映射、异步任务队列（Worker 模式）、多层缓存与代理支持等能力；引擎级增强模块（反爬对抗、智能增强等）的设计与代码位置详见 [🏗️ 架构文档](docs/cn/ARCHITECTURE.md)。

---

## 🚀 快速开始

### 📦 安装

<details>
<summary>📄 代码 · bash</summary>

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

# 默认特性构建（platform 全套业务能力 + db-postgres）
cargo build --release
```

</details>

要求 Rust 1.97.1 及以上（`Cargo.toml` 的 `rust-version`，最新稳定版即可）；默认数据库后端为 PostgreSQL 16+（`db-postgres`）；Docker 20+ 用于集成测试（testcontainers）与容器化部署。生产推荐 `--features standard`、全功能组合 `--features full`，完整构建预设与特性语义见 [🎨 特性标志 · 用户指南](docs/cn/USER_GUIDE.md#-特性标志)。

### 💡 最小示例

以下示例改编自 [`config/default.toml`](config/default.toml) 配置模板与 [📖 用户指南](docs/cn/USER_GUIDE.md) 的启动流程：

<details>
<summary>📄 代码 · bash</summary>

```bash
# 1. 准备配置（完整模板见 config/default.toml，环境变量见 .env.example）
cat > config/default.toml <<'EOF'
[database]
url = "postgresql://user:password@localhost/crawlrs"

[server]
host = "0.0.0.0"
port = 8899
EOF

# 2. 初始化数据库（服务启动不自动迁移；migrate 按数据库特性选择 migrations/<dialect>/ 并有序应用）
cargo run --bin crawlrs migrate
# 迁移根目录缺省 ./migrations，可用 CRAWLRS_MIGRATIONS_DIR 覆盖；
# 也可手工应用 PostgreSQL 套：for f in migrations/postgres/*.sql; do psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$f"; done

# 3. 启动服务（API 模式；worker 模式：cargo run --bin crawlrs worker）
cargo run --bin crawlrs
```

</details>

<details>
<summary>📄 代码 · bash</summary>

```bash
# 4. 验证安装
curl http://localhost:8899/health
# {"status":"healthy","version":"0.2.0"}
```

</details>

调用抓取接口（认证与全部端点详见 [📘 API 参考](docs/cn/API_REFERENCE.md)）：

<details>
<summary>📄 代码 · bash</summary>

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer <garrison_key_id>.<garrison_secret>" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

</details>

### 🧭 核心概念

引擎与路由、四种运行形态（api / worker / bootstrap / migrate）、DDD 四层、配置管理、SearXNG 元搜索（opt-in）与特性门控等核心概念详见 [📖 用户指南](docs/cn/USER_GUIDE.md) 与 [🏗️ 架构文档](docs/cn/ARCHITECTURE.md)。

---

## 📚 文档

| 文档 | 说明 |
|------|------|
| [📖 用户指南](docs/cn/USER_GUIDE.md) | 认证、抓取、爬取、搜索、提取、Webhook、团队等全部功能的完整教程 |
| [📘 API 参考](docs/cn/API_REFERENCE.md) | 全部 REST 端点、请求/响应格式、错误码与 SDK |
| [🏗️ 架构文档](docs/cn/ARCHITECTURE.md) | DDD 分层、引擎路由、增强模块、安全模型与部署拓扑 |
| [⚡ 性能指南](docs/cn/PERFORMANCE.md) | 基准套件、性能观测与调优建议 |
| [🔒 安全文档](docs/cn/SECURITY.md) | 安全设计、供应链门禁、漏洞报告流程与生产加固清单 |
| [❓ FAQ](docs/cn/FAQ.md) | 常见问题解答 |
| [🧪 测试场景矩阵](docs/cn/TEST_SCENARIOS.md) | 测试套件穷举、特性组合矩阵与运行手册 |
| [🤝 贡献指南](docs/cn/CONTRIBUTING.md) | 环境搭建、开发工作流与提交规范 |
| [📋 更新日志](docs/cn/CHANGELOG.md) | 每个版本的变更记录 |
| [📦 Releases](https://github.com/Kirky-X/crawlrs/releases) | 版本发布页面 |

---

## 💻 示例

全部 66 个可运行示例位于 [`examples/`](examples/)（根 workspace 成员 crate，主 `Cargo.toml` 的 `members = ["examples"]`），按功能域分为 search、scrape、crawl、extract、auth、teams、webhooks、cache、config、database、proxy、rate-limiting、sdk、tasks、browser、advanced 等类别（每个示例是 `[[bin]]` 目标，如 `basic_scrape`）。分类清单与说明见 [examples/README.md](examples/README.md) 与 [examples/QUICKSTART.md](examples/QUICKSTART.md)。

<details>
<summary>📄 代码 · bash</summary>

```bash
cd examples
cargo run --bin basic_scrape          # 基础抓取
cargo build                           # 编译全部示例
```

</details>

---

## 🚢 部署

<details>
<summary>📄 代码 · bash</summary>

```bash
# Docker Compose 一键启动（crawlrs + PostgreSQL 16 + FlareSolverr + Chrome + Prometheus）
docker compose -f docker/docker-compose.yml up -d
```

</details>

服务默认监听 `8899`（`CRAWLRS__SERVER__PORT` 可调）；镜像构建命令与容器化部署详见 [🏗️ 架构文档 · 容器化部署命令](docs/cn/ARCHITECTURE.md#-容器化部署命令)，单实例与 Kubernetes 多实例拓扑、Worker 池与外部依赖关系详见 [🏗️ 架构文档 · 部署架构](docs/cn/ARCHITECTURE.md)，生产环境安全加固清单见 [🔒 安全文档 · 生产部署加固](docs/cn/SECURITY.md)。

---

## 🏗️ 架构

crawlrs 遵循领域驱动设计（DDD），采用 `presentation → application → domain → infrastructure` 四层架构。分层职责、引擎路由细节、全部增强模块、队列/缓存/限流设计与部署拓扑详见 [🏗️ 架构文档](docs/cn/ARCHITECTURE.md)；自研库矩阵的消费面与缺口登记见 [🏗️ 架构文档 · 自研库反向需求](docs/cn/ARCHITECTURE.md)，作为库依赖的嵌入方式见 [🔌 作为库依赖](docs/cn/ARCHITECTURE.md#-作为库依赖)。

---

## 🧪 测试

### 🎯 测试策略

测试体系覆盖七层：`src/` 内联单元测试、`tests/unit/` 按模块组织的单元测试、`tests/main.rs` 表驱动机（mock 注入）、SDK API 测试（`sdk_api_test`）、真实 PostgreSQL 集成测试（`integration_tests`，含 garrison 认证端到端）、Python API/性能测试与 E2E 质量保障套件（`tests/e2e/e2e-suite.sh`：22 组特性编译矩阵 → 静态检查 → 单元/集成测试 → 基准 → 报告）。各层场景穷举、与 CI 一致的运行命令及逐项统计见 [🧪 测试场景矩阵](docs/cn/TEST_SCENARIOS.md)；覆盖率门禁为行覆盖率不低于 90%，由 CI 执行（实测 90.14%）。

---

## 📊 性能

性能工程内建于架构：AIMD 自适应并发、内存感知调度、TabPool 复用、请求合并（singleflight）、WaitFor 条件等待与 Hedge 副本控制器；可复现的基准为 `benches/benchmark.rs` 的 9 组 Criterion 基准。基准说明与调优建议见 [⚡ 性能指南](docs/cn/PERFORMANCE.md)。

---

## 🔒 安全

### 🛡️ 安全设计

安全设计覆盖请求全链路：SSRF 在 handler 前置与引擎路由双重校验；认证由 garrison 接管（HS256 JWT、RBAC、IP 级防暴力破解、审计落库）；Webhook 经 Standard Webhooks 签名验证（subtle 恒时比较）；JWT secret 以 zeroize 防内存残留；安全响应头与可信代理中间件默认加固。反爬升级跳次产生结构化 `antibot_escalation` 审计事件；运行 `crawlrs doctor --security` 可对 feature 开关、SSRF 防护与运行时加固做启动自检。逐项机制详见 [🔒 安全文档](docs/cn/SECURITY.md) 与 [🏗️ 架构文档 · 安全模型](docs/cn/ARCHITECTURE.md)。

### ⛓️ 供应链与门禁

CI 固定执行 `cargo deny check`（advisories / licenses / bans / sources）、CodeQL 静态分析、clippy `-D warnings` 与 RSA 代码路径前提校验；本地 pre-commit 脚本含私钥扫描。完整门禁清单见 [🔒 安全文档 · 供应链与门禁](docs/cn/SECURITY.md)。

### 🚨 报告安全漏洞

请勿通过公开 issue 报告安全漏洞。请优先使用 GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/policy) 私密披露通道提交报告（不可用时再发送邮件至 [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)，受理人 Kirky.X）。完整漏洞处理流程见 [SECURITY.md](docs/cn/SECURITY.md)。

---

## 🗺️ 开发路线图

版本方向与在途工作见 [📋 更新日志 · 开发路线图](docs/cn/CHANGELOG.md#️-开发路线图)。

---

## 🤝 参与贡献

详细的贡献流程与代码规范请参阅 [🤝 贡献指南](docs/cn/CONTRIBUTING.md)。

---

## 📋 更新日志

完整版本历史见 [📋 更新日志](docs/cn/CHANGELOG.md)（遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) 格式，语义化版本）。

---

## 📄 许可证

本项目采用 [Apache License 2.0](LICENSE) 许可证。Copyright © 2025-2026 Kirky.X🌠。

---

## 🙏 致谢

crawlrs 站在众多优秀开源项目的肩膀上（核心依赖与用途见 `Cargo.toml` 与 [🏗️ 架构文档](docs/cn/ARCHITECTURE.md)）；同作者自研基座组件：dbnexus（数据库抽象）、confers（配置管理）、garrison（认证框架）、limiteron（限流熔断）、oxcache（缓存）、inklog（结构化日志）、sdforge（SDK 生成）、trait-kit（依赖注入）。感谢 Rust 社区与所有[贡献者](https://github.com/Kirky-X/crawlrs/graphs/contributors)。

---

## 📞 联系与支持

Issues（[报告问题和 Bug](https://github.com/Kirky-X/crawlrs/issues)）· 邮箱（[Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)）· GitHub（[查看源代码](https://github.com/Kirky-X/crawlrs)）

---

## ⭐ Star 历史

[![Star History Chart](https://api.star-history.com/svg?repos=Kirky-X/crawlrs&type=Date)](https://star-history.com/#Kirky-X/crawlrs&Date)

如果这个项目对您有帮助，请考虑给它一个 ⭐️！

**由 Kirky.X 构建**

---

<sub>© 2025 Kirky.X. 保留所有权利。</sub>
