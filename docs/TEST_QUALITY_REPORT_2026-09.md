# crawlrs 测试与质量加固报告（2026-09）

> 范围：全链路测试执行、零报错零告警验证、E2E 测试套件固化（API 语义验证 + 审计日志）。
> 关联文档：[TEST_SCENARIOS.md](TEST_SCENARIOS.md)（场景总矩阵）、`tests/docs/feature-test-matrix.md`（特性组合矩阵）。

## 1. 结论速览

| 项 | 状态 |
|---|---|
| clippy（default / full / agent-lib，`--all-targets -D warnings`） | ✅ 零告警 |
| 依赖 crate（sdforge / trait-kit / limiteron / garrison）编译告警 | ✅ 已清零（本地一等仓库修复） |
| lib 单元测试（default + full） | ✅ 全绿（5522 + 5653 通过，0 失败） |
| 集成测试（PostgreSQL，含 garrison 认证端到端） | ✅ 全绿（152 通过，0 失败） |
| 活体 API E2E（状态码冒烟 + 语义级验证 + 审计日志） | ✅ **冒烟 91/91 + 语义 47/47**（2026-09-16，9 轮迭代收敛） |
| **全量 7 阶段套件单次通过** | ✅ matrix/static/unit/integration/bench/liveapi 全 PASS（`test-results/e2e-report.txt`，终版无重试触发） |
| 发现并修复的既有缺陷 | 13 项修复 + 1 项语义澄清（见 §3） |

## 2. 测试覆盖矩阵（场景 → 验证层 → 结果）

### 2.1 单元 / 集成层（Rust）

| 场景类别 | 正常路径 | 边界条件 | 异常与容错 | 验证层 |
|---|---|---|---|---|
| 领域模型 / 状态机 | ✅ | ✅（极值/空值/非法词汇） | ✅ | L1 内联 + L2 `tests/unit/`（71 文件） |
| 引擎路由与降级（SmartHybrid 等） | ✅ | ✅（并发竞争、fallback） | ✅（超时/引擎不可用） | L1 + L2 + 表驱动机 L3 |
| 仓库层（真实 PostgreSQL） | ✅ | ✅（分页/过滤/锁） | ✅（连接失败重试） | L6 集成（testcontainers/compose） |
| 认证（garrison RBAC） | ✅（签发→验证→鉴权） | ✅（过期/吊销/32 字节密钥边界） | ✅（INVALID_TOKEN/暴力破解防护） | L6 认证端到端 |
| SSRF 防护 | ✅ 放行公网 | ✅（内网段/IPv6/metadata/代理） | ✅（DNS 解析失败 fail-closed） | L1/L2 + 语义 E2E |
| 中间件（限流/信号量/CORS/安全头/i18n） | ✅ | ✅（阈值边界） | ✅ | L2 + 语义 E2E |
| Workers（webhook/backlog/expiration） | ✅ | ✅ | ✅ | L2 + 集成 |
| API 响应包封一致性 | ✅ | ✅（未知字段 422 / 非法 JSON 400） | ✅（错误包封无 data 泄漏） | **语义 E2E（新增）** |

### 2.2 活体 API 语义层（`tests/e2e/api_semantics_test.py`，47 场景）

每次运行自动生成 `test-results/api-semantics-matrix.md`（场景 × 结果 × 失败明细）。
场景分组：

- **A 公开端点**：/health、/v1/version、/ready（200|503 双合法态）、/metrics、404 语义
- **B 认证**：无凭证/无效凭证 401 包封（UNAUTHORIZED）、团队画像/用量
- **C Scrape**：创建回显（url/credits_used/UUID）、状态机推进至终态、结果完整性
  （content/status_code/response_time_ms）、敏感响应头 `[REDACTED]` 脱敏、
  sync_wait 同步模式、5 类无效输入、7 类 SSRF 目标（含不产生任务的断言）
- **D Crawl**：生命周期（创建→终态→结果非空）、`include_patterns` 结果过滤、
  取消状态转换（204→cancelled）、不存在 404、max_depth 上限 422
- **E Search**：成功路径 / 环境受限降级（500 显式错误包封，禁止静默空成功）、
  空 query、未知引擎
- **F 安全脱敏**：key 签发一次性明文 + `Cache-Control: no-store`/`Pragma`/`Expires`
  防缓存头、新 key 立即可用、只读 scope 越权签发 403、nil team/空 scopes 400、
  geo 限制写后读一致、审计日志不含明文凭证
- **G 任务过滤**：`team_id` 以认证身份为准（请求体不可越权，IDOR）、limit/
  task_types 过滤精确、批量取消空请求 422
- **H 并发**：并发提交全部受理 + 任务 id 唯一性

> **真实站点约束（2026-09-16 起）**：E2E/集成/语义测试全部使用真实新闻网站
> （主站 `text.npr.org`，故障转移 `news.ycombinator.com`、`lite.cnn.com`），
> 禁止 `example.com`（仅允许单元测试使用）；抓取结果断言含真实站点内容标记
> （npr.org/ycombinator/cnn.com），防占位页冒充。

每次交互（请求/响应/断言）记录至 `test-results/api-audit.jsonl`（Authorization
与已签发 key 全程掩码），供人工审查安全脱敏、状态转换与数据过滤细节。

## 3. 发现的既有缺陷与修复

| # | 层 | 缺陷 | 修复 | 影响 |
|---|---|---|---|---|
| 1 | crawlrs 路由装配 | **回归**：`create_scrape` 以 `Extension<Option<Arc<dyn GeoRestrictionRepository>>>` / `Option<Arc<TeamService>>` 提取，装配未注入 Option 形态 → **所有 POST /v1/scrape 500**（活体 E2E 首跑暴露） | `bootstrap/routes.rs` teams-on/off 两分支显式注入 `Extension(Some(..))`/`Extension(None::<..>)` | 高（核心入口不可用） |
| 2 | crawlrs API 契约 | tasks/_cancel、_query 等端点的 axum JSON 提取拒绝返回**纯文本**响应，绕过 `ApiResponse` 包封（静默失败风险） | 新增 `json_rejection_response`/`json_rejection_error` 助手 + `CrawlRsError::Unprocessable`（422）变体；9 个 JSON handler 统一采用 | 中 |
| 3 | crawlrs E2E 工具 | `api_test.sh` 硬编码 BASE_URL 与 /ready 单一合法态、结果文件绝对路径 | BASE_URL 可经 `CRAWLRS_TEST_BASE_URL` 覆盖；/ready 接受 200\|503；路径仓库相对化 | 低 |
| 4 | limiteron | `event-system` 特性关闭时 `EventOutbox` re-export/dead_code 告警；`dispatcher.rs` webhook 分派未按特性门控 → **webhook-off 编译失败**（E0425/E0061 既有缺陷） | re-export 与分派块同 `event-system`/`webhook` 特性门控；删除不可达存根；保留项加条件 `allow(dead_code)` | 中（该仓库自身） |
| 5 | trait-kit | `ReloadSubscriber` 别名在 `reload` 特性关闭时 dead_code；`e2e_async.rs` 测试模块未随 `decorator` 特性门控 | 别名与定义随使用处同门控 | 低 |
| 6 | sdforge | `config/app.rs` 顶部 `use ValidateConfig` 未使用（impl 用全限定路径，测试另有局部导入） | 删除该导入 | 低 |
| 7 | 环境适配 | Docker Desktop WSL 集成未随宿主自启导致套件首跑失败 | 编排脚本保持既有 fail-fast 语义；本次运行前手动拉起 Docker Desktop | 低 |
| 8 | **安全（CWE-798/540）** | `tests/e2e/test_scenarios.py` 在库内**硬编码历史 garrison API key**（`api_test.sh` 注释中"历史泄漏密钥"的源头） | 改为 `CRAWLRS_TEST_API_KEY` 环境变量注入，未设置即 SKIP；库内引用已清零。**失效实证（2026-09-16）**：garrison DAO 为进程内 oxcache 存储，该 key 仅存在于早已销毁的服务进程内存中——活体服务复验：携带泄漏 key 访问 `/v1/teams/me` 与 `/v1/admin/api-keys` 均 401（INVALID_TOKEN），持久层 `api_keys` 映射 0 行，结构上不可能对任何现存/未来实例生效，无需额外吊销动作 | 高（凭证泄漏，已实证失效） |
| 9 | **crawlrs 致命缺陷** | `ShutdownCoordinator::wait_for_completion` 用 `select! { notified, sleep(graceful_period) }` 实现——无信号时最迟 30s 也返回 → **`crawlrs worker` 启动 30 秒后必然静默退出**（守护进程语义根本性错误；活体 E2E 任务滞留 queued 直查定位） | `wait_for_completion` 改为仅等 `notify`（无限等待）；宽限期预算仅作用于触发后的 drain 流程；更新回归测试（`test_wait_for_completion_waits_indefinitely_without_trigger`） | 高（worker 模式不可用） |
| 10 | crawlrs 行为澄清（非缺陷） | `include_patterns` 作用于抽取发现的**外链**（`crawl_link_extractor::UrlPatternFilter`），种子 URL 无条件抓取，可能出现在结果中 | 语义 E2E 按此契约断言（种子豁免 + 非种子外链全部命中模式）——若 API 层期望"结果 ⊆ pattern"，需后续在设计层面明确 | 低（语义澄清） |
| 11 | crawlrs 测试 | `rate_limiting_service_test.rs` 直调 `LimiteronService::new` 未跟进第 5 参 `StorageHandle`（b76d330c 吸收批次遗留，`*+test-mocks` 矩阵组合编译失败） | 两处直调补 `StorageHandle::Memory`（与 src 内测试助手同口径） | 中（矩阵 2 组合红） |
| 12 | crawlrs 工程规范 | `src/engines/client/reqwest.rs` 存在既有 fmt 违规（注释缩进，吸收批次遗留） | `cargo fmt` 修复并验证 | 低 |
| 14 | crawlrs 生产健壮性 | `wait_audit_tasks` 用 `join_all()`——遇被取消的 audit task 直接 panic（"task was cancelled"）。跨 runtime 场景（并行测试）真实触发；shutdown 等待方被连带炸掉 | 改为逐个 `join_next()`：cancelled 视为完成，task panic 记录日志不传播（监听器 best-effort 契约） | 中（shutdown 路径） |
| 13 | 套件稳定性 | 共享测试库的全局队列语义测试（acquire_next/reset_stuck 系列）与真实池用例在高并行 + 宿主高负载下存在竞态/瞬态抖动（含 WSL2 Docker 端口转发 `ConnectionClosed`，既有已定性环境问题） | ① 11 个队列语义测试以共享 `tokio::sync::Mutex` 串行化；② lib/mock-main 跑加"失败重试一次"护栏（确定性失败重试仍红，不掩盖真问题） | 中（套件信噪比） |

> 说明：缺陷 1/2/9 触碰 crawlrs 符号，按仓库规约需 GitNexus 影响分析；本会话
> GitNexus MCP 工具不可用，已以人工等价方式完成影响面梳理（全部 `Extension<Option<…>>`
> 提取点 = 仅 `create_scrape`；全部装配点 = `create_protected_routes_with_state` 单点；
> JSON 提取点清单见 §3#2 修复提交），风险等级：中；并通过全量测试链回归验证。

## 4. E2E 套件执行方式

```bash
# 全量 7 阶段（矩阵→静态→单测→集成→bench→活体API→报告）
./tests/e2e/e2e-suite.sh

# 只跑活体 API E2E（compose PG → 迁移 → 进程内自举 admin key → 起服 → 冒烟+语义 → 审计产物）
./tests/e2e/live-api-e2e.sh
./tests/e2e/live-api-e2e.sh --skip-build     # 跳过 cargo build
KEEP_ENV=1 ./tests/e2e/live-api-e2e.sh       # 保留环境便于排查

# 已有运行中的服务时单独跑语义验证
python3 tests/e2e/api_semantics_test.py \
  --base-url http://localhost:8899 \
  --api-key "$KEY" --team-id "$TEAM_ID" \
  --audit-log test-results/api-audit.jsonl
```

### 关键实现语义（排障备忘）

- garrison DAO 为**进程内** oxcache：API key 必须由服务进程经
  `CRAWLRS_BOOTSTRAP_ADMIN=true` 自举签发；`crawlrs bootstrap` 子命令签发的
  key 跨进程不可见（verify 报 INVALID_TOKEN）。
- `run_bootstrap` 仅在 TTY 输出明文 key（管道下掩码，CWE-532），编排用
  `script` 伪 TTY 捕获后**立即掩码日志**；伪 TTY 为 `\r\n` 行尾，解析需 `tr -d '\r'`。
- 抓取按次扣积分（402 QUOTA_EXCEEDED 为正确配额语义），编排自举后为测试团队充值。
- garrison IP 暴力破解防护默认 5 次/60s、锁 300s（FirewallBlocked→429）：
  E2E 的无效凭证负例次数须保持在此阈值内。

## 5. 审计产物一览（`test-results/`）

| 文件 | 内容 |
|---|---|
| `api-audit.jsonl` | 全部 HTTP 交互（请求/响应/断言/耗时），凭证掩码（含运行结束时对签发 key 的回溯掩码） |
| `api-interaction-digest.md` | 人工审查摘要：安全脱敏/状态转换/数据过滤/错误包封四类取证点 + 状态机观测序列 + 全量结果矩阵 |
| `api-semantics-matrix.md` | 语义覆盖矩阵（场景 × 结果 × 失败明细） |
| `api-semantics.log` / `api-smoke.log` | 语义验证与状态码冒烟输出 |
| `liveapi-server.log` / `liveapi-worker.log` | API 服务与 worker 运行日志（key 已掩码） |
| `e2e-report.txt` | 7 阶段套件汇总 |
| `stage*.log` | 各阶段原始日志 |

## 6. 复核清单

- [x] `./tests/e2e/e2e-suite.sh` 全阶段 PASS（2026-09-16 08:15 终版，`test-results/e2e-report.txt`）
- [x] `test-results/api-semantics-matrix.md` 0 FAIL（47/47）
- [x] `test-results/api-audit.jsonl` 抽查：脱敏、状态转换、数据过滤场景符合预期
  （人工审查入口：`test-results/api-interaction-digest.md`）
- [x] 依赖 crate 修复已提交：trait-kit `563bf11`、limiteron `0241aa1`（sdforge 的导入修复已随其 HEAD 生效，无需单独提交）
- [x] 并行会话在途的 R-key 注记清理已收编提交（19 文件纯注释变更，`8414dd1c`，该状态已受终版套件验证）
- [x] `test_results.txt` 冒烟产物已入 .gitignore（`17ec4763`）
