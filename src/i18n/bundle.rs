// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! i18n 翻译 bundle 门面
//!
//! 消息层底层为 `sdforge::i18n` 宿主注册表（见 [`super::catalog`]）：
//! `locales/*.ftl` 编译期内嵌，启动期行解析注册；本类型只承载
//! default/supported locale 元数据并提供翻译入口，供 `CrawlRsState`
//! （`Arc<I18nBundle>`）与 `i18n` 中间件（locale 协商）使用。
//!
//! 查找顺序：指定 locale → default locale → 返回 key 本身（绝不 panic）。
//! 键位一致性校验（en/zh 键集合与占位符对齐）基于同一份内嵌目录。

use std::collections::BTreeSet;

use unic_langid::LanguageIdentifier;

use crate::common::error::CrawlRsError;

use super::catalog;

/// Locale 类型别名（BCP 47 语言标识符）
pub type Locale = LanguageIdentifier;

/// i18n 翻译 bundle 门面（元数据句柄，天然 `Send + Sync`）
///
/// 翻译内容注册在 `sdforge::i18n` 全局注册表（加载时幂等注册一次）；
/// 本结构持有 default/supported locale，作为状态组件在应用内共享。
pub struct I18nBundle {
    /// 默认 locale（回退语言）
    default_locale: Locale,
    /// 支持的 locale 列表
    supported_locales: Vec<Locale>,
}

impl I18nBundle {
    /// 加载翻译 bundle：注册内嵌目录并解析 locale 元数据
    ///
    /// # Arguments
    /// * `default_locale` - 默认 locale 字符串（如 "en-US"）
    /// * `supported` - 支持的 locale 字符串列表
    ///
    /// # Errors
    /// 返回 `CrawlRsError::Config` 如果 locale 字符串非法，或某个
    /// supported locale 无内嵌消息源（未登记进 `catalog` 模块的 `FILES` 清单）
    pub fn load(default_locale: &str, supported: &[&str]) -> Result<Self, CrawlRsError> {
        catalog::register_all();

        let default_locale: Locale = default_locale
            .parse()
            .map_err(|e| CrawlRsError::Config(format!("Invalid default locale: {e}")))?;

        let supported_locales: Vec<Locale> = supported
            .iter()
            .map(|s| {
                s.parse().map_err(|e| {
                    CrawlRsError::Config(format!("Invalid supported locale '{s}': {e}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        for locale in &supported_locales {
            if catalog::sources_for(&locale.to_string()).is_none() {
                return Err(CrawlRsError::Config(format!(
                    "No embedded message catalog for supported locale '{locale}' \
                     (only en-US/zh-CN are built in)"
                )));
            }
        }

        Ok(Self {
            default_locale,
            supported_locales,
        })
    }

    /// 翻译消息（无参数）
    ///
    /// 查找指定 locale 的翻译，如果 key 不存在则回退到 default_locale。
    /// 如果 default_locale 也不存在，返回 key 本身。
    pub fn translate(&self, locale: &Locale, key: &str) -> String {
        self.translate_inner(&locale.to_string(), key, &[])
    }

    /// 翻译消息（带参数）
    ///
    /// 参数通过 `&[(&str, String)]` 传入，`{ $name }` 占位符按名替换。
    pub fn translate_with_args(
        &self,
        locale: &Locale,
        key: &str,
        args: &[(&str, String)],
    ) -> String {
        self.translate_inner(&locale.to_string(), key, args)
    }

    /// 翻译公共路径：指定 locale → default locale → key 本身。
    /// `translate_for` 未命中返回 key 原串（键值域为 kebab-case 键名，
    /// 与人类文案不相交，以「结果等于 key」判定 miss）。
    fn translate_inner(&self, locale: &str, key: &str, args: &[(&str, String)]) -> String {
        let primary = sdforge::i18n::translate_for(locale, key, args);
        if primary != key {
            return primary;
        }
        if self.default_locale != locale {
            let fallback =
                sdforge::i18n::translate_for(&self.default_locale.to_string(), key, args);
            if fallback != key {
                return fallback;
            }
        }
        log::warn!("Translation key '{key}' not found for locale '{locale}'");
        key.to_string()
    }

    /// 获取默认 locale
    pub fn default_locale(&self) -> &Locale {
        &self.default_locale
    }

    /// 获取支持的 locale 列表（供中间件使用）
    pub fn supported_locales(&self) -> &[Locale] {
        &self.supported_locales
    }

    /// 启动时验证所有 supported locale 的 key 集合一致性
    ///
    /// 返回不一致的描述列表。空列表表示完全一致。
    /// 以 default_locale 的 key 集合为基准，其他 locale 缺少或多出的
    /// key 都会被报告；占位符集合不一致同样报告（运行时会把未替换的
    /// `{ $name }` 裸句柄吐给用户）。
    pub fn validate_key_consistency(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let default_str = self.default_locale.to_string();
        let default_keys = self.keys_for(&default_str);

        for locale in &self.supported_locales {
            let locale_str = locale.to_string();
            if locale_str == default_str {
                continue;
            }
            warnings.extend(self.diff_locale_keys(&locale_str, &default_keys));
        }

        warnings
    }

    /// 内嵌目录中指定 locale 的全部消息键
    fn keys_for(&self, locale: &str) -> BTreeSet<String> {
        catalog::sources_for(locale)
            .map(|sources| {
                sources
                    .iter()
                    .flat_map(|s| catalog::parse_entries(s))
                    .map(|(k, _)| k.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 键 → 占位符集合（内嵌目录，指定 locale）
    fn placeholders_for(&self, locale: &str) -> Vec<(String, BTreeSet<&'static str>)> {
        catalog::sources_for(locale)
            .map(|sources| {
                sources
                    .iter()
                    .flat_map(|s| catalog::parse_entries(s))
                    .map(|(k, v)| (k.to_string(), catalog::placeholders(v)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 对比单个 locale 与 default：先报缺少/多出的 key，再报占位符失配
    fn diff_locale_keys(&self, locale: &str, default_keys: &BTreeSet<String>) -> Vec<String> {
        let mut warnings = Vec::new();
        let locale_keys = self.keys_for(locale);

        for key in default_keys {
            if !locale_keys.contains(key) {
                warnings.push(format!(
                    "Locale '{locale}' missing key '{key}' (present in default '{}')",
                    self.default_locale
                ));
            }
        }

        for key in &locale_keys {
            if !default_keys.contains(key) {
                warnings.push(format!(
                    "Locale '{locale}' has extra key '{key}' (not in default '{}')",
                    self.default_locale
                ));
            }
        }

        // 占位符对齐（键存在但占位符不一致会导致某语言渲染出裸 {$name} 句柄）
        let default_placeholders: std::collections::BTreeMap<String, BTreeSet<&str>> = self
            .placeholders_for(&self.default_locale.to_string())
            .into_iter()
            .collect();
        for (key, vars) in self.placeholders_for(locale) {
            if let Some(default_vars) = default_placeholders.get(&key) {
                if vars != *default_vars {
                    warnings.push(format!(
                        "Locale '{locale}' key '{key}' placeholders {vars:?} differ from \
                         default '{default_vars:?}'"
                    ));
                }
            }
        }

        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_bundle() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        assert_eq!(bundle.default_locale().to_string(), "en-US");
        assert_eq!(bundle.supported_locales().len(), 2);
    }

    #[test]
    fn test_load_unsupported_locale_returns_error() {
        let result = I18nBundle::load("en-US", &["en-US", "fr-FR"]);
        assert!(result.is_err(), "unsupported locale must be rejected");
    }

    #[test]
    fn test_load_invalid_locale_string_returns_error() {
        let result = I18nBundle::load("not a locale!!", &["en-US"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_translate_no_args() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate(&locale, "error-database");
        assert_eq!(msg, "Database operation failed. Please try again later.");

        let msg = bundle.translate(&locale, "error-permission");
        assert_eq!(msg, "Permission denied.");
    }

    #[test]
    fn test_translate_zh_cn() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = bundle.translate(&locale, "error-database");
        assert_eq!(msg, "数据库操作失败，请稍后重试。");
    }

    #[test]
    fn test_translate_with_args() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate_with_args(
            &locale,
            "error-validation",
            &[("message", "invalid email".to_string())],
        );
        assert_eq!(msg, "Validation error: invalid email");
    }

    #[test]
    fn test_translate_with_args_zh_cn() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = bundle.translate_with_args(
            &locale,
            "error-validation",
            &[("message", "无效邮箱".to_string())],
        );
        assert!(msg.contains("无效邮箱"), "got: {msg}");
    }

    #[test]
    fn test_fallback_to_default_locale() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        // 使用一个不存在的 locale（不在 supported_locales 中）：
        // sdforge 注册表查不到该 locale 的条目 → 门面回退 default en-US
        let unknown_locale: Locale = "fr-FR".parse().unwrap();

        let msg = bundle.translate(&unknown_locale, "error-database");
        assert_eq!(msg, "Database operation failed. Please try again later.");
    }

    #[test]
    fn test_missing_key_returns_key() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate(&locale, "nonexistent-key");
        assert_eq!(msg, "nonexistent-key");
    }

    #[test]
    fn test_validate_key_consistency() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"]).unwrap();
        let warnings = bundle.validate_key_consistency();
        // en-US 和 zh-CN 应该有相同的 key 集合与占位符
        assert!(
            warnings.is_empty(),
            "Key consistency warnings: {warnings:?}"
        );
    }
}
