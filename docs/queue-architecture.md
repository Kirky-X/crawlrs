# Queue Architecture — 三队列职责边界

> 决策记录（ADR），change `db-retention-governance` / R-queue-001。定义项目内三套 Postgres 队列机制的职责边界，防止新代码混用机制。

## 背景

项目演进中出现了三套基于 PostgreSQL 的队列/投递机制，各有独立语义与实现：

| 机制 | 核心表 | 入口 | 语义 |
|------|--------|------|------|
| 任务队列 | `tasks` | `PostgresTaskQueue` / `TaskRepository::acquire_next` | 抓取任务调度，优先级 + 行锁占用 |
| 通知 outbox | `webhook_events` | `WebhookEventRepository::find_pending` | webhook 事件重试投递 |
| 通用消息队列 | `message_queues`（+ `message_queues_archive`） | `PostgresMessageQueueRepository` / `MessageQueue` trait | pgmq 风格命名队列消息 |

三者都落在 Postgres，但解决的问题不同。混用会导致：锁语义缺失、优先级不可表达、投递重试与任务调度耦合。

## 各队列机制

### 1. `tasks` — 抓取任务调度（任务队列）

- **机制**：`acquire_next` 两步查询（`status='queued'` 正常路径 + `status='active'` lock 过期恢复路径），`lock_token`/`lock_expires_at` 行级占用，partial index（`idx_tasks_acquire_queued`/`idx_tasks_acquire_stale`）保证低延迟有序取任务。
- **适用条件**：需要优先级（`priority DESC, created_at ASC`）、lock 语义（worker 崩溃后恢复）、调度与任务生命周期（retry/max_retries/expires_at）绑定的工作负载——即抓取任务本身。
- **禁止混用**：
  - 任务调度**禁止**改用 `message_queues`——其无 priority 排序、无 lock/占用语义、无 `acquire_next` 的崩溃恢复路径；任务会被消费者的读节奏改变而失去队首优先。
  - 任务调度**禁止**经 `webhook_events` 表达——该表为事件型投递设计，无任务状态机。

### 2. `webhook_events` — 通知 outbox（重试投递）

- **机制**：`create`（写入 outbox 事件）→ `find_pending`（pending + 到期 failed）→ 投递 → 状态流转（delivered/failed/dead）；`next_retry_at`/`attempt_count`/`max_retries` 控制退避重试。
- **适用条件**：对外通知类事件（crawl 完成等），成功/失败/死信语义明确，需要投递尝试记录。
- **禁止混用**：
  - 不用于承载任务调度（见上）。
  - 不用于进程内解耦/消息广播——那是 `message_queues` 的职责；把广播塞进 outbox 会污染投递状态（无法投递的电影票）。
- **终态清理**（R-retention-004）：`delivered`/`dead` 终态事件按保留期清理（migration 007 partial index 支撑）；`pending`/`failed` 是活事件永不清理。

### 3. `message_queues` — 通用消息队列（pgmq 风格）

- **机制**：命名队列（`queue_name` 区分）；visibility timeout（`vt`）实现"至少一次"消费；`read_batch` 用 `FOR UPDATE SKIP LOCKED` 原子取消息；`archive`/`archive_batch` 移入归档表支持回放；`send_batch`/`read_batch`/`delete_batch` 批量接口。
- **适用条件**：服务内部的事件/命令分发，生产者-消费者解耦，无优先级/无任务生命周期要求的消息流。
- **禁止混用**：
  - 不承载需要优先级与 lock 恢复的调度任务（见 `tasks` 段）。
  - 不承载需要投递重试与状态机的外部通知（见 `webhook_events` 段）——`message_queues` 无投递成功/失败/死信状态字段。

## 收敛触发条件

当前三套机制并存成本可控（各表职责已明确）。出现以下任一信号时，优先收敛到统一机制：

1. **任务吞吐量 > 1k tasks/min 持续 24h**：`tasks` 表的锁更新写放大成为瓶颈 → 评估迁移到独立消息中间件（Redis Streams / NATS / Kafka），或统一 `message_queues`（补齐 priority/lock 语义）。
2. **`message_queues` 使用方增至 ≥ 3 个业务模块**：机制边界开始模糊 → 先做一次使用场景审计，再决定收敛或拆分。
3. **跨队列事务需求出现**（一个业务操作需原子地写 tasks + message_queues）：Postgres 单事务可支持，但需评估是否应统一到单一队列。

收敛候选方案（按成本升序）：增加 `message_queues` 的 priority/lock 能力 → 全部调度迁移到 `message_queues` 并废弃 tasks 队列 → 引入外部队列中间件。

## 决策规则（新代码对照）

写任何"入队/出队/投递"代码前，对照下表选择机制：

| 我需要… | 用 |
|---------|-----|
| 排队一个带优先级、可崩溃恢复的抓取任务 | `tasks` |
| 投递一个外部 webhook 通知（含重试） | `webhook_events` |
| 向 N 个内部消费者分发一条消息 | `message_queues` |
| 批量吞吐、无状态机的事物流 | `message_queues`（`send_batch`/`read_batch`） |