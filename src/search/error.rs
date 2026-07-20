// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::impl_basic_error_conversions;
use thiserror::Error;
use tokio::time::error::Elapsed;

/// 搜索错误类型
///
/// 架构 MEDIUM 3：将原来的 `Engine(String)` catch-all 拆分为结构化变体，
/// 使错误分类可被程序化匹配（如熔断器只对 `RateLimited`/`Captcha` 触发降级，
/// 重试逻辑只对 `EngineClient`/`BadHttpStatus(5xx)` 触发重试）。
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("搜索超时: {0}")]
    Timeout(#[from] Elapsed),
    #[error("解析错误: {0}")]
    Parse(String),
    #[error("内容解析错误: {0}")]
    ContentParsing(String),

    /// 引擎客户端调用失败（如 `EngineClient::scrape` 返回错误）
    /// 字段: (引擎名, 底层错误描述)
    #[error("引擎 {0} 客户端调用失败: {1}")]
    EngineClient(String, String),

    /// 引擎返回非 2xx HTTP 状态码
    /// 字段: (引擎名, HTTP 状态码)
    #[error("引擎 {0} 返回 HTTP 错误状态码: {1}")]
    BadHttpStatus(String, u16),

    /// 引擎被限流（HTTP 429 或速率限制服务拒绝）
    /// 字段: 引擎名或限流原因描述
    #[error("引擎被限流: {0}")]
    RateLimited(String),

    /// 引擎返回 CAPTCHA 验证页面（反爬虫拦截）
    /// 字段: 引擎名
    #[error("引擎 {0} 返回 CAPTCHA 验证页面")]
    Captcha(String),

    /// 引擎返回内容不足（HTML 内容过少，可能被反爬虫拦截）
    /// 字段: (引擎名, 详细描述)
    #[error("引擎 {0} 返回内容不足: {1}")]
    InsufficientContent(String, String),

    /// 所有搜索引擎都失败（router 层聚合失败）
    #[error("所有搜索引擎都失败")]
    AllEnginesFailed,

    /// 单个引擎执行失败（含 mock 测试用例）
    /// 字段: 引擎名或失败描述
    #[error("引擎 {0} 执行失败")]
    EngineFailed(String),

    /// 智能路由失败（重试耗尽后仍失败）
    /// 字段: 底层错误描述
    #[error("智能路由失败: {0}")]
    SmartRoutingFailed(String),

    /// 智能路由超时（手动 `tokio::time::timeout` 触发）
    /// 字段: 超时秒数
    #[error("智能路由超时: {0} 秒")]
    SmartRoutingTimeout(u64),

    /// 引擎创建失败（factory 层）
    /// 字段: 底层错误描述
    #[error("引擎创建失败: {0}")]
    EngineCreationFailed(String),

    #[error("熔断器打开: {0}")]
    CircuitOpen(String),

    #[error("没有可用的搜索引擎")]
    NoEngineAvailable,
}

impl_basic_error_conversions!(SearchError, Parse);
