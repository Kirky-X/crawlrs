# crawlrs 项目上下文文档

## 项目概述

**crawlrs** 是一个用 Rust 开发的高性能企业级网页数据采集平台，提供搜索、抓取、爬取、映射与结构化提取能力。相比传统 Node.js 方案，性能提升 3-5 倍，P99 延迟降低 50%。

### 核心优势
- 🚀 **高性能**: 单机 10000+ RPS，P99 延迟 < 200ms
- 🛡️ **类型安全**: 利用 Rust 编译期检查，消除 90% 运行时错误
- 🔄 **弹性扩展**: 支持单机和集群部署，按需水平扩展
- 📊 **可观测性**: 内置分布式追踪和 Prometheus 指标
- 🔐 **企业级**: SSRF 防护、速率限制、多租户隔离

### 核心功能
- **搜索 (Search)**: 多引擎并发聚合（Google/Bing/Baidu/Sogou），智能去重排序，支持异步回填
- **抓取 (Scrape)**: 单页面内容获取，支持多格式输出（Markdown/HTML/截图/JSON）
- **爬取 (Crawl)**: 全站递归爬取，支持深度控制和路径过滤
- **提取 (Extract)**: 基础 CSS 选择器结构化数据提取

### 技术栈
| 组件 | 技术 | 版本 |
|------|------|------|
| **Web 框架** | Axum | 0.8 |
| **ORM** | SeaORM | 1.0 |
| **异步运行时** | Tokio | 1.48 |
| **数据库** | PostgreSQL | 15+ |
| **缓存** | Redis | 7+ |
| **HTTP 客户端** | reqwest | 0.12 |
| **浏览器自动化** | chromiumoxide | 0.8 |
| **限流** | governor | 0.10 |
| **日志** | tracing | 0.1 |

## 项目结构

```
crawlrs/
├── src/
│   ├── main.rs              # 应用入口，服务启动逻辑
│   ├── lib.rs               # 库导出
│   ├── application/         # 应用层（用例）
│   │   ├── dto/            # 数据传输对象
│   │   └── use_cases/      # 业务用例
│   ├── config/             # 配置管理
│   ├── domain/             # 领域层（核心业务逻辑）
│   │   ├── models/         # 领域模型
│   │   ├── repositories/   # 仓储接口
│   │   ├── search/         # 搜索引擎抽象
│   │   ├── services/       # 领域服务
│   │   └── use_cases/      # 领域用例
│   ├── engines/            # 抓取引擎实现
│   ├── infrastructure/     # 基础设施层
│   │   ├── cache/          # 缓存实现
│   │   ├── database/       # 数据库连接
│   │   ├── repositories/   # 仓储实现
│   │   ├── search/         # 搜索实现
│   │   └── services/       # 基础设施服务
│   ├── presentation/       # 表现层
│   │   ├── handlers/       # HTTP 处理器
│   │   ├── middleware/     # 中间件
│   │   └── routes/         # 路由定义
│   ├── queue/              # 任务队列
│   ├── utils/              # 工具函数
│   └── workers/            # 后台工作器
├── migration/              # 数据库迁移
├── config/                 # 配置文件
├── docker/                 # Docker 配置
├── examples/               # 示例代码
├── tests/                  # 测试文件
│   ├── integration/        # 集成测试
│   ├── e2e/               # 端到端测试
│   └── unit/              # 单元测试
├── benches/                # 性能测试
└── docs/                   # 文档
```

## 构建和运行

### 环境要求
- **Rust**: 1.75+ (Edition 2021)
- **PostgreSQL**: 15+
- **Redis**: 7+
- **Docker** (可选): 用于容器化部署

### 开发模式

#### 从源码编译

```bash
# 克隆仓库
git clone https://gitee.com/kirky-x/crawlrs.git
cd crawlrs

# 编译项目
cargo build --release

# 运行测试
cargo test

# 启动 API 服务
cargo run -- api

# 启动 Worker 服务
cargo run -- worker
```

#### 使用配置文件

配置文件位于 `config/default.toml`，包含数据库、Redis、搜索引擎等配置：

```toml
[database]
url = "postgres://crawlrs:password@localhost:5432/crawlrs_test"
max_connections = 20

[redis]
url = "redis://localhost:6379"

[server]
host = "0.0.0.0"
port = 8899
enable_port_detection = true

[rate_limiting]
enabled = false
default_rpm = 60

[concurrency]
default_team_limit = 10
task_lock_duration_seconds = 300
```

### Docker 部署

#### 完整栈启动

```bash
# 启动所有服务（包括基础设施、应用、浏览器、监控）
docker-compose up -d

# 查看日志
docker-compose logs -f crawlrs

# 停止所有服务
docker-compose down
```

#### 分层启动

```bash
# 仅启动基础设施（PostgreSQL, Redis）
docker-compose --profile infrastructure up -d

# 仅启动应用
docker-compose --profile app up -d

# 仅启动浏览器服务
docker-compose --profile browser up -d

# 仅启动监控服务
docker-compose --profile monitoring up -d
```

#### 环境变量配置

创建 `.env` 文件或使用 `docker/.env.example`：

```env
# 数据库配置
DB_USER=crawlrs
DB_PASSWORD=password
DB_NAME=crawlrs
DB_PORT=5432

# Redis 配置
REDIS_HOST=redis
REDIS_PORT=6379

# 应用配置
APP_PORT=3000
RUST_LOG=debug

# 搜索引擎配置
SEARCH_ENGINE_GOOGLE_ENABLED=false
SEARCH_ENGINE_BING_ENABLED=true
SEARCH_ENGINE_BAIDU_ENABLED=true
SEARCH_ENGINE_SOGOU_ENABLED=true

# Google 搜索 API
GOOGLE_SEARCH_API_KEY=your-api-key
GOOGLE_SEARCH_CX=your-cx-id
```

### 服务类型

项目支持两种服务模式：

1. **API 服务** (`cargo run -- api`): 提供 HTTP API 接口，包含 Webhook 处理
2. **Worker 服务** (`cargo run -- worker`): 后台任务处理，执行抓取任务

### 数据库迁移

数据库迁移使用 SeaORM，迁移文件位于 `migration/src/`：

```bash
# 运行迁移
cargo run --bin migrate

# 或在启动时自动运行（main.rs 中已配置）
```

### 健康检查

```bash
# 检查服务状态
curl http://localhost:8899/health

# 查看版本信息
curl http://localhost:8899/v1/version

# 查看 Prometheus 指标
curl http://localhost:8899/metrics
```

## 开发约定

### 代码风格

#### Rust 规范

```bash
# 格式化代码
cargo fmt

# Lint 检查
cargo clippy -- -D warnings

# 运行测试
cargo test

# 运行特定测试
cargo test integration_tests

# 运行性能测试
cargo bench
```

#### 架构分层

项目采用六边形架构（Hexagonal Architecture）：

1. **领域层 (Domain)**: 核心业务逻辑，不依赖外部框架
   - `models/`: 领域模型
   - `repositories/`: 仓储接口
   - `services/`: 领域服务接口

2. **应用层 (Application)**: 编排领域对象执行用例
   - `use_cases/`: 具体用例实现
   - `dto/`: 数据传输对象

3. **基础设施层 (Infrastructure)**: 外部依赖实现
   - `repositories/`: 仓储实现
   - `cache/`: 缓存实现
   - `services/`: 基础设施服务

4. **表现层 (Presentation)**: HTTP 接口
   - `handlers/`: 请求处理器
   - `middleware/`: 中间件
   - `routes/`: 路由定义

#### 依赖注入

使用 `Arc` 和 trait 实现依赖注入：

```rust
// 仓储实现
let task_repo = Arc::new(TaskRepositoryImpl::new(db.clone()));

// 服务实现
let rate_limiting_service = Arc::new(RateLimitingServiceImpl::new(
    redis_client.clone(),
    task_repo.clone(),
    // ...
));

// 注入到处理器
.layer(Extension(task_repo))
.layer(Extension(rate_limiting_service))
```

### 测试规范

#### 测试类型

1. **单元测试**: 位于 `src/` 各模块内，使用 `#[cfg(test)]`
2. **集成测试**: 位于 `tests/integration/`，测试组件交互
3. **端到端测试**: 位于 `tests/e2e/`，测试完整业务流程
4. **性能测试**: 位于 `benches/`，使用 criterion

#### 测试运行

```bash
# 运行所有测试
cargo test

# 运行集成测试
cargo test --test integration_tests

# 运行特定测试
cargo test test_scrape_handler

# 运行测试并显示输出
cargo test -- --nocapture

# 运行性能测试
cargo bench
```

### 错误处理

使用 `thiserror` 和 `anyhow` 进行错误处理：

```rust
// 自定义错误类型
#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}

// 在 main 函数中使用 anyhow
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ...
}
```

### 日志规范

使用 `tracing` 进行结构化日志：

```rust
use tracing::{info, warn, error, debug};

// 初始化日志
telemetry::init_telemetry();

// 记录日志
info!("Starting crawlrs...");
debug!("Processing request: {:?}", request);
warn!("Rate limit approaching: {}/{}", current, limit);
error!("Failed to scrape URL: {}", url, error = %err);
```

### 并发控制

#### 速率限制

使用 `governor` 实现令牌桶限流：

```rust
// 配置
[rate_limiting]
enabled = true
default_rpm = 60  // 每分钟 60 次请求
```

#### 并发控制

使用信号量控制并发任务数：

```rust
// 配置
[concurrency]
default_team_limit = 10  // 每个团队最多 10 个并发任务
task_lock_duration_seconds = 300  // 任务锁定时间
```

### 配置管理

#### 配置加载

使用 `config` crate 加载配置，支持环境变量覆盖：

```rust
// 环境变量格式: CRAWLRS__SECTION__KEY
// 示例: CRAWLRS__DATABASE__URL=postgres://...

let settings = Settings::new()?;
```

#### 配置优先级

1. 环境变量（最高优先级）
2. 配置文件 (`config/default.toml`)
3. 默认值

### 数据库操作

#### 使用 SeaORM

```rust
use sea_orm::*;

// 查询
let tasks = Task::find()
    .filter(task::Column::Status.eq(TaskStatus::Pending))
    .limit(10)
    .all(db.as_ref())
    .await?;

// 插入
let task = task::ActiveModel {
    url: Set(url.to_string()),
    status: Set(TaskStatus::Pending),
    ..Default::default()
};
task.insert(db.as_ref()).await?;

// 更新
let task: task::ActiveModel = task.into();
task.status = Set(TaskStatus::Completed);
task.update(db.as_ref()).await?;
```

### 搜索引擎集成

#### 智能搜索引擎

使用 `EngineRouter` 自动选择最优抓取引擎：

```rust
// 引擎优先级
// 1. ReqwestEngine - 静态页面
// 2. PlaywrightEngine - 动态页面
// 3. FireEngineTls - TLS 指纹绕过
// 4. FireEngineCdp - CDP 协议

let router = Arc::new(EngineRouter::new(engines));
```

#### 搜索引擎配置

```toml
[search.engines]
google_enabled = false
bing_enabled = true
baidu_enabled = true
sogou_enabled = true

[search.flaresolverr]
enabled = true
url = "http://flaresolverr:8191/v1"
```

### Webhook 集成

#### 配置 Webhook

```toml
[webhook]
timeout_seconds = 10
max_retries = 3
retry_interval_seconds = 60
secret = "your-webhook-secret"
```

#### 接收事件

系统会 POST 到配置的 Webhook URL，事件类型包括：
- `scrape.completed` - 抓取完成
- `scrape.failed` - 抓取失败
- `crawl.completed` - 爬取完成
- `crawl.failed` - 爬取失败

### 性能优化

#### 缓存策略

- 使用 Redis 缓存 robots.txt
- 使用 LRU 缓存热点数据
- 使用 CDN 缓存静态资源

#### 并发优化

- 使用 Tokio 异步运行时
- 使用连接池复用数据库连接
- 使用信号量控制并发数

#### 引擎选择

根据页面类型选择合适的引擎：
- 静态页面 → ReqwestEngine
- SPA 应用 → PlaywrightEngine
- 反爬虫站点 → FireEngineTls/CDP

## 常见问题

### 端口冲突

如果默认端口被占用，系统会自动检测并切换到可用端口：

```bash
# 禁用端口检测
[server]
enable_port_detection = false
```

### 数据库连接失败

检查数据库配置和网络连接：

```bash
# 测试数据库连接
psql postgres://crawlrs:password@localhost:5432/crawlrs_test
```

### Redis 连接失败

检查 Redis 服务状态：

```bash
# 测试 Redis 连接
redis-cli -h localhost -p 6379 ping
```

### 任务队列阻塞

检查并发配置和任务锁定时间：

```toml
[concurrency]
default_team_limit = 10
task_lock_duration_seconds = 300
```

## 贡献指南

### 开发流程

1. Fork 仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

### 代码审查

- 确保所有测试通过
- 运行 `cargo fmt` 格式化代码
- 运行 `cargo clippy` 检查代码质量
- 更新相关文档

## 相关文档

- [README.md](./README.md) - 项目介绍
- [README_zh.md](./README_zh.md) - 中文版项目介绍
- [USER_GUIDE.md](./USER_GUIDE.md) - 用户使用手册
- [API 文档](./docs/API.md) - API 参考
- [架构文档](./docs/architecture.md) - 系统架构说明

## 许可证

MIT License - 详见 [LICENSE](./LICENSE) 文件