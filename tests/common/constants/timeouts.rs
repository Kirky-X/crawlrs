// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 测试超时常量定义
//!
//! 本模块定义了测试中使用的各种超时时间常量，用于统一管理测试的超时行为。

use std::time::Duration;

/// 快速测试超时时间（10秒）
///
/// 用于预期在 5 秒内完成的测试，提供一些缓冲时间。
pub const QUICK_TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 默认测试超时时间（30秒）
///
/// 用于预期在 5-30 秒内完成的测试，这是大多数测试的标准超时时间。
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// E2E 测试超时时间（90秒）
///
/// 用于端到端测试，预期在 30-90 秒内完成。
pub const E2E_TEST_TIMEOUT: Duration = Duration::from_secs(90);

/// 长时间运行测试超时时间（120秒）
///
/// 用于预期超过 90 秒的测试，如性能测试、压力测试等。
pub const LONG_RUNNING_TEST_TIMEOUT: Duration = Duration::from_secs(120);

/// API 请求超时时间（10秒）
///
/// 用于单个 HTTP API 请求的超时时间。
pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Webhook 交付超时时间（30秒）
///
/// 用于 webhook 交付操作的超时时间。
pub const WEBHOOK_DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 数据库操作超时时间（10秒）
///
/// 用于数据库查询和操作的超时时间。
pub const DATABASE_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Redis 操作超时时间（5秒）
///
/// 用于 Redis 缓存操作的超时时间。
pub const REDIS_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

/// 任务执行超时时间（60秒）
///
/// 用于任务执行操作的超时时间。
pub const TASK_EXECUTION_TIMEOUT: Duration = Duration::from_secs(60);

/// 爬虫任务超时时间（90秒）
///
/// 用于爬虫任务完成操作的超时时间。
pub const CRAWL_TASK_TIMEOUT: Duration = Duration::from_secs(90);

/// 搜索任务超时时间（30秒）
///
/// 用于搜索任务完成操作的超时时间。
pub const SEARCH_TASK_TIMEOUT: Duration = Duration::from_secs(30);

/// 提取任务超时时间（30秒）
///
/// 用于内容提取任务完成操作的超时时间。
pub const EXTRACTION_TASK_TIMEOUT: Duration = Duration::from_secs(30);

/// 批量操作超时时间（120秒）
///
/// 用于批量操作的超时时间，如批量取消任务等。
pub const BATCH_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);