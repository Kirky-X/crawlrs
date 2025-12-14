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
