// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置验证示例
//!
//! 演示如何在 confers 配置结构中添加验证规则，并在加载时自动校验。
//!
//! confers 0.4 集成了 `validator` crate，可通过：
//! - `#[config(validate)]` 在结构体级别启用自动验证
//! - `#[validate(...)]` 在字段级别添加验证规则（范围、长度、正则等）
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example validation
//! ```

use confers::{Config, ConfigBuilder};
use log::info;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// 带验证规则的配置结构
///
/// `#[config(validate)]` 让 confers 在 `build()` 时自动调用 `Validate::validate()`。
#[derive(Debug, Clone, Deserialize, Serialize, Config, Validate)]
#[config(env_prefix = "APP_", validate)]
struct ValidatedSettings {
    /// 服务器端口（1-65535）
    #[validate(range(min = 1, max = 65535))]
    port: u32,

    /// 主机地址（不能为空）
    #[validate(length(min = 1, max = 255))]
    host: String,

    /// 数据库连接 URL（必须以 postgresql:// 开头）
    #[validate(url)]
    database_url: String,

    /// 日志级别（必须是预定义值之一）
    #[validate(custom(function = "validate_log_level"))]
    log_level: String,

    /// 最大连接数（1-1000）
    #[validate(range(min = 1, max = 1000))]
    max_connections: u32,
}

/// 自定义验证函数：检查日志级别是否合法
fn validate_log_level(level: &str) -> Result<(), validator::ValidationError> {
    let valid_levels = ["trace", "debug", "info", "warn", "error"];
    if valid_levels.contains(&level.to_lowercase().as_str()) {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_log_level")
            .with_message(format!(
                "log_level must be one of {:?}, got '{}'",
                valid_levels, level
            )
            .into()))
    }
}

/// 验证通过的示例配置
fn valid_sample() -> ValidatedSettings {
    ValidatedSettings {
        port: 8080,
        host: "0.0.0.0".to_string(),
        database_url: "postgresql://user:pass@localhost/db".to_string(),
        log_level: "info".to_string(),
        max_connections: 100,
    }
}

/// 验证失败的示例配置（端口越界）
fn invalid_port_sample() -> ValidatedSettings {
    ValidatedSettings {
        port: 70000, // 越界（u16 上限是 65535，但 validator 仍会检查 range）
        host: "0.0.0.0".to_string(),
        database_url: "postgresql://user:pass@localhost/db".to_string(),
        log_level: "info".to_string(),
        max_connections: 100,
    }
}

/// 验证失败的示例配置（非法日志级别）
fn invalid_log_level_sample() -> ValidatedSettings {
    ValidatedSettings {
        port: 8080,
        host: "0.0.0.0".to_string(),
        database_url: "postgresql://user:pass@localhost/db".to_string(),
        log_level: "verbose".to_string(), // 非法值
        max_connections: 100,
    }
}

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 confers 配置验证示例");
    info!("=====================================\n");

    // 1. 验证机制说明
    info!("1️⃣  confers 验证机制");
    info!("-----------------------------");
    info!("📖 confers 0.4 集成 validator crate，提供两层验证：");
    info!("   1. 结构体级：`#[config(validate)]` 在 build() 时自动调用 validate()");
    info!("   2. 字段级：`#[validate(...)]` 声明字段的验证规则");
    info!("");
    info!("支持的验证规则：");
    info!("   - range(min, max)      : 数值范围");
    info!("   - length(min, max)     : 字符串长度");
    info!("   - url                   : URL 格式");
    info!("   - email                 : 邮箱格式");
    info!("   - custom(function)      : 自定义验证函数");
    info!("   - does_not_contain(...) : 排除特定子串");
    info!("");

    // 2. 验证成功示例
    info!("2️⃣  验证成功示例");
    info!("-----------------------------");
    let valid = valid_sample();
    info!("📖 验证合法配置：port={}, log_level={}", valid.port, valid.log_level);
    match valid.validate() {
        Ok(()) => info!("✅ 验证通过"),
        Err(e) => info!("❌ 验证失败：{:?}", e),
    }
    info!("");

    // 3. 验证失败示例（端口越界）
    info!("3️⃣  验证失败示例（端口越界）");
    info!("-----------------------------");
    let invalid_port = invalid_port_sample();
    info!("📖 验证非法配置：port={}（应 ≤ 65535）", invalid_port.port);
    match invalid_port.validate() {
        Ok(()) => info!("⚠️  验证通过（不应发生）"),
        Err(e) => {
            info!("✅ 验证按预期失败");
            info!("   错误：{}", format_validation_errors(&e));
        }
    }
    info!("");

    // 4. 验证失败示例（非法日志级别）
    info!("4️⃣  验证失败示例（自定义规则）");
    info!("-----------------------------");
    let invalid_level = invalid_log_level_sample();
    info!("📖 验证非法配置：log_level='{}'（应为 trace/debug/info/warn/error）", invalid_level.log_level);
    match invalid_level.validate() {
        Ok(()) => info!("⚠️  验证通过（不应发生）"),
        Err(e) => {
            info!("✅ 验证按预期失败");
            info!("   错误：{}", format_validation_errors(&e));
        }
    }
    info!("");

    // 5. ConfigBuilder 自动验证
    info!("5️⃣  ConfigBuilder 自动验证");
    info!("-----------------------------");
    info!("📖 启用 `#[config(validate)]` 后，build() 会自动执行验证");
    info!("   验证失败时返回 ConfigError，包含详细的字段错误信息");
    info!("");
    info!("📌 代码示例：");
    info!("   let settings = ConfigBuilder::<ValidatedSettings>::new()");
    info!("       .file(\"config.toml\")");
    info!("       .build()?;  // 验证失败时返回 Err");
    info!("");
    info!("📌 错误处理示例：");
    info!("   match ConfigBuilder::<ValidatedSettings>::new().build() {{");
    info!("       Ok(s) => info!(\"配置有效: {{:?}}\", s),");
    info!("       Err(e) => {{");
    info!("           error!(\"配置验证失败: {{}}\", e);");
    info!("           std::process::exit(1);");
    info!("       }}");
    info!("   }}");
    info!("");

    // 6. 验证最佳实践
    info!("6️⃣  验证最佳实践");
    info!("-----------------------------");
    info!("💡 推荐做法：");
    info!("   - 所有外部输入（端口、URL、连接数）都加验证规则");
    info!("   - 敏感字段使用 does_not_contain 防止日志泄露");
    info!("   - 自定义验证函数处理业务规则（如枚举值）");
    info!("   - 生产环境启用 `#[config(validate)]`，确保启动时配置合法");
    info!("   - crawlrs 主项目在 src/bootstrap/config.rs 中执行额外安全检查");

    info!("\n=====================================");
    info!("✨ confers 配置验证示例完成");
}

/// 格式化验证错误为可读字符串
fn format_validation_errors(e: &validator::ValidationErrors) -> String {
    let mut parts = Vec::new();
    for (field, errors) in e.field_errors() {
        for err in errors {
            let msg = err
                .message
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or_else(|| err.code.to_string());
            parts.push(format!("{}: {}", field, msg));
        }
    }
    parts.join("; ")
}

/// 占位：保证 ConfigBuilder 类型被引用（用于演示自动验证的入口）
#[allow(dead_code)]
fn _build_entry(settings: &ValidatedSettings) {
    // ConfigBuilder 在 build() 时会调用 settings.validate()
    let _ = settings.validate();
    let _ = ConfigBuilder::<ValidatedSettings>::new();
}
