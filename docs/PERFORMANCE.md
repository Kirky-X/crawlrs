# ⚡ Crawlrs 性能指南

crawlrs 的性能工程内建于架构（AIMD 自适应并发、内存感知调度、TabPool、请求合并、WaitFor、Hedge 等）。本指南介绍可复现的基准套件、性能观测手段与调优建议。基准设计细节与模块实现见 [🏗️ 架构文档](ARCHITECTURE.md)。

## 📋 目录

- [🎯 概述](#-概述)
- [📏 基准套件](#-基准套件)
- [📊 历史对比数据说明](#-历史对比数据说明)
- [⚡ 运行时性能设计](#-运行时性能设计)
- [💾 缓存与资源配置](#-缓存与资源配置)
- [📈 可观测性与测量](#-可观测性与测量)

---

## 🎯 概述

- **可复现基准**：`benches/benchmark.rs` 提供 9 组 Criterion 基准，`cargo bench` 一键运行；E2E 套件 Stage 5 首跑建立 `e2e-baseline`，此后每次全量运行自动对比检测性能回退。
- **观测优先**：Prometheus 指标（`/metrics`）暴露队列深度、引擎成功率、引擎耗时分布、缓存命中率与 Webhook 投递结果，调优前先用数据定位瓶颈。
- **按特性裁剪**：未启用的引擎/业务能力不参与编译与运行时，`--no-default-features` 可获得最小资源占用的部署面。

### 性能目标

crawlrs 以高吞吐、低尾延迟（P99）与可控内存为目标。当前仓库内**尚未发布权威的基准数字**——旧版宣传的 Node.js 对比数据缺乏测量口径，见[历史对比数据说明](#-历史对比数据说明)；复现方式见[基准套件](#-基准套件)。

---

## 📏 基准套件

`benches/benchmark.rs`（`[[bench]]` name=`benchmark`，harness=false，Criterion 0.8）包含 9 组基准：

| 基准组 | 测量内容 |
|--------|----------|
| `benchmark_task_creation` | 任务创建路径开销 |
| `benchmark_task_status_transitions` | 任务状态机迁移开销 |
| `benchmark_json_serialization` | JSON 序列化/反序列化（serde_json） |
| `benchmark_url_parsing` | URL 解析 |
| `benchmark_url_validation` | URL 校验 |
| `benchmark_uuid_generation` | UUID 生成 |
| `benchmark_ssrf_detection` | SSRF 检测热路径 |
| `benchmark_regex_cache` | RegexCache 缓存命中/构建 |
| `benchmark_engine_routing` | 引擎路由（多 mock 引擎下的策略选择） |

```bash
# 运行全部基准（release profile，继承 lto=thin / codegen-units=1 / opt-level=3）
cargo bench

# 查看报告
target/criterion/report/index.html
```

### 与 E2E 基线联动

`./tests/e2e/e2e-suite.sh` 的 Stage 5 以缩短采样参数编译并短跑基准：

- 首次运行建立 `e2e-baseline` 基线；
- 之后每次运行自动与基线对比，显著回退即失败退出——性能回归与功能回归同等级看护。

---

## 📊 历史对比数据说明

项目早期宣传材料中曾引用如下对比（相对 Node.js 版实现）：

| 指标 | 早期宣传口径 |
|--------|-------------|
| 吞吐量 | 提升 3-5 倍（约 1,200 → 4,500 请求/秒） |
| P99 延迟 | 降低约 50-60%（450ms → 180ms） |
| 内存使用 | 降低约 75%（512 MB → 128 MB） |
| CPU 使用 | 降低约 59%（85% → 35%） |

> ⚠️ **该组数字未附带测量脚本、环境与负载模型，仓库内无法复现，仅作历史参考。** 权威基准数据待补充：计划以 Criterion 套件与 k6 压测（`tests/stress/k6_script.js`）在固定环境下重测后更新本节。

---

## ⚡ 运行时性能设计

以下机制均已实现并有对应测试/基准（代码位置与设计细节见 [🏗️ 架构文档 · 爬取能力增强模块](ARCHITECTURE.md)）：

| 机制 | 收益 |
|------|------|
| AIMD 自适应并发（`adaptive_concurrency.rs`） | 无锁 `AtomicUsize` + `AdaptiveSemaphore`，连续失败减半、成功递增，自动逼近最优并发 |
| 内存感知调度（`workers/scheduler/`） | `MemoryState{Normal,Pressure,Critical}` 三态准入 + 优先级队列老化，防止高压下 OOM 与饿死 |
| TabPool（Chrome CDP Tab 池） | DashMap + AtomicUsize LIFO 栈复用 Tab，消除每次请求的 tab 创建开销 |
| 请求合并（`coalesce.rs`） | DashMap 单飞 + broadcast 通知，同 URL 并发仅 1 次实际抓取 |
| WaitFor 条件等待 | NetworkIdle / Selector / DomStable 替代固定 sleep，降低无效等待 |
| Hedge 请求副本控制器 | EMA + 方差估算 P84 阈值，超阈值发起副本请求 race，降低长尾延迟 |
| 智能重试 + Full-jitter 退避 | 按 RetryReason 独立上限，避免重试风暴 |
| RegexCache / Bloom⊕Interner 去重 | 正则编译缓存与 URL 分层去重，降低 CPU 与 DB 查询量 |
| Waterfall/MRT 超时 | 按引擎配置 MRT，超时即切换下一引擎，避免单引擎拖垮尾延迟 |

---

## 💾 缓存与资源配置

### 多层缓存（oxcache / moka）

`[cache]` 配置段控制 L1 内存缓存（`enabled` / `[cache.memory] capacity` / `ttl_seconds`），并支持 search / dns / regex 三种分类型 TTL（`[cache.types.*]`）；另有 5 种缓存模式（Enabled / Disabled / ReadOnly / WriteOnly / Bypass）按请求粒度门控（`CacheContext`）。

### 关键调优参数

| 配置 | 默认 | 建议 |
|------|------|------|
| `[cache.memory] capacity` / `ttl_seconds` | 10000 / 300 | 按工作集大小上调可提高命中率（观测 `crawlrs_cache_hit_total`） |
| `[concurrency] default_team_limit` | 10 | 多租户场景按团队配额调整 |
| `[workers] count` | `auto` | CPU 密集型可设为物理核数；IO 密集可更高 |
| `[timeouts.engines]` 各 MRT | 见 `config/default.toml` | 依据目标站点响应分布校准，过小会放大引擎切换率 |
| `[rate_limiting] default_limit` / `burst_size` | 60 / 20 | 与下游站点承受度匹配，避免无谓 429 重试 |
| `[proxy]` 轮换与粘性会话 | - | 被封禁域名较多时启用按类别路由与健康检查 |

---

## 📈 可观测性与测量

### Prometheus 指标（`metrics` 特性，`GET /metrics`）

| 指标 | 类型 | 说明 |
|------|------|------|
| `crawlrs_queue_depth` | Gauge | 任务队列深度 |
| `crawlrs_engine_success_total` | Counter | 引擎成功/失败计数 |
| `crawlrs_engine_duration_seconds` | Histogram | 引擎请求耗时分布 |
| `crawlrs_cache_hit_total` | Counter | 缓存命中/未命中 |
| `crawlrs_webhook_delivery_total` | Counter | Webhook 投递成功/失败 |

`docker/prometheus/` 提供 Prometheus 与 Alertmanager 配置样例；`docker/docker-compose.yml` 一键拉起完整观测栈。

### 测量方法建议

1. **微基准**：`cargo bench`（本页基准套件），用于库内热路径回归。
2. **端到端压测**：`k6 run tests/stress/k6_script.js` 对运行中的服务施压，观测吞吐与分位延迟。
3. **Python 性能探针**：`python -m pytest tests/python/test_performance.py`（或 `./scripts/run-tests.sh local`）做基础响应时间巡检。
4. **生产观测**：Grafana 面板关注 `queue_depth` 持续增长（worker 不足）、`engine_duration_seconds` 分位突增（引擎/站点劣化）与 `cache_hit_total` 命中率下降（容量不足）。
