// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
