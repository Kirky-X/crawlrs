// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::i18n::{I18nBundle, Locale};
use crate::impl_basic_error_conversions;
use thiserror::Error;
use tokio::time::error::Elapsed;

/// 搜索错误类型
///
/// 架构将原来的 `Engine(String)` catch-all 拆分为结构化变体，
/// 使错误分类可被程序化匹配（如熔断器只对 `RateLimited`/`Captcha` 触发降级，
/// 重试逻辑只对 `EngineClient`/`BadHttpStatus(5xx)` 触发重试）。
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Search timed out: {0}")]
    Timeout(#[from] Elapsed),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Content parsing error: {0}")]
    ContentParsing(String),

    /// 引擎客户端调用失败（如 `EngineClient::scrape` 返回错误）
    /// 字段: (引擎名, 底层错误描述)
    #[error("Engine {0} client call failed: {1}")]
    EngineClient(String, String),

    /// 引擎返回非 2xx HTTP 状态码
    /// 字段: (引擎名, HTTP 状态码)
    #[error("Engine {0} returned HTTP error status: {1}")]
    BadHttpStatus(String, u16),

    /// 引擎被限流（HTTP 429 或速率限制服务拒绝）
    /// 字段: 引擎名或限流原因描述
    #[error("Engine rate limited: {0}")]
    RateLimited(String),

    /// 引擎返回 CAPTCHA 验证页面（反爬虫拦截）
    /// 字段: 引擎名
    #[error("Engine {0} returned a CAPTCHA page")]
    Captcha(String),

    /// 引擎返回内容不足（HTML 内容过少，可能被反爬虫拦截）
    /// 字段: (引擎名, 详细描述)
    #[error("Engine {0} returned insufficient content: {1}")]
    InsufficientContent(String, String),

    /// 所有搜索引擎都失败（router 层聚合失败）
    #[error("All search engines failed.")]
    AllEnginesFailed,

    /// 单个引擎执行失败（含 mock 测试用例）
    /// 字段: 引擎名或失败描述
    #[error("Engine {0} execution failed.")]
    EngineFailed(String),

    /// 智能路由失败（重试耗尽后仍失败）
    /// 字段: 底层错误描述
    #[error("Smart routing failed: {0}")]
    SmartRoutingFailed(String),

    /// 智能路由超时（手动 `tokio::time::timeout` 触发）
    /// 字段: 超时秒数
    #[error("Smart routing timed out: {0}s")]
    SmartRoutingTimeout(u64),

    /// 引擎创建失败（factory 层）
    /// 字段: 底层错误描述
    #[error("Engine creation failed: {0}")]
    EngineCreationFailed(String),

    #[error("Circuit breaker open: {0}")]
    CircuitOpen(String),

    #[error("No available search engine.")]
    NoEngineAvailable,
}

impl_basic_error_conversions!(SearchError, Parse);

impl SearchError {
    /// 获取本地化的用户可见错误消息
    ///
    /// 与 `DomainError::user_message_locale` 同构：Display 为英文规范串，
    /// 本地化文案经 FTL（`locales/*/errors.ftl` 的 `search-error-*`）输出。
    pub fn user_message_locale(&self, locale: &Locale, bundle: &I18nBundle) -> String {
        use fluent_bundle::FluentValue;

        match self {
            SearchError::Network(inner) => bundle.translate_with_args(
                locale,
                "search-error-network",
                &[("message", FluentValue::from(inner.to_string()))],
            ),
            SearchError::Timeout(inner) => bundle.translate_with_args(
                locale,
                "search-error-timeout",
                &[("message", FluentValue::from(inner.to_string()))],
            ),
            SearchError::Parse(message) => bundle.translate_with_args(
                locale,
                "search-error-parse",
                &[("message", FluentValue::from(message.as_str()))],
            ),
            SearchError::ContentParsing(message) => bundle.translate_with_args(
                locale,
                "search-error-content-parsing",
                &[("message", FluentValue::from(message.as_str()))],
            ),
            SearchError::EngineClient(engine, message) => bundle.translate_with_args(
                locale,
                "search-error-engine-client",
                &[
                    ("engine", FluentValue::from(engine.as_str())),
                    ("message", FluentValue::from(message.as_str())),
                ],
            ),
            SearchError::BadHttpStatus(engine, status) => bundle.translate_with_args(
                locale,
                "search-error-bad-http-status",
                &[
                    ("engine", FluentValue::from(engine.as_str())),
                    ("status", FluentValue::from(*status as i64)),
                ],
            ),
            SearchError::RateLimited(reason) => bundle.translate_with_args(
                locale,
                "search-error-rate-limited",
                &[("message", FluentValue::from(reason.as_str()))],
            ),
            SearchError::Captcha(engine) => bundle.translate_with_args(
                locale,
                "search-error-captcha",
                &[("engine", FluentValue::from(engine.as_str()))],
            ),
            SearchError::InsufficientContent(engine, message) => bundle.translate_with_args(
                locale,
                "search-error-insufficient-content",
                &[
                    ("engine", FluentValue::from(engine.as_str())),
                    ("message", FluentValue::from(message.as_str())),
                ],
            ),
            SearchError::AllEnginesFailed => {
                bundle.translate(locale, "search-error-all-engines-failed")
            }
            SearchError::EngineFailed(engine) => bundle.translate_with_args(
                locale,
                "search-error-engine-failed",
                &[("engine", FluentValue::from(engine.as_str()))],
            ),
            SearchError::SmartRoutingFailed(message) => bundle.translate_with_args(
                locale,
                "search-error-smart-routing-failed",
                &[("message", FluentValue::from(message.as_str()))],
            ),
            SearchError::SmartRoutingTimeout(seconds) => bundle.translate_with_args(
                locale,
                "search-error-smart-routing-timeout",
                &[("seconds", FluentValue::from(*seconds as i64))],
            ),
            SearchError::EngineCreationFailed(message) => bundle.translate_with_args(
                locale,
                "search-error-engine-creation-failed",
                &[("message", FluentValue::from(message.as_str()))],
            ),
            SearchError::CircuitOpen(message) => bundle.translate_with_args(
                locale,
                "search-error-circuit-open",
                &[("message", FluentValue::from(message.as_str()))],
            ),
            SearchError::NoEngineAvailable => {
                bundle.translate(locale, "search-error-no-engine-available")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bundle() -> I18nBundle {
        let dir = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        I18nBundle::load("en-US", &["en-US", "zh-CN"], &dir).unwrap()
    }

    #[test]
    fn test_display_english_canonical() {
        let err = SearchError::NoEngineAvailable;
        assert_eq!(err.to_string(), "No available search engine.");

        let err = SearchError::BadHttpStatus("bing".to_string(), 503);
        assert_eq!(
            err.to_string(),
            "Engine bing returned HTTP error status: 503"
        );
    }

    #[test]
    fn test_user_message_locale_en() {
        let bundle = test_bundle();
        let locale: Locale = "en-US".parse().unwrap();
        let err = SearchError::Captcha("sogou".to_string());
        // Fluent 在参数周围插入 Unicode 隔离标记（U+2068/U+2069）
        let msg = err.user_message_locale(&locale, &bundle);
        assert!(msg.starts_with("Engine "), "got: {msg}");
        assert!(msg.contains("sogou"), "got: {msg}");
        assert!(msg.ends_with("returned a CAPTCHA page"), "got: {msg}");
    }

    #[test]
    fn test_user_message_locale_zh() {
        let bundle = test_bundle();
        let locale: Locale = "zh-CN".parse().unwrap();
        let err = SearchError::AllEnginesFailed;
        assert_eq!(
            err.user_message_locale(&locale, &bundle),
            "所有搜索引擎都失败。"
        );
    }
}
