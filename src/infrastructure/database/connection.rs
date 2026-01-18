// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::DatabaseSettings;
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::time::Duration;

/// 创建数据库连接池
///
/// # 参数
///
/// * `settings` - 数据库配置
///
/// # 返回值
///
/// * `Ok(DatabaseConnection)` - 数据库连接
/// * `Err(DbErr)` - 连接过程中出现的错误
pub async fn create_pool(settings: &DatabaseSettings) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(settings.url.to_owned());

    if let Some(max) = settings.max_connections {
        opt.max_connections(max);
    }

    if let Some(min) = settings.min_connections {
        opt.min_connections(min);
    }

    if let Some(timeout) = settings.connect_timeout {
        opt.connect_timeout(Duration::from_secs(timeout));
        opt.acquire_timeout(Duration::from_secs(timeout));
    }

    if let Some(idle) = settings.idle_timeout {
        opt.idle_timeout(Duration::from_secs(idle));
    }

    opt.max_lifetime(Duration::from_secs(3600));

    // 根据环境决定是否启用SQL日志，生产环境禁用以防止敏感信息泄露
    let is_production = std::env::var("CRAWLRS_ENV")
        .map(|v| v.eq_ignore_ascii_case("production") || v.eq_ignore_ascii_case("prod"))
        .unwrap_or(false);
    opt.sqlx_logging(!is_production);

    Database::connect(opt).await
}
