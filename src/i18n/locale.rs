// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Locale 检测与协商
//!
//! 提供 Accept-Language header 解析和 locale 协商功能。

use super::bundle::Locale;

/// 从 Accept-Language header 解析语言偏好列表
///
/// 按质量值（q factor）降序排列。无效的语言标识符会被跳过。
///
/// # Examples
/// ```ignore
/// let locales = parse_accept_language("en-US,en;q=0.9,zh-CN;q=0.8");
/// assert_eq!(locales.len(), 3);
/// assert_eq!(locales[0].to_string(), "en-US");
/// ```
pub fn parse_accept_language(header: &str) -> Vec<Locale> {
    let mut locales: Vec<(Locale, f32)> = header
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }

            let mut segments = part.split(';');
            let tag = segments.next()?.trim();

            let locale: Locale = tag.parse().ok()?;

            let quality = segments
                .next()
                .and_then(|q| q.trim().strip_prefix("q="))
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(1.0);

            Some((locale, quality))
        })
        .collect();

    // 按质量值降序排列
    locales.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    locales.into_iter().map(|(l, _)| l).collect()
}

/// 根据请求偏好协商最佳 locale
///
/// 优先级：精确匹配 > 语言前缀匹配 > 默认 locale
///
/// # Arguments
/// * `preferred` - 用户偏好的 locale 列表（按优先级排列）
/// * `supported` - 服务器支持的 locale 列表
/// * `default` - 默认 locale（最终回退）
pub fn negotiate_locale(preferred: &[Locale], supported: &[Locale], default: &Locale) -> Locale {
    for pref in preferred {
        // 精确匹配
        if supported.contains(pref) {
            return pref.clone();
        }

        // 语言前缀匹配（如 "en" 匹配 "en-US"）
        for sup in supported {
            if sup.language == pref.language {
                return sup.clone();
            }
        }
    }

    default.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_accept_language_basic() {
        let locales = parse_accept_language("en-US,en;q=0.9,zh-CN;q=0.8");
        assert_eq!(locales.len(), 3);
        assert_eq!(locales[0].to_string(), "en-US");
        assert_eq!(locales[1].to_string(), "en");
        assert_eq!(locales[2].to_string(), "zh-CN");
    }

    #[test]
    fn test_parse_accept_language_single() {
        let locales = parse_accept_language("zh-CN");
        assert_eq!(locales.len(), 1);
        assert_eq!(locales[0].to_string(), "zh-CN");
    }

    #[test]
    fn test_parse_accept_language_with_spaces() {
        let locales = parse_accept_language("en-US, zh-CN;q=0.5");
        assert_eq!(locales.len(), 2);
        assert_eq!(locales[0].to_string(), "en-US");
        assert_eq!(locales[1].to_string(), "zh-CN");
    }

    #[test]
    fn test_parse_accept_language_empty() {
        let locales = parse_accept_language("");
        assert!(locales.is_empty());
    }

    #[test]
    fn test_parse_accept_language_invalid_tags() {
        // 无效的 tag 被跳过
        let locales = parse_accept_language("invalid!!!,en-US;q=0.9");
        assert_eq!(locales.len(), 1);
        assert_eq!(locales[0].to_string(), "en-US");
    }

    #[test]
    fn test_negotiate_exact_match() {
        let preferred: Vec<Locale> = vec!["zh-CN".parse().unwrap()];
        let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
        let default: Locale = "en-US".parse().unwrap();

        let result = negotiate_locale(&preferred, &supported, &default);
        assert_eq!(result.to_string(), "zh-CN");
    }

    #[test]
    fn test_negotiate_language_prefix_match() {
        // "en" 应该匹配 "en-US"
        let preferred: Vec<Locale> = vec!["en".parse().unwrap()];
        let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
        let default: Locale = "en-US".parse().unwrap();

        let result = negotiate_locale(&preferred, &supported, &default);
        assert_eq!(result.to_string(), "en-US");
    }

    #[test]
    fn test_negotiate_fallback_to_default() {
        let preferred: Vec<Locale> = vec!["fr-FR".parse().unwrap()];
        let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
        let default: Locale = "en-US".parse().unwrap();

        let result = negotiate_locale(&preferred, &supported, &default);
        assert_eq!(result.to_string(), "en-US");
    }

    #[test]
    fn test_negotiate_empty_preferred() {
        let preferred: Vec<Locale> = vec![];
        let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
        let default: Locale = "en-US".parse().unwrap();

        let result = negotiate_locale(&preferred, &supported, &default);
        assert_eq!(result.to_string(), "en-US");
    }

    #[test]
    fn test_negotiate_priority_order() {
        // 第一个偏好不匹配，第二个匹配
        let preferred: Vec<Locale> = vec!["fr-FR".parse().unwrap(), "zh-CN".parse().unwrap()];
        let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
        let default: Locale = "en-US".parse().unwrap();

        let result = negotiate_locale(&preferred, &supported, &default);
        assert_eq!(result.to_string(), "zh-CN");
    }
}
