// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::config::settings::DatabaseSettings;
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

    opt.max_lifetime(Duration::from_secs(3600))
        .sqlx_logging(true);

    Database::connect(opt).await
}
