// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm_migration::prelude::*;

/// 主函数
///
/// 数据库迁移工具入口点
#[async_std::main]
async fn main() {
    cli::run_cli(migration::Migrator).await;
}
