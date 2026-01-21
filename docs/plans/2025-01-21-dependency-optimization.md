# 依赖导入优化与二进制体积缩减方案

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 通过特性分层、条件编译和依赖精简，将默认编译的二进制体积从 43MB 减少到 25-30MB（lite 版本），同时支持自由组合的预设配置。

**Architecture:** 采用三层特性架构（核心层/扩展层/实验层），通过 Cargo feature flags 实现条件编译，精简 tokio 运行时依赖，移除未使用的大型依赖。

**Tech Stack:** Rust 1.49+, Cargo features, conditional compilation (`#[cfg(...)]`)

---

## 当前状态分析

### 1.1 现状

| 项目 | 当前状态 | 问题 |
|-----|---------|------|
| 二进制体积 | 43MB (default) | 包含大量未使用的功能 |
| Tokio | `features = ["full"]` | 包含 process, signal 等未使用模块 |
| S3 存储 | 默认启用 | 不是所有部署都需要 |
| AI 功能 | 默认启用 | genai 是实验性功能 |
| Playwright | 可选 | 较大 (~20MB)，需要时可启用 |

### 1.2 使用频率分析

| 功能 | 使用频率 | 优化建议 |
|-----|---------|---------|
| 核心爬取 (reqwest) | 高 | 始终保留 |
| SQLite | 中 | lite 版本默认 |
| Postgres | 高 | default 版本默认 |
| Redis 缓存/限流 | 中 | 可选 |
| Playwright JS 渲染 | 高 | standard 版本默认 |
| S3 云存储 | 中 | full 版本可选 |
| Fire 引擎 (CDP/TLS) | 低 | full 版本可选 |
| AI 功能 (genai) | 低 | experimental 显式启用 |
| 搜索引擎 | 低 | full 版本可选 |

---

## 实施方案

### Task 1: 重构 Cargo.toml 特性配置

**Files:**
- Modify: `Cargo.toml`

**Step 1: 分析并记录当前特性**

```bash
# 运行以下命令并记录结果
cargo tree -e features --depth 1 > /tmp/current-features.txt
```

**Step 2: 重构 Cargo.toml**

```toml
[features]
# 默认核心功能（不包含 S3、genai）
default = ["engine-reqwest", "redis-cache", "rate-limiting", "metrics", "db-postgres"]

# 预设配置
lite = ["engine-reqwest", "db-sqlite"]
standard = ["default", "engine-playwright"]
full = ["standard", "engine-fire-cdp", "engine-fire-tls", "engine-flaresolverr", "db-sqlite", "storage-s3", "search-all"]

# 实验性功能（需要显式启用）
experimental = ["genai"]

# 搜索引擎组合特性
search-all = ["search-google", "search-bing", "search-baidu", "search-sogou"]

# 引擎特性
engine-reqwest = []
engine-playwright = ["dep:chromiumoxide"]
engine-fire-cdp = []
engine-fire-tls = []
engine-flaresolverr = []

# 存储特性
storage-s3 = []

# 其他特性保持不变...
search-google = []
search-bing = []
search-baidu = []
search-sogou = []
redis-cache = []
rate-limiting = ["dep:redis"]
metrics = ["dep:metrics", "dep:metrics-exporter-prometheus"]
db-postgres = ["sea-orm/sqlx-postgres", "sqlx/postgres"]
db-sqlite = ["sea-orm/sqlx-sqlite", "sqlx/sqlite"]
```

**Step 3: 提交**

```bash
git add Cargo.toml
git commit -m "refactor: 重构 Cargo.toml 特性配置，添加预设组合"
```

---

### Task 2: 精简 Tokio 运行时依赖

**Files:**
- Modify: `Cargo.toml:58`
- Modify: `Cargo.toml:150`

**Step 1: 分析当前使用的 tokio 模块**

```bash
grep -r "use tokio::" src --include="*.rs" -h | sort | uniq
```

**识别的模块:**
- `tokio::net::TcpListener`
- `tokio::net::lookup_host`
- `tokio::fs`
- `tokio::io::AsyncWriteExt`
- `tokio::signal`
- `tokio::sync::Mutex`
- `tokio::sync::RwLock`
- `tokio::sync::Semaphore`
- `tokio::sync::OwnedSemaphorePermit`
- `tokio::task::JoinHandle`
- `tokio::time::sleep`
- `tokio::time::interval`
- `tokio::time::timeout`
- `#[tokio::main]`
- `#[tokio::test]`

**Step 2: 更新依赖配置**

```toml
[dependencies]
# 优化前
tokio = { version = "1.49", features = ["full"] }

# 优化后 - 仅包含实际使用的特性
tokio = { version = "1.49", features = [
    "rt-multi-thread",  # 多线程运行时（必须）
    "macros",           # #[tokio::main] 和 #[tokio::test]（必须）
    "net",              # TcpListener, lookup_host（必须）
    "fs",               # 异步文件系统操作（必须）
    "io-util",          # AsyncWriteExt（必须）
    "signal",           # 信号处理（必须）
    "sync",             # Mutex, RwLock, Semaphore（必须）
    "time",             # sleep, interval, timeout（必须）
] }
```

**Step 3: 更新 dev-dependencies**

```toml
[dev-dependencies]
tokio = { version = "1.49", features = [
    "rt-multi-thread",
    "macros",
    "net",
    "fs",
    "io-util",
    "signal",
    "sync",
    "time",
    "test-util",  # 测试工具，仅开发需要
] }
```

**Step 4: 编译验证**

```bash
cargo build --release --features default 2>&1 | tail -10
```

**Step 5: 提交**

```bash
git add Cargo.toml
git commit -m "refactor: 精简 tokio 运行时依赖，移除未使用的特性"
```

---

### Task 3: 移除 rustls 的 logging 特性

**Files:**
- Modify: `Cargo.toml:78`

**Step 1: 更新 rustls 配置**

```toml
# 优化前
rustls = { version = "0.23", default-features = false, features = ["ring", "logging", "std", "tls12"] }

# 优化后 - 移除 logging
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
```

**Step 2: 编译验证**

```bash
cargo build --release --features default 2>&1 | tail -5
```

**Step 3: 提交**

```bash
git add Cargo.toml
git commit -m "refactor: 移除 rustls 的 logging 特性以减少二进制体积"
```

---

### Task 4: 将 genai 移为可选依赖

**Files:**
- Modify: `Cargo.toml:142`
- Modify: `src/domain/services/llm_service.rs`
- Create: `src/domain/services/llm_service_disabled.rs`

**Step 1: 更新 Cargo.toml**

```toml
# 优化前
genai = "0.5.1"

# 优化后
genai = { version = "0.5.1", optional = true }
```

**Step 2: 创建禁用状态的占位类型**

```rust
// src/domain/services/llm_service_disabled.rs
use async_trait::async_trait;
use thiserror::Error;

/// AI 服务未启用时的禁用错误
#[derive(Debug, Error)]
pub enum GenAIDisabledError {
    #[error("AI 功能未在编译时启用，请使用 --features experimental 重新编译")]
    FeatureDisabled,
}

/// 禁用的 AI 服务实现（编译时未启用 genai）
#[derive(Debug, Clone)]
pub struct DisabledGenAIService;

impl DisabledGenAIService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
pub trait GenAIServiceTrait: Send + Sync {
    async fn generate_response(&self, prompt: &str) -> Result<String, GenAIDisabledError>;
    async fn analyze_content(&self, content: &str) -> Result<String, GenAIDisabledError>;
}

#[async_trait]
impl GenAIServiceTrait for DisabledGenAIService {
    async fn generate_response(&self, _prompt: &str) -> Result<String, GenAIDisabledError> {
        Err(GenAIDisabledError::FeatureDisabled)
    }

    async fn analyze_content(&self, _content: &str) -> Result<String, GenAIDisabledError> {
        Err(GenAIDisabledError::FeatureDisabled)
    }
}

pub type GenAIService = DisabledGenAIService;
```

**Step 3: 修改 llm_service.rs 使用条件编译**

```rust
// src/domain/services/llm_service.rs

#[cfg(feature = "genai")]
pub use self::genai_impl::GenAIService;

#[cfg(not(feature = "genai"))]
pub use self::disabled_impl::GenAIService;

#[cfg(feature = "genai")]
mod genai_impl {
    use super::*;
    // 保留原有的 genai 实现
}

#[cfg(not(feature = "genai"))]
mod disabled_impl {
    pub use super::llm_service_disabled::*;
}
```

**Step 4: 编译验证**

```bash
# 不启用 experimental 时应该编译成功
cargo build --release --features default 2>&1 | tail -5

# 启用 experimental 时应该编译成功
cargo build --release --features "default,experimental" 2>&1 | tail -5
```

**Step 5: 提交**

```bash
git add Cargo.toml src/domain/services/llm_service.rs src/domain/services/llm_service_disabled.rs
git commit -m "refactor: 将 genai 移为可选依赖，添加条件编译支持"
```

---

### Task 5: 将 S3 存储移为可选依赖

**Files:**
- Modify: `Cargo.toml:140-141`
- Modify: `src/infrastructure/storage.rs`

**Step 1: 更新 Cargo.toml**

```toml
# 优化前
aws-sdk-s3 = "1.120.0"
aws-config = "1.8.12"

# 优化后
aws-sdk-s3 = { version = "1.120.0", optional = true }
aws-config = { version = "1.8.12", optional = true }

# 添加 storage-s3 特性
storage-s3 = ["dep:aws-sdk-s3", "dep:aws-config"]
```

**Step 2: 修改 storage.rs 使用条件编译**

```rust
// src/infrastructure/storage.rs

#[cfg(feature = "storage-s3")]
pub use s3_storage::S3Storage;

#[cfg(not(feature = "storage-s3"))]
pub use s3_storage_disabled::DisabledS3Storage as S3Storage;

#[cfg(feature = "storage-s3")]
mod s3_storage {
    use aws_sdk_s3::Client;
    use aws_config::Region;
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum S3StorageError {
        #[error("S3 操作失败: {0}")]
        OperationFailed(String),
    }

    #[derive(Clone)]
    pub struct S3Storage {
        client: Client,
        bucket: String,
    }

    impl S3Storage {
        pub async fn new(config: &Config) -> Result<Self, S3StorageError> {
            // 保留原有实现
        }

        pub async fn upload(&self, key: &str, body: Vec<u8>) -> Result<(), S3StorageError> {
            // 保留原有实现
        }
    }
}

#[cfg(not(feature = "storage-s3"))]
mod s3_storage_disabled {
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum S3StorageDisabledError {
        #[error("S3 存储未在编译时启用，请使用 --features storage-s3 重新编译")]
        FeatureDisabled,
    }

    #[derive(Clone)]
    pub struct DisabledS3Storage;

    impl DisabledS3Storage {
        pub fn new() -> Self {
            Self
        }
    }
}
```

**Step 3: 编译验证**

```bash
cargo build --release --features default 2>&1 | tail -5
cargo build --release --features "default,storage-s3" 2>&1 | tail -5
```

**Step 4: 提交**

```bash
git add Cargo.toml src/infrastructure/storage.rs
git commit -m "refactor: 将 S3 存储移为可选依赖，添加条件编译支持"
```

---

### Task 6: 为 Playwright 引擎添加条件编译保护

**Files:**
- Modify: `src/engines/client/playwright.rs`
- Modify: `src/engines/mod.rs`

**Step 1: 修改 playwright.rs 添加条件编译**

```rust
// src/engines/client/playwright.rs

#[cfg(feature = "engine-playwright")]
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
#[cfg(feature = "engine-playwright")]
use chromiumoxide::{Browser, BrowserConfig};

// 如果未启用特性，创建禁用类型
#[cfg(not(feature = "engine-playwright"))]
pub struct PlaywrightBrowserManager;

#[cfg(not(feature = "engine-playwright"))]
impl PlaywrightBrowserManager {
    pub fn new() -> Self {
        Self
    }
}
```

**Step 2: 修改引擎注册逻辑**

```rust
// src/engines/mod.rs

#[cfg(feature = "engine-playwright")]
use crate::engines::client::playwright::PlaywrightEngine;

#[cfg(feature = "engine-playwright")]
const PLAYWRIGHT_AVAILABLE: bool = true;

#[cfg(not(feature = "engine-playwright"))]
const PLAYWRIGHT_AVAILABLE: bool = false;
```

**Step 3: 编译验证**

```bash
cargo build --release --features default 2>&1 | tail -5
cargo build --release --features "default,engine-playwright" 2>&1 | tail -5
```

**Step 4: 提交**

```bash
git add src/engines/client/playwright.rs src/engines/mod.rs
git commit -m "refactor: 为 Playwright 引擎添加条件编译保护"
```

---

### Task 7: 验证各预设配置的编译和功能

**Files:**
- Test: `Cargo.toml`
- Test: `src/`

**Step 1: 验证 lite 配置**

```bash
cargo build --release --features lite 2>&1 | tail -10
ls -lh target/release/crawlrs
```

**Step 2: 验证 standard 配置**

```bash
cargo build --release --features standard 2>&1 | tail -10
ls -lh target/release/crawlrs
```

**Step 3: 验证 full 配置**

```bash
cargo build --release --features full 2>&1 | tail -10
ls -lh target/release/crawlrs
```

**Step 4: 验证 experimental 配置**

```bash
cargo build --release --features "full,experimental" 2>&1 | tail -10
ls -lh target/release/crawlrs
```

**Step 5: 记录各配置的大小**

| 配置 | 预期大小 | 实际大小 |
|-----|---------|---------|
| lite | 25-30MB | |
| default | 35-40MB | |
| standard | 45-50MB | |
| full | 55-60MB | |
| experimental | 60-65MB | |

**Step 6: 提交**

```bash
git add .
git commit -m "test: 验证各预设配置的编译和二进制体积"
```

---

### Task 8: 运行测试确保功能正常

**Files:**
- Test: `tests/`

**Step 1: 运行 lite 配置的测试**

```bash
cargo test --features lite 2>&1 | tail -20
```

**Step 2: 运行 default 配置的测试**

```bash
cargo test --features default 2>&1 | tail -20
```

**Step 3: 运行 full 配置的测试**

```bash
cargo test --features full 2>&1 | tail -20
```

**Step 4: 提交**

```bash
git add .
git commit -m "test: 运行各配置测试确保功能正常"
```

---

### Task 9: 更新文档

**Files:**
- Modify: `README.md`

**Step 1: 添加特性配置说明**

```markdown
## 编译特性

本项目支持通过 Cargo 特性灵活控制编译功能和二进制体积。

### 预设配置

| 配置 | 特性组合 | 二进制大小 | 适用场景 |
|-----|---------|-----------|---------|
| lite | `engine-reqwest, db-sqlite` | ~25-30MB | 简单爬取，资源受限环境 |
| standard | `default + engine-playwright` | ~45-50MB | 默认使用，需要 JS 渲染 |
| full | 所有功能 | ~55-60MB | 生产环境，所有功能 |

### 自定义组合

```bash
# 轻量版 + S3 存储
cargo build --release --features "lite,storage-s3"

# 标准版 + AI 功能
cargo build --release --features "standard,experimental"

# 自定义组合
cargo build --release --features "engine-reqwest,db-sqlite,storage-s3,redis-cache"
```

### 特性参考

- `engine-reqwest`: HTTP 客户端引擎（基础，始终可用）
- `engine-playwright`: Playwright JS 渲染引擎（+20MB）
- `engine-fire-cdp`: Fire CDP 引擎（远程 FlareSolverr）
- `engine-fire-tls`: Fire TLS 引擎（远程 FlareSolverr）
- `storage-s3`: AWS S3 云存储
- `redis-cache`: Redis 缓存
- `experimental`: AI 功能（genai）
```

**Step 2: 提交**

```bash
git add README.md
git commit -m "docs: 添加编译特性配置说明"
```

---

## 预期效果

| 指标 | 优化前 | 优化后 | 改善 |
|-----|-------|-------|------|
| 默认版本体积 | 43MB | 35-40MB | -10% to -20% |
| Lite 版本体积 | - | 25-30MB | 新增 |
| Tokio 依赖 | full (14个特性) | 9个必需特性 | -35% |
| S3 存储 | 默认 | 可选 | 按需加载 |
| AI 功能 | 默认 | 可选 | 按需加载 |

---

## 执行选项

**Plan complete and saved to `docs/plans/2025-01-21-dependency-optimization.md`. Two execution options:**

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
