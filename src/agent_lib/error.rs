// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `agent-lib` 独立错误枚举（thiserror），与平台错误解耦。

use thiserror::Error;

/// `agent-lib` 模块的错误类型。
#[derive(Debug, Error)]
pub enum AgentLibError {
    /// URL 无效（解析失败或 scheme 不支持）
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// SSRF 校验未通过（DNS 解析/私网 IP/重绑定等）
    #[error("SSRF validation failed for {url}: {reason}")]
    SsrfDenied {
        /// 被拒绝的 URL
        url: String,
        /// 拒绝原因
        reason: String,
    },

    /// 逐跳出口裁决拒绝（请求前判定，未发起连接）
    #[error("egress guard denied URL: {url}")]
    EgressDenied {
        /// 被拒绝的 URL
        url: String,
    },

    /// HTTP 状态码非 2xx
    #[error("HTTP status {status} for {url}")]
    HttpStatus {
        /// 实际状态码
        status: u16,
        /// 请求的 URL
        url: String,
    },

    /// 响应体超过 `max_bytes` 上限
    #[error("response exceeded max_bytes limit of {max_bytes}")]
    MaxBytesExceeded {
        /// 配置的字节上限
        max_bytes: usize,
    },

    /// 请求超时
    #[error("request timed out: {0}")]
    Timeout(String),

    /// 重定向次数超过上限
    #[error("too many redirects (max {max_redirects})")]
    TooManyRedirects {
        /// 最大重定向次数
        max_redirects: u8,
    },

    /// 网络错误（reqwest 层）
    #[error("network error: {0}")]
    Network(String),

    /// 正文提取失败
    #[error("content extraction failed: {0}")]
    Extraction(String),

    /// Markdown 转换失败
    #[error("markdown conversion failed: {0}")]
    Markdown(String),

    /// 搜索失败（底层 SearchError）
    #[error("search failed: {0}")]
    Search(String),

    /// 不支持的搜索 provider
    #[error("unsupported search provider: {0}")]
    UnsupportedProvider(String),

    /// 内部错误
    #[error("internal error: {0}")]
    Internal(String),
}
