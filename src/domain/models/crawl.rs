// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// 爬取任务实体
///
/// 表示一个网站爬取任务的完整信息，包含任务的基本属性、
/// 执行状态、统计信息和生命周期时间戳。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crawl {
    /// 爬取任务唯一标识符
    pub id: Uuid,
    /// 所属团队ID，用于权限隔离和数据归属
    pub team_id: Uuid,
    /// 任务名称，用于用户识别和管理
    pub name: String,
    /// 根URL，爬取的起始地址
    pub root_url: String,
    /// 目标URL，实际需要爬取的具体地址
    pub url: String,
    /// 爬取状态，跟踪任务的执行进度
    pub status: CrawlStatus,
    /// 爬取配置，JSON格式的任务参数和规则
    pub config: serde_json::Value,
    /// 总任务数，该爬取任务包含的子任务总数
    pub total_tasks: i32,
    /// 已完成任务数，成功完成的子任务数量
    pub completed_tasks: i32,
    /// 失败任务数，执行失败的子任务数量
    pub failed_tasks: i32,
    /// 创建时间，任务创建的时间戳
    pub created_at: DateTime<Utc>,
    /// 更新时间，任务状态最后更新的时间戳
    pub updated_at: DateTime<Utc>,
    /// 完成时间，任务完成的时间戳（可选，因为任务可能未完成）
    pub completed_at: Option<DateTime<Utc>>,
}

/// 爬取状态枚举
///
/// 表示爬取任务在其生命周期中的不同状态，用于跟踪任务的执行进度。
/// 状态转换遵循以下流程：
/// Queued → Processing → Completed/Failed/Cancelled
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

/// 将爬取状态格式化为字符串表示
///
/// 用于日志记录、API响应和状态显示
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

/// 从字符串解析爬取状态
///
/// 用于从数据库、API请求或配置文件中恢复状态值
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
