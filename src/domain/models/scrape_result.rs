// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 爬取结果实体
///
/// 存储网页爬取任务的结果数据，包含爬取到的内容、
/// 响应信息和性能指标。每个结果对应一个具体的爬取任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResult {
    /// 结果唯一标识符
    pub id: Uuid,
    /// 关联的任务ID，建立与爬取任务的关联关系
    pub task_id: Uuid,
    /// 目标URL，被爬取的具体网页地址
    pub url: String,
    /// HTTP响应状态码，表示请求的处理结果
    pub status_code: u16,
    /// 爬取的内容，网页的HTML或其他响应内容
    pub content: String,
    /// 内容类型，HTTP响应的Content-Type头信息
    pub content_type: String,
    /// 响应头，完整的HTTP响应头部信息
    pub headers: serde_json::Value,
    /// 元数据，额外的爬取信息和统计
    pub meta_data: serde_json::Value,
    /// 截图数据，网页截图的base64编码（可选）
    pub screenshot: Option<String>,
    /// 响应时间（毫秒），从发起请求到收到响应的总时间
    pub response_time_ms: u64,
    /// 创建时间，结果记录创建的时间戳
    pub created_at: DateTime<Utc>,
}

impl ScrapeResult {
    /// 创建一个新的爬取结果
    ///
    /// # 参数
    ///
    /// * `task_id` - 关联的任务ID
    /// * `url` - 被爬取的URL
    /// * `status_code` - HTTP响应状态码
    /// * `content` - 爬取到的内容
    /// * `content_type` - 内容类型
    /// * `response_time_ms` - 响应时间（毫秒）
    ///
    /// # 返回值
    ///
    /// 返回一个新的ScrapeResult实例，包含生成的唯一ID和当前时间戳
    pub fn new(
        task_id: Uuid,
        url: String,
        status_code: u16,
        content: String,
        content_type: String,
        response_time_ms: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            task_id,
            url,
            status_code,
            content,
            content_type,
            headers: serde_json::Value::Null,
            meta_data: serde_json::Value::Null,
            screenshot: None,
            response_time_ms,
            created_at: Utc::now(),
        }
    }
}
