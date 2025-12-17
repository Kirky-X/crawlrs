// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::scrape_result::ScrapeResult;
use anyhow::Result;
use async_trait::async_trait;

use uuid::Uuid;

/// 爬取结果仓库特质
///
/// 定义爬取结果数据访问接口
#[async_trait]
pub trait ScrapeResultRepository: Send + Sync {
    /// 保存爬取结果
    async fn save(&self, result: ScrapeResult) -> Result<()>;
    /// 根据任务ID查找结果
    async fn find_by_task_id(&self, task_id: Uuid) -> Result<Option<ScrapeResult>>;
}
