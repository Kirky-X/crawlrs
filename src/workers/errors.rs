// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeWorker 错误类型
//!
//! 定义工作器模块使用的错误类型

use thiserror::Error;

/// 工作器错误类型
#[derive(Error, Debug)]
pub enum ScrapeWorkerError {
    /// 正则表达式编译错误
    #[error("正则表达式编译错误: {0}")]
    RegexError(String),

    /// 缓存锁获取失败
    #[error("正则表达式缓存锁获取失败")]
    CacheLockError,

    /// 选择器解析错误
    #[error("选择器解析错误: {0}")]
    SelectorError(String),

    /// 任务处理错误
    #[error("任务处理错误: {0}")]
    TaskError(String),
}

impl From<String> for ScrapeWorkerError {
    fn from(msg: String) -> Self {
        ScrapeWorkerError::TaskError(msg)
    }
}

impl From<regex::Error> for ScrapeWorkerError {
    fn from(e: regex::Error) -> Self {
        ScrapeWorkerError::RegexError(e.to_string())
    }
}

impl From<url::ParseError> for ScrapeWorkerError {
    fn from(e: url::ParseError) -> Self {
        ScrapeWorkerError::TaskError(format!("URL解析错误: {}", e))
    }
}

// 注意：scraper crate 的 SelectorError 不是公开类型
// 如果需要处理选择器错误，可以使用 Result 类型的错误信息
