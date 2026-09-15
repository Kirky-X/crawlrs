// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 工作器模块
///
/// 提供后台任务处理和工作器管理功能
/// 包括任务执行、工作器生命周期管理和并发控制
pub mod backlog_worker;
/// 抓取缓存工具集
///
/// 从 ScrapeWorker 抽取的 cache key 生成、URL 日志脱敏、敏感响应头过滤、
/// borrowed 序列化结构体（性能）等纯函数。
pub mod cache_utils;
/// 请求合并协调器
///
/// 从 ScrapeWorker 抽取的同 URL 并发请求 single-flight 协调逻辑。
pub mod coalesce_coordinator;
/// 深度爬取模块（§16）
///
/// URL 过滤、评分、优先级队列与自适应停止条件。
pub mod crawl;
/// 爬取链接提取器
///
/// 从 `scrape_worker.rs` 提取的 robots.txt 检查、完成状态更新和链接提取入队。
pub mod crawl_link_extractor;
pub mod errors;
pub mod expiration_worker;
pub mod lock_heartbeat;
pub mod manager;
/// Markdown 后处理器（gated `markdown` 特性）
///
/// 从 ScrapeWorker 抽取的 HTML→Markdown 转换逻辑。
#[cfg(feature = "content")]
pub mod markdown_post_processor;
pub mod scheduler;
pub mod scrape_executor;
pub mod scrape_response_builder;
pub mod scrape_worker;
/// 优雅退出协调器
///
/// 提供 SIGTERM/SIGINT 监听与 worker 循环的关闭编排。
pub mod shutdown;
pub mod task_state_machine;
/// webhook feature 关闭时不编译此模块
/// webhook_worker spawn 也会门控 main.rs
#[cfg(feature = "webhook")]
pub mod webhook_worker;
pub mod worker;

pub use errors::ScrapeWorkerError;
pub use worker::{AbstractWorker, ProcessResult, Worker, WorkerProcess};
