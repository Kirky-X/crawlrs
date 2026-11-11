// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 翻译辅助函数
//!
//! 提供 `t()` 和 `t_with_args()` 辅助函数，简化翻译调用。
//! 底层经 [`super::bundle::I18nBundle`] 走 `sdforge::i18n` 注册表。

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
/// 参数为 `&[(&str, String)]`，`{ $name }` 占位符按名替换。
///
/// # Examples
/// ```ignore
/// use crate::i18n::t_with_args;
///
/// let msg = t_with_args(
///     &locale, &bundle, "error-validation",
///     &[("message", "bad input".to_string())],
/// );
/// assert_eq!(msg, "Validation error: bad input");
/// ```
pub fn t_with_args(
    locale: &Locale,
    bundle: &I18nBundle,
    key: &str,
    args: &[(&str, String)],
) -> String {
    bundle.translate_with_args(locale, key, args)
}

/// 经启动期 i18n 全局束翻译运维日志（worker/引擎等无请求上下文出口，locale
/// 取启动检测/配置决议的默认值）。未初始化或 key 缺失时回退 key 本身
/// （与缺 key 语义一致，不 panic）。
pub fn tr_log(key: &str) -> String {
    match super::startup_i18n() {
        Some((locale, bundle)) => t(locale, bundle, key),
        None => key.to_string(),
    }
}

/// 同 [`tr_log`]，带 `{ $name }` 占位参数
pub fn tr_log_args(key: &str, args: &[(&str, String)]) -> String {
    match super::startup_i18n() {
        Some((locale, bundle)) => t_with_args(locale, bundle, key, args),
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bundle() -> I18nBundle {
        I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap()
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
            &[("message", "invalid input".to_string())],
        );
        assert_eq!(msg, "Validation error: invalid input");
    }

    #[test]
    fn test_t_zh_cn() {
        let bundle = test_bundle();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = t(&locale, &bundle, "error-permission");
        assert_eq!(msg, "权限不足。");
    }
}
