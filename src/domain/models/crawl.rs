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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// 爬取任务实体
///
/// 表示一个网站爬取任务的完整信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crawl {
    /// 爬取任务唯一标识符
    pub id: Uuid,
    /// 所属团队ID
    pub team_id: Uuid,
    /// 任务名称
    pub name: String,
    /// 根URL
    pub root_url: String,
    /// 目标URL
    pub url: String,
    /// 爬取状态
    pub status: CrawlStatus,
    /// 爬取配置
    pub config: serde_json::Value,
    /// 总任务数
    pub total_tasks: i32,
    /// 已完成任务数
    pub completed_tasks: i32,
    /// 失败任务数
    pub failed_tasks: i32,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
}

/// 爬取状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrawlStatus {
    /// 已入队
    #[default]
    Queued,
    /// 处理中
    Processing,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已取消
    Cancelled,
}

impl fmt::Display for CrawlStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CrawlStatus::Queued => write!(f, "queued"),
            CrawlStatus::Processing => write!(f, "processing"),
            CrawlStatus::Completed => write!(f, "completed"),
            CrawlStatus::Failed => write!(f, "failed"),
            CrawlStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for CrawlStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(CrawlStatus::Queued),
            "processing" => Ok(CrawlStatus::Processing),
            "completed" => Ok(CrawlStatus::Completed),
            "failed" => Ok(CrawlStatus::Failed),
            "cancelled" => Ok(CrawlStatus::Cancelled),
            _ => Err(()),
        }
    }
}
