// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 翻译辅助函数
//!
//! 提供 `t()` 和 `t_with_args()` 辅助函数，简化翻译调用。

use fluent_bundle::FluentValue;

use super::bundle::{I18nBundle, Locale};

/// 翻译消息（无参数）
///
/// 对 `I18nBundle::translate()` 的简化包装。
///
/// # Examples
/// ```ignore
/// use crate::i18n::t;
///
/// let msg = t(&locale, &bundle, "error-permission");
/// assert_eq!(msg, "Permission denied.");
/// ```
pub fn t(locale: &Locale, bundle: &I18nBundle, key: &str) -> String {
    bundle.translate(locale, key)
}

/// 翻译消息（带参数）
///
/// 对 `I18nBundle::translate_with_args()` 的简化包装。
///
/// # Examples
/// ```ignore
/// use crate::i18n::t_with_args;
/// use fluent_bundle::FluentValue;
///
/// let msg = t_with_args(
///     &locale, &bundle, "error-validation",
///     &[("message", FluentValue::from("bad input"))],
/// );
/// assert_eq!(msg, "Validation error: bad input");
/// ```
pub fn t_with_args(
    locale: &Locale,
    bundle: &I18nBundle,
    key: &str,
    args: &[(&str, FluentValue)],
) -> String {
    bundle.translate_with_args(locale, key, args)
}

/// 经启动期 i18n 全局束翻译运维日志（worker/引擎等无请求上下文出口，locale
/// 取启动检测/配置决议的默认值）。未初始化或 key 缺失时回退 key 本身
/// （与 Fluent 缺 key 语义一致，不 panic）。
pub fn tr_log(key: &str) -> String {
    match super::startup_i18n() {
        Some((locale, bundle)) => t(locale, bundle, key),
        None => key.to_string(),
    }
}

/// 同 [`tr_log`]，带 Fluent 占位参数
pub fn tr_log_args(key: &str, args: &[(&str, FluentValue)]) -> String {
    match super::startup_i18n() {
        Some((locale, bundle)) => t_with_args(locale, bundle, key, args),
        None => key.to_string(),
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
    fn test_t_function() {
        let bundle = test_bundle();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = t(&locale, &bundle, "error-permission");
        assert_eq!(msg, "Permission denied.");
    }

    #[test]
    fn test_t_with_args_function() {
        let bundle = test_bundle();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = t_with_args(
            &locale,
            &bundle,
            "error-validation",
            &[("message", FluentValue::from("invalid input"))],
        );
        // Fluent 在参数周围插入 Unicode 隔离标记（U+2068/U+2069）
        assert!(
            msg.contains("invalid input"),
            "Expected message to contain 'invalid input', got: {msg}"
        );
        assert!(msg.starts_with("Validation error:"));
    }

    #[test]
    fn test_t_zh_cn() {
        let bundle = test_bundle();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = t(&locale, &bundle, "error-permission");
        assert_eq!(msg, "权限不足。");
    }
}
