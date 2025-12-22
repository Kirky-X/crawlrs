// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::utils::errors::WorkerError;
use async_trait::async_trait;

/// Worker trait定义
///
/// 所有后台工作器都必须实现此trait
#[async_trait]
pub trait Worker: Send + Sync {
    /// 运行工作器
    async fn run(&self) -> Result<(), WorkerError>;

    /// 获取工作器名称
    fn name(&self) -> &str;
}
