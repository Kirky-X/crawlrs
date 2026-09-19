// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Locale 检测与协商
//!
//! 提供系统语言检测链（`CRAWLRS_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG` →
//! `sys_locale::get_locale()` → `en-US`）、Accept-Language header 解析和 locale 协商功能。
//!
//! 只支持 en-US / zh-CN 两种 locale；`zh*` 归一为 zh-CN，其余（含 en 变体、
//! 未知语言、畸形值）归一为 en-US；`C`/`POSIX` 跳过当前环节继续回退链。

use std::str::FromStr;

use super::bundle::Locale;

/// 内置默认 locale（default.toml 未显式指定 `i18n.default_locale` 时的回退值）
pub const BUILTIN_DEFAULT_LOCALE: &str = "en-US";

/// 项目级语言覆盖环境变量（检测链第一优先级，语义同 `<PROJ>_LANG`）
const PROJ_LANG_ENV: &str = "CRAWLRS_LANG";

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

/// 归一化原始 locale 标签为受支持的 `Locale`
///
/// - 剥离 `@modifier` 与 `.codeset`（如 `zh_CN.UTF-8` → `zh-CN`），`_` 归一为 `-`
/// - `zh*` → `zh-CN`；其余一切（en 变体、未知语言、畸形值）→ `en-US`
/// - `C`/`POSIX`/空串 → `None`（回退链继续，不直接终结）
fn normalize_locale_label(raw: &str) -> Option<Locale> {
    let s = raw
        .trim()
        .split('@')
        .next()?
        .split('.')
        .next()?
        .trim()
        .replace('_', "-");
    if s.is_empty() || s.eq_ignore_ascii_case("C") || s.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    if s.to_ascii_lowercase().starts_with("zh") {
        return "zh-CN".parse().ok();
    }
    "en-US".parse().ok()
}

/// 从注入的 env 读取函数 + 系统 locale 探测值检测语言（纯函数，可测）
///
/// 检测链（强制顺序）：`CRAWLRS_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG` →
/// `sys_locale` → `en-US`（终极回退）。结果域恒为 {en-US, zh-CN}。
///
/// # Arguments
/// * `getenv` - 环境变量读取函数（返回 `None` 或空白串表示未设置）
/// * `sys_locale` - `sys_locale::get_locale()` 的探测结果（测试可注入）
pub fn detect_locale_from(
    getenv: impl Fn(&str) -> Option<String>,
    sys_locale: Option<&str>,
) -> Locale {
    // 1. 项目覆盖变量
    if let Some(lang) = getenv(PROJ_LANG_ENV) {
        if let Some(locale) = normalize_locale_label(&lang) {
            return locale;
        }
    }
    // 2. POSIX 环境链（显式读取保证 Windows/边缘环境的确定性）
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(lang) = getenv(key) {
            if let Some(locale) = normalize_locale_label(&lang) {
                return locale;
            }
        }
    }
    // 3. sys-locale 系统探测
    if let Some(s) = sys_locale {
        if let Some(locale) = normalize_locale_label(s) {
            return locale;
        }
    }
    // 4. 终极回退：默认英文
    Locale::from_str(BUILTIN_DEFAULT_LOCALE).expect("builtin default locale is valid")
}

/// 检测系统语言（读取真实进程环境 + sys-locale）
///
/// 结果域恒为 {en-US, zh-CN}；任何失败/未知终结于 en-US。
pub fn detect_system_locale() -> Locale {
    detect_locale_from(
        |key| std::env::var(key).ok().filter(|v| !v.trim().is_empty()),
        sys_locale::get_locale().as_deref(),
    )
}

/// 决议启动期默认 locale（纯函数，可测）
///
/// 优先级：
/// 1. 配置显式指定（`configured_default` ≠ 内置默认 `en-US`）→ 配置值优先；
/// 2. 配置未显式指定（仍为 default.toml 内置值）→ 系统检测链
///    （[`detect_system_locale`]）；
/// 3. 检测结果不在 `supported` 内（视为检测失败）→ 回退配置值——
///    即 default.toml 的 `i18n.default_locale` 语义：**检测失败时的回退**。
pub fn resolve_startup_default_locale(configured_default: &str, supported: &[&str]) -> Locale {
    let builtin: Locale = Locale::from_str(BUILTIN_DEFAULT_LOCALE).expect("valid builtin locale");
    let configured: Locale = configured_default.parse().unwrap_or_else(|_| builtin.clone());

    // 配置显式指定（≠ 内置默认）→ 配置优先
    if configured != builtin {
        return configured;
    }

    // 未显式指定 → 系统检测；结果须在 supported 内，否则回退配置值
    let detected = detect_system_locale();
    let supported_locales: Vec<Locale> = supported.iter().filter_map(|s| s.parse().ok()).collect();
    if supported_locales.contains(&detected) {
        detected
    } else {
        configured
    }
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

    // =========================================================================
    // 系统语言检测链守卫测试（env 注入式纯函数，不触碰进程环境，并行安全）
    // =========================================================================

    /// 空 env 的便捷构造
    fn no_env() -> impl Fn(&str) -> Option<String> {
        |_| None
    }

    #[test]
    fn test_detect_chain_zh_cn_utf8() {
        // zh_CN.UTF-8 → zh-CN（剥离 codeset + `_` 归一）
        let getenv = |key: &str| match key {
            "LANG" => Some("zh_CN.UTF-8".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "zh-CN");
    }

    #[test]
    fn test_detect_chain_zh_tw_collapses_to_zh_cn() {
        // zh_TW → zh-CN（zh* 全部归一，禁止第三语言）
        let getenv = |key: &str| match key {
            "LC_ALL" => Some("zh_TW".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "zh-CN");
    }

    #[test]
    fn test_detect_chain_unsupported_language_falls_back_to_en() {
        // fr_FR → en-US（其余一切归一 en-US）
        let getenv = |key: &str| match key {
            "LANG" => Some("fr_FR.UTF-8".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn test_detect_chain_c_posix_skips_and_terminates_en() {
        // C/POSIX 不终结检测链：LC_ALL=C 时继续尝试 LANG=zh_CN
        let getenv = |key: &str| match key {
            "LC_ALL" => Some("C".to_string()),
            "LANG" => Some("zh_CN".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "zh-CN");

        // 全链 C/POSIX/POSIX → 终极回退 en-US
        let getenv = |key: &str| match key {
            "LC_ALL" => Some("C".to_string()),
            "LC_MESSAGES" => Some("C".to_string()),
            "LANG" => Some("POSIX".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn test_detect_chain_empty_or_malformed_falls_back_to_en() {
        // 空/畸形 env → 跳过，链尾 en-US
        let getenv = |key: &str| match key {
            "LC_ALL" => Some("   ".to_string()),
            "LC_MESSAGES" => Some("@@@!!!".to_string()),
            "LANG" => Some(String::new()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "en-US");

        // 全部未设置且 sys-locale 无结果 → en-US
        let locale = detect_locale_from(no_env(), None);
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn test_detect_chain_proj_lang_overrides_lc_all() {
        // CRAWLRS_LANG 优先于 LC_ALL
        let getenv = |key: &str| match key {
            "CRAWLRS_LANG" => Some("zh-CN".to_string()),
            "LC_ALL" => Some("en_US.UTF-8".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "zh-CN");

        // 反向：CRAWLRS_LANG=en 时覆盖 zh 系统
        let getenv = |key: &str| match key {
            "CRAWLRS_LANG" => Some("en".to_string()),
            "LC_ALL" => Some("zh_CN.UTF-8".to_string()),
            _ => None,
        };
        let locale = detect_locale_from(getenv, None);
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn test_detect_chain_sys_locale_used_after_env() {
        // env 链全部未设置 → sys-locale 探测
        assert_eq!(
            detect_locale_from(no_env(), Some("zh-CN")).to_string(),
            "zh-CN"
        );
        assert_eq!(
            detect_locale_from(no_env(), Some("en_US")).to_string(),
            "en-US"
        );
        // sys-locale 未知语言同样归一 en-US
        assert_eq!(
            detect_locale_from(no_env(), Some("fr-FR")).to_string(),
            "en-US"
        );
    }

    #[test]
    fn test_detect_system_locale_result_domain() {
        // 真实环境调用：结果域恒为 {en-US, zh-CN}，不 panic
        let locale = detect_system_locale();
        assert!(
            locale.to_string() == "en-US" || locale.to_string() == "zh-CN",
            "detect_system_locale must resolve to en-US or zh-CN, got {}",
            locale
        );
    }

    // =========================================================================
    // 启动默认 locale 决议守卫测试
    // =========================================================================

    #[test]
    fn test_resolve_startup_explicit_config_wins() {
        // 配置显式指定（≠ 内置默认 en-US）→ 配置优先，不走检测链
        let locale = resolve_startup_default_locale("zh-CN", &["en-US", "zh-CN"]);
        assert_eq!(locale.to_string(), "zh-CN");

        // 非内置显式值即便不在 supported 内也优先（显式指定即用户意图，
        // 加载期由 I18nBundle::load/validate_i18n 负责报错）
        let locale = resolve_startup_default_locale("zh-TW", &["en-US"]);
        assert_eq!(locale.to_string(), "zh-TW");
    }

    #[test]
    fn test_resolve_startup_detection_applies_when_unspecified() {
        // 配置仍为内置默认（未显式指定）→ 检测链生效（此处环境注入 zh）。
        // CRAWLRS_LANG 为进程级环境变量，经 ENV_MUTEX 串行化避免并行竞态。
        let _guard = crate::common::test_support::ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = std::env::var("CRAWLRS_LANG").ok();
        std::env::set_var("CRAWLRS_LANG", "zh-CN");
        let locale = resolve_startup_default_locale("en-US", &["en-US", "zh-CN"]);
        match saved {
            Some(v) => std::env::set_var("CRAWLRS_LANG", v),
            None => std::env::remove_var("CRAWLRS_LANG"),
        }
        assert_eq!(locale.to_string(), "zh-CN");
    }

    #[test]
    fn test_resolve_startup_unsupported_detection_falls_back_to_config() {
        // 检测结果不在 supported 内（检测失败）→ 回退配置值：
        // 检测给出 zh-CN，但 supported 仅 en-US → 配置值 en-US 胜出
        let _guard = crate::common::test_support::ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let saved = std::env::var("CRAWLRS_LANG").ok();
        std::env::set_var("CRAWLRS_LANG", "zh-CN");
        let locale = resolve_startup_default_locale("en-US", &["en-US"]);
        match saved {
            Some(v) => std::env::set_var("CRAWLRS_LANG", v),
            None => std::env::remove_var("CRAWLRS_LANG"),
        }
        assert_eq!(locale.to_string(), "en-US");
    }

    #[test]
    fn test_resolve_startup_invalid_config_falls_back_to_builtin() {
        // 配置值非法 → 内置默认 en-US
        let locale = resolve_startup_default_locale("not-a-locale!!!", &["en-US", "zh-CN"]);
        assert_eq!(locale.to_string(), "en-US");
    }
}
