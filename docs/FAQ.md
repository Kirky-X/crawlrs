# ❓ Crawlrs FAQ

本页汇总 crawlrs 的常见问题与解答，按主题分组。答案均基于仓库实际代码与配置。没有找到答案？欢迎前往 [GitHub Issues](https://github.com/Kirky-X/crawlrs/issues) 提问。

## 📋 目录

- [🧭 通用问题](#-通用问题)
- [📦 安装与配置](#-安装与配置)
- [🔐 认证与多租户](#-认证与多租户)
- [🚂 引擎与抓取](#-引擎与抓取)
- [🧪 测试与部署](#-测试与部署)

---

## 🧭 通用问题

### ❓ crawlrs 是什么？

crawlrs 是一个使用 Rust 构建的自托管企业级网页数据采集平台，提供五大能力：统一搜索（Google/Bing/百度/搜狗）、单页抓取、多页爬取、数据提取与站点映射，并内置多租户、认证、限流、Webhook、指标等平台能力。架构与能力全景见 [🏗️ 架构文档](ARCHITECTURE.md)。

### ❓ 为什么选择 Rust 实现？

零成本抽象与 fearless concurrency 带来高吞吐与低尾延迟；强类型与 trait 契约（`ScraperEngine` 等）让引擎扩展安全可控；单个静态二进制便于部署。

### ❓ 可以用于生产环境吗？

可以。项目具备企业级配套：garrison 认证（RBAC + JWT + 防暴力破解）、limiteron 限流熔断、审计日志、Prometheus 指标、覆盖率门禁 ≥ 80% 的 CI、22 组合特性矩阵与 E2E 质量套件。上线前请过一遍 [🔒 安全文档 · 生产部署加固清单](SECURITY.md)。

### ❓ 项目采用什么许可证？

[Apache License 2.0](../LICENSE)。Copyright © 2025-2026 Kirky.X🌠。

### ❓ 在哪里可以获得帮助？

- 使用问题：[📖 用户指南](USER_GUIDE.md) 与本 FAQ；
- 接口细节：[📘 API 参考](API_REFERENCE.md)；
- Bug 与建议：[GitHub Issues](https://github.com/Kirky-X/crawlrs/issues)；
- 安全漏洞：邮件 [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)（勿公开披露）。

---

## 📦 安装与配置

### ❓ 系统要求是什么？

Rust 1.97+（`Cargo.toml` `rust-version`，构建需 protoc）、PostgreSQL 16+（默认后端）、Docker 20+（集成测试与容器部署）。详见 README「快速开始」。

### ❓ 如何选择特性组合？

按场景选择预设：默认（完整平台）、`standard`（生产推荐）、`full`（全功能）、`--no-default-features`（单租户/嵌入）、`agent-lib`（agent 最小库面）。逐项矩阵见 README「🎨 特性标志」；组合的编译验证方式见 [🧪 测试场景矩阵](TEST_SCENARIOS.md)。

### ❓ 支持哪些数据库？

`db-postgres`（默认）/ `db-sqlite` / `db-mysql` 三组特性按构建三选一，互斥由 dbnexus 编译期强制。注意：当前 `migrations/*.sql` 为 PostgreSQL 专用 DDL，sqlite/mysql 为编译级覆盖，运行时建表需自行提供 schema（见路线图）。

### ❓ 环境变量如何命名？

统一 `CRAWLRS__` 前缀，`__` 分隔嵌套层级（confers 约定），如 `CRAWLRS__DATABASE__URL`、`CRAWLRS__SERVER__PORT`。完整参考见 [.env.example](../.env.example)；配置段速查见用户指南。

### ❓ 服务监听哪个端口？如何确认启动成功？

默认 `8899`（`CRAWLRS__SERVER__PORT` 可调）。`curl http://localhost:8899/health` 返回 `{"status":"healthy","version":"0.2.0"}` 即成功。

---

## 🔐 认证与多租户

### ❓ API Key 是什么格式？

0.2.0 起认证由 garrison 接管，Bearer token 格式为 `garrison_key_id.garrison_secret`；明文 key 仅在签发响应中返回一次，请立即写入 secrets manager。

### ❓ 0.1.x 的旧 API Key 还能用吗？

不能。0.2.0 认证迁移后旧的 `api_keys.key_hash`（SHA-256）体系全部作废，`CRAWLRS__AUTH__KEYS` / `[auth] keys` 配置项已弃用，需向 garrison 重新领取。

### ❓ 如何签发新的 API Key？

三种方式：`POST /v1/admin/api-keys`（需 `crawlrs:admin` 权限）；`reissue_api_keys` CLI 工具；开发/测试环境用 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 环境变量自动引导 admin key（生产禁用）。

### ❓ 必须配置哪些认证相关项？

`CRAWLRS__AUTH__JWT_SECRET`（HS256，≥32 字节）——弱密钥会拒绝启动。

### ❓ 401 和 429 有什么区别？

401 表示 key 无效/过期/权限不足；429 表示触发限流或 garrison 防暴力破解锁定（5 次失败 / 60 秒窗口 / 300 秒锁定）。

### ❓ 可以完全关闭认证吗？

可以。`cargo build --release --no-default-features` 构建单租户/无认证版本：`default_identity_middleware` 注入固定身份，全部请求归属默认团队；rate-limit / webhook 关闭时同样注入 Noop 实现，业务逻辑无感知。

---

## 🚂 引擎与抓取

### ❓ 五个引擎分别什么时候用？

| 引擎 | 适用场景 |
|------|----------|
| Reqwest | 静态 HTML、API 响应（默认最快路径） |
| Playwright（chromiumoxide） | JS 渲染的 SPA、页面交互 |
| FlareSolverr | 反爬保护站点（Full/Cdp/Tls 三模式，需外部服务） |
| WreqEngine | TLS 指纹伪装（BoringSSL JA3/JA4，`engine-tls-fingerprint`） |
| MllmEngine | 视觉 LLM 自主导航（截图→决策→执行，`engine-mllm`） |

`EngineRouter` 以 `SmartHybrid` 策略自动选路；反爬检测命中后可动态改派浏览器引擎。细节见 [🏗️ 架构文档 · Crawling Engines](ARCHITECTURE.md)。

### ❓ 构建 TLS 指纹引擎有什么额外要求？

`engine-tls-fingerprint` 引入 wreq（BoringSSL 后端），构建机器需要 cmake 与 C 编译器。许可证上刻意排除了 GPL 的 wreq-util，指纹模拟由项目自有枚举实现。

### ❓ 抓取结果支持哪些输出格式？

`ScrapeResponse` 支持原始 HTML、纯文本、结构化提取结果，启用 `content` 后附 `markdown` 字段（htmd 转换）；正文提取可选用 Trafilatura（主路径）/ DomSmoothie（性能回退）/ LLM / RAG 策略。

### ❓ 遵守 robots.txt 吗？

遵守。`robotstxt` 为核心栈非可选依赖，爬取示例中有 `robots_compliance` 专项演示；深度爬取支持 URL 过滤链与自适应停止。

---

## 🧪 测试与部署

### ❓ 跑测试为什么需要 Docker？

集成测试用 testcontainers 自动拉起 PostgreSQL 16 容器（无 Docker 时相关用例自动 skip）；也可用 `TEST_DATABASE_URL` 指向外部实例（`docker/docker-compose.test.yml` 提供 `test-db`）。完整命令见 [🧪 测试场景矩阵 · 运行手册](TEST_SCENARIOS.md)。

### ❓ 测试规模有多大？

截至 0.2.0：`src/` 内联测试约 5700+，`tests/` 约 950 个，9 组 Criterion 基准，5 个 Python 套件，CI 行覆盖率门禁 80%。详见 [🧪 测试场景矩阵 · 统计汇总](TEST_SCENARIOS.md)。

### ❓ 如何部署？

`docker build -t crawlrs:latest -f docker/Dockerfile .` 构建镜像；`docker compose -f docker/docker-compose.yml up -d` 一键拉起 crawlrs + PostgreSQL + FlareSolverr + Chrome + Prometheus。Kubernetes 多实例拓扑见 [🏗️ 架构文档 · 部署架构](ARCHITECTURE.md)。

### ❓ `crawlrs worker` 是什么？

同一二进制的第二种运行形态：API 模式（默认）接收请求并入队，Worker 模式消费任务队列执行抓取。二者可分开部署、独立扩缩容。
