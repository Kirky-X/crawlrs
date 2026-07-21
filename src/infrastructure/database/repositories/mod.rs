// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod audit_log_repo_impl;
/// 仓库实现模块
///
/// 提供领域仓库接口的具体实现
/// 包括各种实体仓库的数据库实现
pub mod auth_scope_repo_impl;
pub mod crawl_repo_impl;
pub mod credits_repo_impl;
pub mod database_geo_restriction_repo;
pub mod geo_restriction_repo_impl;
pub mod macros;
pub mod scrape_result_repo_impl;
pub mod task_repo_impl;
pub mod tasks_backlog_repo_impl;
pub mod webhook_event_repo_impl;
pub mod webhook_repo_impl;
