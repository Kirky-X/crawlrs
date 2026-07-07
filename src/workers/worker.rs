// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use log::{debug, error, info};

/// Worker trait定义
///
/// 所有后台工作器都必须实现此trait
#[async_trait]
pub trait Worker: Send + Sync {
    /// 运行工作器
    async fn run(&self);

    /// 获取工作器名称
    fn name(&self) -> &str;
}

/// 处理结果枚举
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    /// 处理成功完成
    Completed,
    /// 发生错误
    Error(String),
    /// 无任务需要处理（空闲状态）
    Empty,
}

/// Worker 处理 trait
///
/// 定义单个处理周期的工作逻辑，配合 AbstractWorker 使用
#[async_trait]
pub trait WorkerProcess: Send + Sync {
    /// 获取工作器名称
    fn name(&self) -> &str;

    /// 处理单个周期
    async fn process(&self) -> ProcessResult;
}

/// 模板工作器骨架
///
/// 封装通用的循环逻辑：定时周期 + 错误处理 + 日志记录
pub struct AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync,
{
    processor: Arc<P>,
    interval: Duration,
}

impl<P> AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync,
{
    /// 创建新的模板工作器
    pub fn new(processor: Arc<P>, interval: Duration) -> Self {
        Self {
            processor,
            interval,
        }
    }
}

#[async_trait]
impl<P> Worker for AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync + 'static,
{
    /// 运行工作器（模板方法）
    async fn run(&self) {
        info!("Worker '{}' started", self.processor.name());
        let mut interval = interval(self.interval);

        loop {
            interval.tick().await;

            match self.processor.process().await {
                ProcessResult::Completed => {
                    debug!("Worker '{}' completed one cycle", self.processor.name());
                }
                ProcessResult::Error(e) => {
                    error!("Worker '{}' error: {}", self.processor.name(), e);
                }
                ProcessResult::Empty => {
                    debug!("Worker '{}' no work to do", self.processor.name());
                }
            }
        }
    }

    fn name(&self) -> &str {
        self.processor.name()
    }
}
