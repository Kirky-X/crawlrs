// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 环境变量覆盖示例
//!
//! 演示如何使用 confers 的 `EnvSource` 通过环境变量覆盖配置文件中的值。
//!
//! crawlrs 项目使用以下环境变量约定：
//! - 前缀：`CRAWLRS__`
//! - 嵌套分隔符：`__`
//! - 示例：`CRAWLRS__DATABASE__URL=postgresql://...` → `settings.database.url`
//!
//! # 使用方法
//!
//! ```bash
//! # 直接运行（使用默认值）
//! cargo run --example env_override
//!
//! # 通过环境变量覆盖
//! CRAWLRS__SERVER__PORT=9999 cargo run --example env_override
//! ```
//!
//! # 前置条件
//!
//! - 主项目根目录的 `config/default.toml` 文件

use confers::{Config, ConfigBuilder, EnvSource};
use log::info;
use serde::{Deserialize, Serialize};

/// 演示用配置结构（与 crawlrs::config::Settings 的子集对应）
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__")]
struct DemoSettings {
    /// 服务器配置
    server: ServerConfig,

    /// 数据库配置
    database: DatabaseConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct ServerConfig {
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct DatabaseConfig {
    #[serde(default)]
    url: String,
    #[serde(default = "default_max_conn")]
    max_connections: u32,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8899
}

fn default_max_conn() -> u32 {
    100
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 confers 环境变量覆盖示例");
    info!("=====================================\n");

    // 1. 环境变量命名规则
    info!("1️⃣  环境变量命名规则");
    info!("-----------------------------");
    info!("📖 crawlrs 使用 `CRAWLRS__` 作为前缀，`__` 作为嵌套分隔符：");
    info!("");
    info!("   配置路径            → 环境变量");
    info!("   ─────────────────────────────────────────");
    info!("   server.host         → CRAWLRS__SERVER__HOST");
    info!("   server.port         → CRAWLRS__SERVER__PORT");
    info!("   database.url        → CRAWLRS__DATABASE__URL");
    info!("   database.max_connections → CRAWLRS__DATABASE__MAX_CONNECTIONS");
    info!("");

    // 2. EnvSource 用法
    info!("2️⃣  EnvSource 配置");
    info!("-----------------------------");
    info!("📖 通过 EnvSource::with_prefix + separator 构造环境源");
    info!("   EnvSource 会自动扫描匹配前缀的环境变量并解析嵌套键");
    info!("");
    info!("📌 代码示例：");
    info!("   let env = EnvSource::with_prefix(\"CRAWLRS__\")");
    info!("       .separator(\"__\");");
    info!("   let settings = ConfigBuilder::<Settings>::new()");
    info!("       .file(\"config/default.toml\")");
    info!("       .source(Box::new(env))");
    info!("       .build()?;");
    info!("");

    // 3. 当前环境变量检测
    info!("3️⃣  检测当前环境中的 CRAWLRS__ 变量");
    info!("-----------------------------");
    let crawlrs_env_vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("CRAWLRS__"))
        .collect();
    if crawlrs_env_vars.is_empty() {
        info!("ℹ️  当前未检测到 CRAWLRS__ 前缀的环境变量");
    } else {
        info!(
            "✅ 检测到 {} 个 CRAWLRS__ 环境变量：",
            crawlrs_env_vars.len()
        );
        for (k, v) in &crawlrs_env_vars {
            let masked = if k.contains("URL") || k.contains("KEY") || k.contains("SECRET") {
                mask_sensitive(v)
            } else {
                v.clone()
            };
            info!("   {} = {}", k, masked);
        }
    }
    info!("");

    // 4. 覆盖示例
    info!("4️⃣  覆盖示例演示");
    info!("-----------------------------");
    info!("📌 在终端中尝试以下命令观察覆盖效果：");
    info!("");
    info!("   # 覆盖服务器端口");
    info!("   CRAWLRS__SERVER__PORT=9999 cargo run --example env_override");
    info!("");
    info!("   # 覆盖数据库 URL（注意特殊字符需引号）");
    info!("   CRAWLRS__DATABASE__URL='postgresql://user:pass@host/db' \\");
    info!("       cargo run --example env_override");
    info!("");
    info!("   # 同时覆盖多个值");
    info!("   CRAWLRS__SERVER__HOST=127.0.0.1 \\");
    info!("   CRAWLRS__SERVER__PORT=7777 \\");
    info!("   CRAWLRS__DATABASE__MAX_CONNECTIONS=50 \\");
    info!("       cargo run --example env_override");
    info!("");

    // 5. 加载主项目配置（如果存在）
    info!("5️⃣  尝试加载主项目配置");
    info!("-----------------------------");
    info!("📖 尝试从 ../config/default.toml 加载 crawlrs 主项目配置");
    info!("");
    match try_load_main_config() {
        Ok(settings) => {
            info!("✅ 主项目配置加载成功：");
            info!("   server.host: {}", settings.server.host);
            info!("   server.port: {}", settings.server.port);
            info!(
                "   database.url: {}",
                mask_sensitive(&settings.database.url)
            );
            info!(
                "   database.max_connections: {}",
                settings.database.max_connections
            );
        }
        Err(e) => {
            info!("ℹ️  加载主项目配置失败：{}", e);
            info!("   请确保在 crawlrs 主项目根目录或 examples 目录下运行");
        }
    }

    info!("\n=====================================");
    info!("✨ 环境变量覆盖示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 环境变量优先级高于配置文件");
    info!("   - 敏感信息（API Key、密码）推荐通过环境变量注入");
    info!("   - Docker/K8s 部署时使用 _FILE 后缀读取密钥文件");
}

/// 尝试加载主项目配置
fn try_load_main_config() -> Result<DemoSettings, confers::ConfigError> {
    ConfigBuilder::<DemoSettings>::new()
        .file("../config/default.toml")
        .source(Box::new(
            EnvSource::with_prefix("CRAWLRS__").separator("__"),
        ))
        .build()
}

/// 对敏感值进行掩码处理（仅显示前缀和长度）
fn mask_sensitive(value: &str) -> String {
    if value.is_empty() {
        return "(empty)".to_string();
    }
    if value.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...({} chars)", &value[..4], value.len())
}
