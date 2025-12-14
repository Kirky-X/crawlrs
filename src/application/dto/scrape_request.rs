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

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 爬取请求数据传输对象
///
/// 用于封装客户端发起的网页爬取请求的相关参数
#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapeRequestDto {
    /// 要爬取的网页URL
    pub url: String,
    /// 请求的数据格式列表
    pub formats: Option<Vec<String>>,
    /// 自定义HTTP请求头
    pub headers: Option<Value>,
    /// 包含的HTML标签列表
    pub include_tags: Option<Vec<String>>,
    /// 排除的HTML标签列表
    pub exclude_tags: Option<Vec<String>>,
    /// 等待时间（毫秒）
    pub wait_for: Option<u64>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 回调Webhook地址
    pub webhook: Option<String>,
    /// 是否需要JavaScript渲染
    pub js_rendering: Option<bool>,
    /// 是否需要截图
    pub screenshot: Option<bool>,
    /// 截图配置
    pub screenshot_options: Option<ScreenshotOptionsDto>,
    /// 是否模拟移动设备
    pub mobile: Option<bool>,
    /// 提取规则
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScreenshotOptionsDto {
    pub full_page: Option<bool>,
    pub selector: Option<String>,
    pub quality: Option<u8>,
    pub format: Option<String>,
}
