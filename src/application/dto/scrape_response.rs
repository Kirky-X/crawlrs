// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 爬取响应数据传输对象
///
/// 用于封装服务器对爬取请求的响应结果
#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapeResponseDto {
    /// 请求处理是否成功
    pub success: bool,
    /// 爬取任务的唯一标识符
    pub id: Uuid,
    /// 请求爬取的URL
    pub url: String,
}
