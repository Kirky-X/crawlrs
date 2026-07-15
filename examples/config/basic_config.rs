// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置加载基础示例
//!
//! 演示如何使用 confers 0.4 加载 TOML 配置文件并定义类型安全的配置结构。
//!
//! confers 0.4 的关键变化（相对于 0.2.x）：
//! - `ConfigBuilder` 不再自动发现配置文件，需通过 `.file()` 显式指定
//! - `#[derive(Config)]` 宏生成 `load_sync()` 与 `load()` 方法
//! - `EnvSource::with_prefix()` 支持嵌套分隔符
//!
//! 本示例展示 API 用法；如需实际加载，可在 examples 目录下放置 config.toml 文件。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_config
//! ```

use confers::{Config, ConfigBuilder, EnvSource};
use log::info;
use serde::{Deserialize, Serialize};

/// 示例配置结构
///
/// 通过 `#[derive(Config)]` 让 confers 自动生成 `load_sync()` / `load()` 方法。
/// `env_prefix` 指定环境变量前缀（与 EnvSource 配合使用）。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "APP_")]
struct AppSettings {
    /// 服务监听地址
    #[serde(default = "default_host")]
    host: String,

    /// 服务监听端口
    #[serde(default = "default_port")]
    port: u16,

    /// 数据库连接 URL
    #[serde(default)]
    database_url: String,

    /// 日志级别
    #[serde(default = "default_log_level")]
    log_level: String,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_log_level() -> String {
    "info".to_string()
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 confers 配置加载示例");
    info!("=====================================\n");

    // 1. 配置结构说明
    info!("1️⃣  定义配置结构");
    info!("-----------------------------");
    info!("📖 通过 #[derive(Config)] 派生 confers 的加载能力");
    info!("   字段使用 #[serde(default = ...)] 提供缺失时的默认值");
    info!("");
    let sample = AppSettings::default();
    info!("✅ 默认配置：");
    info!("   host: {}", sample.host);
    info!("   port: {}", sample.port);
    info!("   database_url: {:?}", sample.database_url);
    info!("   log_level: {}", sample.log_level);
    info!("");

    // 2. ConfigBuilder 用法
    info!("2️⃣  使用 ConfigBuilder 加载配置");
    info!("-----------------------------");
    info!("📖 ConfigBuilder 提供流式 API，按以下顺序构建配置源：");
    info!("   1. .file(path)      : 添加 TOML/YAML/JSON 配置文件");
    info!("   2. .source(...)     : 添加自定义源（如 EnvSource）");
    info!("   3. .build()         : 合并所有源并返回类型安全的配置");
    info!("");
    info!("📌 加载示例（需存在 config.toml 文件）：");
    info!("   let settings = ConfigBuilder::<AppSettings>::new()");
    info!("       .file(\"config.toml\")");
    info!("       .source(Box::new(EnvSource::with_prefix(\"APP_\").separator(\"_\")))");
    info!("       .build()?;");
    info!("");

    // 3. 多源合并说明
    info!("3️⃣  多源合并策略");
    info!("-----------------------------");
    info!("📖 confers 按添加顺序应用配置源，后添加的源优先级更高：");
    info!("   - 文件源提供基础配置");
    info!("   - 环境变量源覆盖文件配置");
    info!("   - 内存源（可选）用于运行时覆盖");
    info!("");
    info!("📌 优先级示例：");
    info!("   config.toml 中 port=8080");
    info!("   环境变量 APP_PORT=9090");
    info!("   最终加载的 port=9090（环境变量覆盖文件配置）");
    info!("");

    // 4. 实际加载演示
    info!("4️⃣  实际加载演示");
    info!("-----------------------------");
    info!("📖 尝试加载当前目录的 config.toml（不存在时使用默认值）");
    info!("");

    match try_load_settings() {
        Ok(settings) => {
            info!("✅ 配置加载成功：");
            info!("   host: {}", settings.host);
            info!("   port: {}", settings.port);
            info!("   database_url: {}", settings.database_url);
            info!("   log_level: {}", settings.log_level);
        }
        Err(e) => {
            info!("ℹ️  配置加载失败（预期，未提供 config.toml）：{}", e);
            info!("   这说明 confers 在文件缺失时会返回错误");
            info!("   生产环境应保证基础配置文件存在");
        }
    }

    info!("\n=====================================");
    info!("✨ confers 配置加载示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - crawlrs 主项目在 src/bootstrap/config.rs 中加载 Settings");
    info!("   - 默认配置文件路径为 config/default.toml");
    info!("   - 通过 EnvSource 支持 CRAWLRS__ 前缀的环境变量覆盖");
}

/// 尝试从 config.toml 加载配置
///
/// 在 examples 目录下运行时，通常不存在 config.toml，
/// 此时会返回错误，演示 confers 的错误处理。
fn try_load_settings() -> Result<AppSettings, confers::ConfigError> {
    ConfigBuilder::<AppSettings>::new()
        .file("config.toml")
        .source(Box::new(EnvSource::with_prefix("APP_").separator("_")))
        .build()
}
