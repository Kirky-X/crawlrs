# Config Examples

配置管理示例，演示如何使用 confers 0.4 进行配置加载、环境变量覆盖和验证。

## 包含的示例

| 示例文件 | 功能描述 |
|---------|---------|
| `basic_config.rs` | 基础配置加载（ConfigBuilder + TOML 文件） |
| `env_override.rs` | 环境变量覆盖（EnvSource + 嵌套配置） |
| `validation.rs` | 配置验证（validator crate 集成） |

## 核心功能

### 配置加载
- `#[derive(Config)]` 自动生成 `Default` 实现和 `load_sync()` / `load()` 方法
- `ConfigBuilder::<T>::new().file(path).build()` 链式构建配置
- `#[config(env_prefix = "APP_")]` 声明环境变量前缀

### 环境变量覆盖
- `EnvSource::with_prefix("CRAWLRS__").separator("__")` 嵌套配置覆盖
- 配置优先级：环境变量 > 配置文件 > 默认值
- 敏感字段脱敏（`mask_sensitive()` 工具函数）

### 配置验证
- `#[config(validate)]` 在 `build()` 时自动调用 `Validate::validate()`
- `#[validate(range(min, max))]` 数值范围验证
- `#[validate(length(min, max))]` 字符串长度验证
- `#[validate(url)]` / `#[validate(email)]` 格式验证
- `#[validate(custom(function = "..."))]` 自定义验证函数

## 快速开始

```rust
use confers::{Config, ConfigBuilder};

#[derive(Config)]
#[config(env_prefix = "APP_")]
struct AppSettings {
    host: String,
    port: u16,
}

let settings = ConfigBuilder::<AppSettings>::new()
    .file("config.toml")
    .build()?;
```

## 前置条件

- confers 启用特性：`toml`、`env`、`validation`

## 相关示例

- 数据库示例：`../database/`
- 认证示例：`../auth/`
- 基础架构示例：`../rate-limiting/`
