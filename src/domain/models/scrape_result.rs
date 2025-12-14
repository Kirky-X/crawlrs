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
use uuid::Uuid;

/// 爬取结果实体
///
/// 存储网页爬取任务的结果数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResult {
    /// 结果唯一标识符
    pub id: Uuid,
    /// 关联的任务ID
    pub task_id: Uuid,
    /// 目标URL
    pub url: String,
    /// HTTP响应状态码
    pub status_code: u16,
    /// 爬取的内容
    pub content: String,
    /// 内容类型
    pub content_type: String,
    /// 响应头
    pub headers: serde_json::Value,
    /// 元数据
    pub meta_data: serde_json::Value,
    /// 截图数据 (base64 encoded)
    pub screenshot: Option<String>,
    /// 响应时间（毫秒）
    pub response_time_ms: u64,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl ScrapeResult {
    /// 创建一个新的爬取结果
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
