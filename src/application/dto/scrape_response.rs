// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Sensitive HTTP header names that must be redacted before returning to clients.
const SENSITIVE_HEADERS: &[&str] = &[
    "set-cookie",
    "authorization",
    "x-auth-token",
    "x-api-key",
    "proxy-authorization",
    "cookie",
];

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

impl ScrapeResultDto {
    /// Remove sensitive headers (e.g. Set-Cookie, Authorization) from the `headers` field.
    ///
    /// This prevents accidental leakage of credentials to API consumers.
    ///
    /// Header 名称按 ASCII 大小写不敏感匹配（HTTP/1.1 折叠大小写合法，
    /// 上游站点可能输出 `SET-COOKIE` 等变体）；命中的键保留、值替换为
    /// `[REDACTED]`，避免向客户端暴露键的存在性差异。
    pub fn filter_sensitive_headers(&mut self) {
        if let Some(Value::Object(ref mut map)) = self.headers {
            let sensitive: Vec<String> = map
                .keys()
                .filter(|key| {
                    SENSITIVE_HEADERS
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(key))
                })
                .cloned()
                .collect();
            for key in sensitive {
                map.insert(key, Value::String("[REDACTED]".to_string()));
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_sensitive_headers_regardless_of_case() {
        let mut dto = ScrapeResultDto {
            content: String::new(),
            status_code: 200,
            content_type: None,
            response_time_ms: 0,
            headers: Some(json!({
                "Set-Cookie": "session=abc",
                "X-Api-Key": "k-123",
                "AUTHORIZATION": "Bearer t",
                "Content-Type": "text/html",
            })),
            meta_data: None,
            screenshot: None,
            created_at: NaiveDateTime::default(),
        };
        dto.filter_sensitive_headers();
        let map = dto.headers.as_ref().unwrap().as_object().unwrap();
        assert_eq!(map.get("Set-Cookie").unwrap(), "[REDACTED]");
        assert_eq!(map.get("X-Api-Key").unwrap(), "[REDACTED]");
        assert_eq!(map.get("AUTHORIZATION").unwrap(), "[REDACTED]");
        assert_eq!(map.get("Content-Type").unwrap(), "text/html");
    }

    #[test]
    fn redacts_headers_when_no_headers_present() {
        let mut dto = ScrapeResultDto {
            content: String::new(),
            status_code: 200,
            content_type: None,
            response_time_ms: 0,
            headers: None,
            meta_data: None,
            screenshot: None,
            created_at: NaiveDateTime::default(),
        };
        dto.filter_sensitive_headers();
        assert!(dto.headers.is_none());
    }
}
