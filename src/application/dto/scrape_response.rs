// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// 爬取响应数据传输对象
///
/// 用于封装服务器对爬取请求的响应结果
#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapeResponseDto {
    /// 爬取任务的唯一标识符
    pub id: Uuid,
    /// 请求爬取的URL
    pub url: String,
    /// 消耗的积分
    #[serde(default)]
    pub credits_used: u32,
}

/// 爬取结果数据传输对象
///
/// 用于封装爬取任务的结果数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResultDto {
    /// 结果内容
    pub content: String,
    /// HTTP 状态码
    pub status_code: u16,
    /// 内容类型
    pub content_type: Option<String>,
    /// 响应时间（毫秒）
    pub response_time_ms: i64,
    /// 响应头
    pub headers: Option<Value>,
    /// 元数据
    pub meta_data: Option<Value>,
    /// 截图（Base64 编码）
    pub screenshot: Option<String>,
    /// 创建时间
    pub created_at: NaiveDateTime,
}

/// 爬取状态响应数据传输对象
///
/// 用于封装获取爬取任务状态的响应结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeStatusResponseDto {
    /// 任务ID
    pub id: Uuid,
    /// 任务状态
    pub status: String,
    /// 请求URL
    pub url: String,
    /// 创建时间
    pub created_at: NaiveDateTime,
    /// 完成时间
    pub completed_at: Option<NaiveDateTime>,
    /// 爬取结果（仅当任务完成时存在）
    pub result: Option<ScrapeResultDto>,
    /// 任务元数据
    pub metadata: Option<Value>,
    /// 错误信息（仅当任务失败时存在）
    pub error: Option<String>,
}

/// 取消爬取响应数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelScrapeResponseDto {
    /// 取消成功的消息
    pub message: String,
}
