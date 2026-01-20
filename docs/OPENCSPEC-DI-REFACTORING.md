# OpenSpec: 依赖注入架构修复方案

## 📋 概述

本文档描述 crawlrs 项目中依赖注入绕过问题的系统性修复方案。

**问题分类**:
- 🔴 高优先级: 全局静态变量、HTTP客户端直接实例化
- 🟡 中优先级: 环境变量直接访问

**迁移状态**: ✅ Phase 1-3 已完成 | 🔄 Phase 4-6 进行中

---

## 迁移完成状态

### ✅ Phase 1: ConfigService 扩展 (已完成)
- ✅ 扩展 `ConfigServiceTrait` 接口，添加 11 个新方法
- ✅ 实现所有新方法，支持从环境变量读取配置

### ✅ Phase 2: 环境变量访问统一 (已完成)
- ✅ 统一环境变量获取逻辑
- ✅ 保留必要的底层配置和调试功能
- ✅ 修改文件: `bootstrap/config.rs`, `config/settings.rs`, `infrastructure/database/connection.rs`, `presentation/middleware/auth_middleware.rs`, `engines/validators.rs`, `engines/enhanced_validators.rs`, `engines/health_monitor.rs`, `search/client/google.rs`, `di/service_module.rs`

### ✅ Phase 3: DI 组件 Default 实现 (已完成)
- ✅ 更新所有 Service 组件的 Default 实现
- ✅ 使用统一的环境变量获取逻辑
- ✅ 修复测试代码问题

### 🔄 Phase 4: 共享集合工厂化 (进行中，可选)
- ⏳ 创建 `CollectionFactoryTrait` (可选)
- ⏳ 更新 `TeamSemaphoreComponent` (可选)
- ⏳ 更新 `CacheStrategy` (可选)

### ⏳ Phase 5: 测试代码 DI 化 (待完成)
- ⏳ 创建测试辅助模块
- ⏳ 添加 Mock 组件

### 🔄 Phase 6: 验证和文档 (进行中)
- ✅ 运行完整测试套件 (263 个测试全部通过)
- ✅ 检查 DI 违规
- ✅ 代码审查通过
- ⏳ 性能基准测试
- 🔄 更新本文档

---

## 1. 全局静态变量修复

### 1.1 playwright.rs - 浏览器实例管理

**当前问题**:
```rust
static BROWSER_INSTANCE: OnceLock<Arc<Mutex<Option<Arc<Browser>>>>> = OnceLock::new();
```

**修复方案**:
```rust
/// 浏览器实例管理 trait（支持 DI）
#[async_trait]
pub trait BrowserManagerTrait: Send + Sync {
    async fn get_browser(&self) -> Result<Arc<Browser>, EngineError>;
    async fn cleanup(&self);
    fn reset(&self);
}

/// Playwright 浏览器管理器（生产实现）
#[derive(Component)]
#[shaku(interface = BrowserManagerTrait)]
pub struct PlaywrightBrowserManager {
    #[shaku(inject)]
    config: Arc<dyn BrowserConfig>,
    browser: Arc<Mutex<Option<Arc<Browser>>>>,
}

impl PlaywrightBrowserManager {
    pub fn new(config: Arc<dyn BrowserConfig>) -> Self {
        Self {
            config,
            browser: Arc::new(Mutex::new(None)),
        }
    }
}
```

### 1.2 regex_cache.rs - 正则表达式缓存

**当前问题**:
```rust
static INSTANCE: Lazy<RegexCache> = Lazy::new(RegexCache::new);
```

**修复方案**:
```rust
/// 正则缓存 trait（支持 DI）
pub trait RegexCacheTrait: Send + Sync {
    fn get_or_insert(&self, pattern: &str) -> Result<Regex, String>;
    fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String>;
    fn clear(&self);
}

/// 正则缓存组件
#[derive(Component)]
#[shaku(interface = RegexCacheTrait)]
pub struct RegexCacheComponent {
    cache: Arc<Mutex<HashMap<String, Regex>>>,
}

impl RegexCacheTrait for RegexCacheComponent { ... }
```

### 1.3 relevance_scorer.rs - 日期正则表达式

**当前问题**:
```rust
static DATE_REGEXES: Lazy<Vec<(Regex, DateParser)>> = Lazy::new(|| { ... });
```

**修复方案**:
```rust
/// 日期解析器 trait
pub trait DateParserTrait: Send + Sync {
    fn extract_date(&self, text: &str) -> Option<DateTime<Utc>>;
}

/// 日期解析器组件（可注入不同实现）
#[derive(Component)]
#[shaku(interface = DateParserTrait)]
pub struct DateParserComponent {
    date_regexes: Vec<(Regex, fn(&str) -> Option<DateTime<Utc>>)>,
}
```

### 1.4 processor.rs - 网页内容处理器

**当前问题**:
```rust
static INSTANCE: once_cell::sync::Lazy<WebContentProcessor> = ...;
```

**修复方案**:
```rust
/// 内容处理器 trait
#[async_trait]
pub trait ContentProcessorTrait: Send + Sync {
    fn process_web_content(&self, content: &[u8], content_type: Option<&str>) -> Result<ProcessedWebContent, WebContentError>;
}

/// 内容处理器组件
#[derive(Component)]
#[shaku(interface = ContentProcessorTrait)]
pub struct WebContentProcessorComponent {
    #[shaku(inject)]
    text_processor: Arc<dyn TextEncodingProcessorTrait>,
}
```

### 1.5 metrics.rs - 系统指标

**当前问题**:
```rust
static CURRENT_CPU_USAGE: AtomicU64 = AtomicU64::new(0);
static CURRENT_MEMORY_USAGE: AtomicU64 = AtomicU64::new(0);
static LAST_UPDATE_TIME: AtomicU64 = AtomicU64::new(0);
```

**修复方案**:
```rust
/// 系统监控 trait
pub trait SystemMonitorTrait: Send + Sync {
    fn cpu_usage(&self) -> f64;
    fn memory_usage(&self) -> f64;
    fn is_metrics_stale(&self) -> bool;
}

/// 系统监控组件
#[derive(Component)]
#[shaku(interface = SystemMonitorTrait)]
pub struct SystemMonitorComponent {
    system: Arc<Mutex<System>>,
    last_update: Arc<AtomicU64>,
    cpu_usage: Arc<AtomicU64>,
    memory_usage: Arc<AtomicU64>,
}
```

---

## 2. 环境变量访问统一

### 2.1 配置接口

```rust
/// 配置服务 trait（统一环境变量访问）
#[async_trait]
pub trait ConfigServiceTrait: Send + Sync {
    fn get_proxy_url(&self) -> Option<String>;
    fn get_remote_debugging_url(&self) -> Option<String>;
    fn is_test_mode(&self) -> bool;
    fn get_timeout(&self) -> Duration;
}

/// 配置组件
#[derive(Component)]
#[shaku(interface = ConfigServiceTrait)]
pub struct ConfigServiceComponent {
    settings: Arc<Settings>,
}
```

### 2.2 迁移点

| 文件 | 迁移前 | 迁移后 |
|------|--------|--------|
| `factory.rs:176` | `std::env::var("CRAWLRS_PROXY_URL")` | `config.get_proxy_url()` |
| `playwright.rs:67` | `std::env::var("CRAWLRS_TEST_NO_BROWSER_REUSE")` | `config.is_test_mode()` |
| `playwright.rs:93` | `std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL")` | `config.get_remote_debugging_url()` |
| `playwright.rs:104` | `std::env::var("CRAWLRS_PROXY_URL")` | `config.get_proxy_url()` |

---

## 3. HTTP 客户端工厂 DI 化

### 3.1 HTTP 客户端组件

```rust
/// HTTP 客户端工厂 trait
#[async_trait]
pub trait HttpClientFactoryTrait: Send + Sync {
    fn create_client(&self) -> Arc<reqwest::Client>;
    fn create_client_with_timeout(&self, timeout_secs: u64) -> Arc<reqwest::Client>;
}

/// HTTP 客户端工厂组件
#[derive(Component)]
#[shaku(interface = HttpClientFactoryTrait)]
pub struct HttpClientFactoryComponent {
    config: Arc<reqwest::Client>,
}

impl HttpClientFactoryTrait for HttpClientFactoryComponent {
    fn create_client(&self) -> Arc<reqwest::Client> {
        self.config.clone()
    }
    // ...
}
```

### 3.2 迁移搜索工厂

```rust
/// 搜索引擎工厂 trait
#[async_trait]
pub trait SearchEngineFactoryTrait: Send + Sync {
    async fn create_all_engines(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn get_engine(&self, engine_type: SearchEngineType) -> Option<Arc<dyn SearchEngine>>;
}

/// 搜索引擎工厂组件
#[derive(Component)]
#[shaku(interface = SearchEngineFactoryTrait)]
pub struct SearchEngineFactoryComponent {
    #[shaku(inject)]
    http_client_factory: Arc<dyn HttpClientFactoryTrait>,
    #[shaku(inject)]
    config_service: Arc<dyn ConfigServiceTrait>,
}
```

---

## 4. DI 模块集成

### 4.1 新增组件到 infrastructure_module.rs

```rust
shaku::module! {
    pub InfrastructureModule {
        components = [
            // ... 现有组件 ...
            // 新增:
            BrowserManagerComponent,
            ConfigServiceComponent,
            HttpClientFactoryComponent,
        ],
        providers = []
    }
}
```

### 4.2 新增组件到 service_module.rs

```rust
shaku::module! {
    pub ServiceModule {
        components = [
            // ... 现有组件 ...
            // 新增:
            RegexCacheComponent,
            DateParserComponent,
            ContentProcessorComponent,
            SystemMonitorComponent,
            SearchEngineFactoryComponent,
        ],
        providers = []
    }
}
```

---

## 5. 测试友好性改进

### 5.1 Mock 实现示例

```rust
#[cfg(test)]
pub struct MockBrowserManager {
    browser: Arc<Mutex<Option<Arc<MockBrowser>>>>,
}

#[cfg(test)]
impl BrowserManagerTrait for MockBrowserManager {
    async fn get_browser(&self) -> Result<Arc<Browser>, EngineError> {
        Ok(self.browser.lock().unwrap().clone().unwrap())
    }
    // ...
}
```

### 5.2 测试容器配置

```rust
#[cfg(test)]
fn create_test_container() -> Container<impl Module> {
    Container::builder()
        .with_component_parameters::<BrowserManagerComponent>(
            BrowserManagerComponent::from_parameters(MockBrowserManager::new())
        )
        // ...
        .build()
}
```

---

## 6. 实施顺序

1. **Phase 1**: 配置服务抽象（低风险，高价值）
   - 创建 `ConfigServiceTrait`
   - 迁移环境变量访问

2. **Phase 2**: HTTP 客户端工厂 DI 化
   - 创建 `HttpClientFactoryTrait`
   - 修改搜索工厂使用 DI

3. **Phase 3**: 全局状态组件化
   - `RegexCacheComponent`
   - `SystemMonitorComponent`
   - `ContentProcessorComponent`

4. **Phase 4**: 浏览器实例管理
   - `BrowserManagerTrait`
   - 替换全局 `BROWSER_INSTANCE`

5. **Phase 5**: 测试验证
   - 添加 mock 实现
   - 验证测试隔离性

---

## 7. 兼容性注意事项

- 保持原有 `global()` 方法用于向后兼容
- 标记为 `#[deprecated`，引导使用 DI
- 新增 `#[cfg(test)]` mock 实现
- 确保现有代码可逐步迁移

---

## 8. 最佳实践指南

### 8.1 配置访问

**✅ 推荐的模式**:
```rust
// 通过 ConfigServiceTrait 访问配置
fn get_redis_url(config: &dyn ConfigServiceTrait) -> String {
    config.get_redis_url()
}
```

**❌ 避免的模式**:
```rust
// 直接读取环境变量（在业务代码中）
let redis_url = std::env::var("REDIS_URL").unwrap();
```

**⚠️ 允许的模式**:
```rust
// 在 ConfigService 实现中读取环境变量（合理）
let redis_url = std::env::var("REDIS_URL")
    .unwrap_or_else(|_| "redis://localhost:6379".to_string());
```

### 8.2 DI 组件设计

**✅ 推荐的模式**:
```rust
// 使用 Shaku Component 注解
#[derive(Component)]
#[shaku(interface = MyServiceTrait)]
pub struct MyServiceComponent {
    #[shaku(inject)]
    config: Arc<dyn ConfigServiceTrait>,
    #[shaku(inject)]
    repository: Arc<dyn MyRepositoryTrait>,
}
```

**❌ 避免的模式**:
```rust
// 直接实例化依赖
pub struct MyService {
    redis_client: Arc<RedisClient>, // 在外部创建后传入
}

// impl Default 直接创建依赖
impl Default for MyService {
    fn default() -> Self {
        Self {
            redis_client: Arc::new(RedisClient::new("redis://localhost").unwrap()),
        }
    }
}
```

### 8.3 测试中的使用

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // 测试配置服务 mock
    #[derive(Clone)]
    struct MockConfigService {
        redis_url: String,
    }

    impl ConfigServiceTrait for MockConfigService {
        fn get_redis_url(&self) -> String {
            self.redis_url.clone()
        }
        // ... 其他方法
    }

    #[test]
    fn test_something() {
        let config = MockConfigService {
            redis_url: "redis://localhost".to_string(),
        };
        // 使用 mock 进行测试
    }
}
```

### 8.4 环境变量命名规范

- **应用级配置**: `CRAWLRS_*` (e.g., `CRAWLRS_ENV`, `CRAWLRS_PROXY_URL`)
- **服务级配置**: `APP_*` (e.g., `APP_ENVIRONMENT`)
- **生产环境禁止**: 避免在生产环境使用 `TEST_*` 环境变量

### 8.5 检查清单

添加新代码时，检查以下项目:

- [ ] 是否通过 `ConfigServiceTrait` 访问配置？
- [ ] 是否通过注入获取依赖，而非直接实例化？
- [ ] 是否有 `Default` 实现直接创建依赖？
- [ ] 是否有遗漏的环境变量直接访问？
- [ ] 测试是否隔离？

---

## 9. 常见问题

### Q1: 为什么保留 `std::env::var` 在 ConfigService 中？

**A**: ConfigService 是配置的单一数据源。所有环境变量读取集中在这一处，便于：
- 配置验证
- 默认值管理
- 测试时 mock

### Q2: 何时使用 `Arc::new()` 直接创建？

**A**: 以下情况是允许的：
- 创建无状态或轻量级对象 (e.g., `Duration`, `Regex`)
- 在 `Default` 实现中创建合理默认值
- 创建临时集合 (`Vec::new()`, `HashMap::new()`)

### Q3: 如何处理跨模块共享状态？

**A**: 建议通过 DI 注入共享状态：
```rust
#[derive(Component)]
pub struct SharedStateComponent {
    #[shaku(inject)]
    cache: Arc<DashMap<String, String>>,
}
```

### Q4: 测试时如何替换 DI 组件？

**A**: 使用 Shaku 的 `with_component_parameters`:
```rust
let module = TestModule::builder()
    .with_component_parameters::<ConfigServiceComponent>(
        ConfigServiceComponent::from_settings(/* test values */)
    )
    .build();
```

---

## 10. 验证命令

```bash
# 运行所有测试
cargo test --lib

# 检查环境变量使用
rg "std::env::var" --type rust src/ | grep -v "test" | grep -v "config_service" | grep -v "security"

# 检查 DI 违规
rg "Arc::new.*::new\(\)" --type rust src/ | grep -v "test"
```

**预期结果**:
- 所有测试通过 (263 tests)
- 环境变量使用仅限 ConfigService 和安全检查
- DI 违规仅限合理的默认值创建

