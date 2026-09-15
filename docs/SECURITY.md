# 🔒 Crawlrs 安全文档

crawlrs 将安全作为平台的核心设计目标。本文档介绍安全策略、漏洞报告流程、内置安全机制、供应链门禁与生产部署加固清单。全部内容基于仓库实际代码与配置（`deny.toml`、`.github/workflows/`、`src/presentation/`、garrison 集成）。

## 📋 目录

- [📌 支持版本](#-支持版本)
- [🚨 漏洞报告流程](#-漏洞报告流程)
- [🛡️ 安全设计概览](#️-安全设计概览)
- [⛓️ 供应链与门禁](#️-供应链与门禁)
- [🏭 生产部署加固清单](#-生产部署加固清单)
- [🧪 安全测试覆盖](#-安全测试覆盖)

---

## 📌 支持版本

| 版本 | 状态 | 说明 |
|------|------|------|
| 0.2.x | ✅ 支持中 | 当前发布线（含 Unreleased 变更） |
| 0.1.0 | ❌ 不再维护 | 0.2.0 认证迁移后旧 API Key 体系已作废，请尽快升级 |

### 最低支持 Rust 版本（MSRV）

`Cargo.toml` 声明 `rust-version = "1.97"`（workspace 同步锁定）。安全修复仅在受支持版本线上发布。

### 依赖安全基线

- `cargo deny check` 在 CI 每次运行：advisories（`yanked = "deny"`）、license 白名单、bans（`wildcards = "deny"`）、来源校验（仅 crates.io）。
- `jsonwebtoken` v11 仅启用 `rust_crypto` 最小后端（不开 rsa/p256/p384/ed25519-dalek 等非必要特性）。
- TLS 指纹引擎的 `wreq-util`（GPL-3.0）被刻意排除：`TlsEmulation` 由本项目自有枚举映射到 Apache-2.0 的 wreq `EmulationProvider` API。

---

## 🚨 漏洞报告流程

### 如何报告

**请勿通过公开 GitHub Issue 报告安全漏洞。** 请发送邮件至 [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)，或通过 GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/advisories/new) 私密披露通道提交。

### 报告内容

请尽量包含：受影响版本与特性组合（feature flags）、复现步骤或 PoC、影响评估（机密性/完整性/可用性）、已知的缓解措施。

### 我们的承诺

- 确认收到报告后尽快给出初步评估（具体时限：待补充）。
- 修复期间与报告者保持沟通；披露时间线与报告者协商确定。
- 修复随下一个受支持版本发布，并在 `docs/CHANGELOG.md` 的 Security 小节记录（如 0.2.0 的暴力破解防护加固）。

---

## 🛡️ 安全设计概览

### 认证与授权（garrison 接管）

0.2.0 起认证引擎由 **garrison v0.9.0-rc.1** 提供（`auth` 特性，隐含 `dep:garrison`）：

- Bearer token 格式 `garrison_key_id.garrison_secret`，明文 key 仅在签发响应中返回一次。
- JWT 以 `CRAWLRS__AUTH__JWT_SECRET`（HS256，≥32 字节，**弱密钥拒绝启动**）签发；secret 经 zeroize 包装，Drop 时清零内存（CWE-316 防护）。
- RBAC：预置 `crawlrs:read/write/admin` 权限与 `admin/user/read_only` 角色，经 `auth_bridge::map_perms_to_scope` 映射为 `ApiKeyScope`，多租户隔离（`tenant-isolation`）。
- 防暴力破解（`firewall-bruteforce`，CWE-307）：5 次失败 / 60 秒窗口 / 300 秒锁定；IP 上下文由 `garrison_ip_context_middleware` 注入，401/429 由 garrison 直接触发（429 遵循 RFC 6585）。
- 认证事件落库 crawlrs `audit_logs` 表 + garrison 自管 schema（`audit-log` + `audit-inklog`）。
- 认证权限/角色查询经三层缓存（L1 oxcache 30s + L2 DAO 300s + L3 回调），避免认证热路径 N+1 查询。

### SSRF 防护（双重校验）

每个抓取请求在两处独立校验目标 URL：handler 入队前的前置校验（`src/presentation/helpers/`）与引擎路由执行前的二次校验。覆盖重定向跟随（`ssrf_redirect_test`）与 IP/协议规则（`ssrf_mod_test`），受保护端点签名经 `extract_handler` 泛型注入地理限制仓库（`teams` 特性）。

### 请求完整性与密钥安全

- Webhook 投递使用 [Standard Webhooks](https://github.com/standard-webhooks/standard-webhooks) 规范签名，验签采用 `subtle` crate 恒时比较，防时序侧信道。
- HMAC / SHA-2 用于内部签名链路；`bcrypt` 用于口令散列场景；`zeroize` 用于敏感内存清理。
- API Key 明文仅在签发时返回一次；`CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 仅建议用于开发/测试环境自动引导。

### HTTP 层加固

- `security_headers_middleware` 统一注入安全响应头（有专属单元测试固化）。
- `[trusted_proxies]` 配置（CIDR 列表）限定可信代理，防止客户端 IP 伪造绕过限流与防火墙。
- `[cors]` 支持 `allowed_origins` 白名单；生产环境应配置具体来源而非 `*`。
- 出站代理统一经 `[proxy]` 配置；限流由 limiteron 提供分布式限流、配额（`quota-control`）与熔断（`circuit-breaker`）。

---

## ⛓️ 供应链与门禁

| 门禁 | 执行环境 | 内容 |
|------|----------|------|
| `cargo deny check` | CI（cargo-deny-action）+ release job | advisories（yanked 拒绝）、license 白名单（MIT/Apache-2.0/BSD/ISC/MPL-2.0 等许可）、重复版本告警、wildcard 版本拒绝、仅允许 crates.io 来源 |
| RUSTSEC 前提校验 | CI feature-matrix（auth-on 组合） | grep 断言 `src/` 无 `RS256/RS384/RS512/use rsa::` 代码路径——这是 `deny.toml` ignore RUSTSEC-2023-0071（RSA Marvin Attack，crawlrs 仅用 HS256 不触碰 rsa crate）的自动化前提守护；前提一旦失效门禁即失败 |
| CodeQL | CI（`codeql.yml`） | GitHub 代码扫描静态分析 |
| clippy | CI + e2e Stage 2 | `--features standard/full -- -D warnings`（E2E 套件另加 agent-lib 面 `--all-targets`） |
| 私钥扫描 | `scripts/pre-commit-check.sh` | pre-commit 检测私钥与 AWS 密钥模式 |
| rustls 通告 | `deny.toml` 记录 | RUSTSEC-2026-0285（rustls TLS 1.3 握手，CVSS 5.3）已随依赖树锁定 0.23.45 修复版本闭环，无需 ignore |

已知且可接受的风险在 `deny.toml` 内逐条注释（风险评级 + 移除条件），例如 RUSTSEC-2023-0071 的 ignore 依赖「无 RSA 代码路径」前提并由 CI 自动验证。

---

## 🏭 生产部署加固清单

- [ ] 设置强随机 `CRAWLRS__AUTH__JWT_SECRET`（HS256，≥32 字节；弱密钥拒绝启动）
- [ ] 通过 `POST /v1/admin/api-keys` 签发 API Key，明文 key 立即写入 secrets manager（仅返回一次）
- [ ] 生产环境禁用 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`
- [ ] 配置适当的数据库连接池（`[database] max_connections`）与最小权限账号
- [ ] CORS 配置为具体来源（禁用 `*` 通配）
- [ ] 根据容量设置速率限制（`default_limit` / `burst_size`）与团队并发（`concurrency.default_team_limit`）
- [ ] 配置 `[trusted_proxies]`（CIDR）防止 IP 伪造绕过防火墙/限流
- [ ] 启用 Prometheus 指标导出并接入告警（docker/prometheus/ 提供配置样例）
- [ ] 启用结构化日志聚合（inklog），审计日志（`/v1/audit/logs`）纳入巡检
- [ ] 在 TLS 终止（反向代理/网关）之后部署，健康检查走 `/health`
- [ ] 审查 Webhook `secret` 配置，验证接收端签名校验
- [ ] 设置数据库备份与灾难恢复方案

---

## 🧪 安全测试覆盖

| 场景 | 位置 |
|------|------|
| 认证端到端（签发/校验/RBAC/锁定） | `tests/integration/auth_garrison_test.rs`（ignored，真实 PostgreSQL） |
| 暴力破解 429 语义 | 同上 + `tests/unit/presentation/middleware/rate_limit_middleware_test.rs` |
| SSRF 前置/重定向校验 | `tests/unit/presentation/helpers/ssrf_mod_test.rs`、`ssrf_redirect_test.rs` |
| 安全响应头 | `tests/unit/presentation/middleware/security_headers_middleware_test.rs` |
| Token 伪造/绕过 | `tests/unit/presentation`（middleware 域） |
| RSA 前提守护 | CI feature-matrix job（auth-on 组合 grep 断言） |

完整场景矩阵见 [🧪 测试场景矩阵](TEST_SCENARIOS.md)。
