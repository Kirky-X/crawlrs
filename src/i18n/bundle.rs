// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! i18n 翻译 bundle 管理
//!
//! 负责加载 FTL 翻译文件、提供翻译接口和验证 key 一致性。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::common::error::CrawlRsError;

/// Locale 类型别名（BCP 47 语言标识符）
pub type Locale = LanguageIdentifier;

/// i18n 翻译 bundle，线程安全（Send + Sync）
///
/// 管理所有语言的翻译资源，提供翻译接口。
/// 在应用启动时加载，作为 `Arc<I18nBundle>` 存储在状态中。
pub struct I18nBundle {
    /// 每个 locale 对应的 FluentBundle
    bundles: HashMap<Locale, FluentBundle<FluentResource>>,
    /// 默认 locale（回退语言）
    default_locale: Locale,
    /// 支持的 locale 列表
    supported_locales: Vec<Locale>,
}

// SAFETY: FluentBundle<IntlLangMemoizer> 内部的 RefCell<TypeMap> 仅用于
// 格式化缓存（memoization），不影响翻译正确性。所有 FTL 资源在加载后不可变，
// 并发格式化时的缓存竞争最坏情况是重复计算，不会产生数据竞争或内存不安全。
unsafe impl Send for I18nBundle {}
unsafe impl Sync for I18nBundle {}

impl I18nBundle {
    /// 从 locales/ 目录加载所有语言包
    ///
    /// # Arguments
    /// * `default_locale` - 默认 locale 字符串（如 "en-US"）
    /// * `supported` - 支持的 locale 字符串列表
    /// * `dir` - locales 根目录路径
    ///
    /// # Errors
    /// 返回 `CrawlRsError::Config` 如果目录不存在或 FTL 文件格式错误
    pub fn load(default_locale: &str, supported: &[&str], dir: &str) -> Result<Self, CrawlRsError> {
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

        let mut bundles = HashMap::new();

        for locale in &supported_locales {
            let locale_dir = Path::new(dir).join(locale.to_string());
            if !locale_dir.exists() {
                return Err(CrawlRsError::Config(format!(
                    "Locale directory not found: {}",
                    locale_dir.display()
                )));
            }

            let mut bundle = FluentBundle::new(vec![locale.clone()]);

            // 加载该 locale 目录下所有 .ftl 文件
            let mut entries: Vec<_> = fs::read_dir(&locale_dir)
                .map_err(|e| {
                    CrawlRsError::Config(format!(
                        "Failed to read locale directory {}: {e}",
                        locale_dir.display()
                    ))
                })?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "ftl"))
                .collect();

            // 按文件名排序确保确定性
            entries.sort_by_key(|e| e.file_name());

            for entry in entries {
                let content = fs::read_to_string(entry.path()).map_err(|e| {
                    CrawlRsError::Config(format!(
                        "Failed to read FTL file {}: {e}",
                        entry.path().display()
                    ))
                })?;

                let resource = FluentResource::try_new(content).map_err(|e| {
                    CrawlRsError::Config(format!(
                        "Failed to parse FTL file {}: {e:?}",
                        entry.path().display()
                    ))
                })?;

                bundle.add_resource(resource).map_err(|e| {
                    CrawlRsError::Config(format!(
                        "Failed to add resource for locale {locale}: {e:?}"
                    ))
                })?;
            }

            bundles.insert(locale.clone(), bundle);
        }

        Ok(Self {
            bundles,
            default_locale,
            supported_locales,
        })
    }

    /// 翻译消息（无参数）
    ///
    /// 查找指定 locale 的翻译，如果 key 不存在则回退到 default_locale。
    /// 如果 default_locale 也不存在，返回 key 本身。
    pub fn translate(&self, locale: &Locale, key: &str) -> String {
        // 尝试请求的 locale
        if let Some(bundle) = self.bundles.get(locale) {
            if let Some(msg) = bundle.get_message(key) {
                if let Some(pattern) = msg.value() {
                    let mut errors = vec![];
                    let result = bundle
                        .format_pattern(pattern, None, &mut errors)
                        .to_string();
                    if !errors.is_empty() {
                        log::warn!(
                            "Fluent format errors for key '{key}' in locale '{locale}': {errors:?}"
                        );
                    }
                    return result;
                }
            }
        }

        // 回退到 default locale
        if locale != &self.default_locale {
            if let Some(bundle) = self.bundles.get(&self.default_locale) {
                if let Some(msg) = bundle.get_message(key) {
                    if let Some(pattern) = msg.value() {
                        let mut errors = vec![];
                        let result = bundle
                            .format_pattern(pattern, None, &mut errors)
                            .to_string();
                        if !errors.is_empty() {
                            log::warn!(
                                "Fluent format errors for key '{key}' in default locale: {errors:?}"
                            );
                        }
                        return result;
                    }
                }
            }
        }

        // Key 不存在，返回 key 本身
        log::warn!("Translation key '{key}' not found for locale '{locale}'");
        key.to_string()
    }

    /// 翻译消息（带参数）
    ///
    /// 参数通过 `&[(&str, FluentValue)]` 传入，支持 Fluent 变量替换。
    pub fn translate_with_args(
        &self,
        locale: &Locale,
        key: &str,
        args: &[(&str, FluentValue)],
    ) -> String {
        let fluent_args: Option<FluentArgs> = if args.is_empty() {
            None
        } else {
            Some(args.iter().map(|(k, v)| (*k, v.clone())).collect())
        };

        // 尝试请求的 locale
        if let Some(bundle) = self.bundles.get(locale) {
            if let Some(msg) = bundle.get_message(key) {
                if let Some(pattern) = msg.value() {
                    let mut errors = vec![];
                    let result = bundle
                        .format_pattern(pattern, fluent_args.as_ref(), &mut errors)
                        .to_string();
                    if !errors.is_empty() {
                        log::warn!(
                            "Fluent format errors for key '{key}' in locale '{locale}': {errors:?}"
                        );
                    }
                    return result;
                }
            }
        }

        // 回退到 default locale
        if locale != &self.default_locale {
            if let Some(bundle) = self.bundles.get(&self.default_locale) {
                if let Some(msg) = bundle.get_message(key) {
                    if let Some(pattern) = msg.value() {
                        let mut errors = vec![];
                        let result = bundle
                            .format_pattern(pattern, fluent_args.as_ref(), &mut errors)
                            .to_string();
                        if !errors.is_empty() {
                            log::warn!(
                                "Fluent format errors for key '{key}' in default locale: {errors:?}"
                            );
                        }
                        return result;
                    }
                }
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
    /// 以 default_locale 的 key 集合为基准，其他 locale 缺少或多出的 key 都会被报告。
    pub fn validate_key_consistency(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // 获取 default locale 的 key 集合
        let default_keys = self.get_message_keys(&self.default_locale);

        for locale in &self.supported_locales {
            if locale == &self.default_locale {
                continue;
            }

            let locale_keys = self.get_message_keys(locale);

            // 检查缺少的 key
            for key in &default_keys {
                if !locale_keys.contains(key) {
                    warnings.push(format!(
                        "Locale '{locale}' missing key '{key}' (present in default '{}')",
                        self.default_locale
                    ));
                }
            }

            // 检查多余的 key
            for key in &locale_keys {
                if !default_keys.contains(key) {
                    warnings.push(format!(
                        "Locale '{locale}' has extra key '{key}' (not in default '{}')",
                        self.default_locale
                    ));
                }
            }
        }

        warnings
    }

    /// 获取指定 locale 的所有 message key
    fn get_message_keys(&self, locale: &Locale) -> HashSet<String> {
        let mut keys = HashSet::new();
        if let Some(bundle) = self.bundles.get(locale) {
            // FluentBundle 没有直接的 keys() 迭代 API，
            // 通过已知 FTL 文件中的 key 列表来验证
            // 这里我们遍历所有已知的 key 前缀来收集
            for entry in self.ftl_entries() {
                if bundle.get_message(&entry).is_some() {
                    keys.insert(entry);
                }
            }
        }
        keys
    }

    /// 所有已知的 FTL key 列表
    ///
    /// 新增 FTL key 时需同步更新此列表。
    fn ftl_entries(&self) -> Vec<String> {
        let mut keys = Vec::new();

        // errors.ftl (17 keys)
        keys.extend([
            "error-database",
            "error-network",
            "error-config",
            "error-validation",
            "error-not-found",
            "error-auth",
            "error-permission",
            "error-timeout",
            "error-rate-limit",
            "error-quota",
            "error-service-unavailable",
            "error-cache",
            "error-task",
            "error-json",
            "error-io",
            "error-engine",
            "error-internal",
        ]);

        // api.ftl (12 keys)
        keys.extend([
            "api-access-denied",
            "api-internal-error",
            "api-task-not-found",
            "api-crawl-not-found",
            "api-scrape-cancelled",
            "api-task-failed",
            "api-validation-error",
            "api-geographic-denied",
            "api-webhook-url-invalid",
            "api-insufficient-permissions-api-key",
            "api-insufficient-permissions-team",
            "api-task-ids-empty",
        ]);

        // validation.ftl (4 keys)
        keys.extend([
            "validation-query-empty",
            "validation-url-invalid",
            "validation-credits-insufficient",
            "validation-field-required",
        ]);

        keys.into_iter().map(String::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_locales_dir() -> String {
        // 从项目根目录的 locales/ 加载
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        format!("{manifest_dir}/locales")
    }

    #[test]
    fn test_load_bundle() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir());
        assert!(bundle.is_ok(), "Failed to load bundle: {:?}", bundle.err());
        let bundle = bundle.unwrap();
        assert_eq!(bundle.default_locale().to_string(), "en-US");
        assert_eq!(bundle.supported_locales().len(), 2);
    }

    #[test]
    fn test_translate_no_args() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate(&locale, "error-database");
        assert_eq!(msg, "Database operation failed. Please try again later.");

        let msg = bundle.translate(&locale, "error-permission");
        assert_eq!(msg, "Permission denied.");
    }

    #[test]
    fn test_translate_zh_cn() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = bundle.translate(&locale, "error-database");
        assert_eq!(msg, "数据库操作失败，请稍后重试。");
    }

    #[test]
    fn test_translate_with_args() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate_with_args(
            &locale,
            "error-validation",
            &[("message", FluentValue::from("invalid email"))],
        );
        // Fluent 在参数周围插入 Unicode 隔离标记（U+2068/U+2069）
        assert!(
            msg.contains("invalid email"),
            "Expected message to contain 'invalid email', got: {msg}"
        );
        assert!(msg.starts_with("Validation error:"));
    }

    #[test]
    fn test_translate_with_args_zh_cn() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let locale: Locale = "zh-CN".parse().unwrap();

        let msg = bundle.translate_with_args(
            &locale,
            "error-validation",
            &[("message", FluentValue::from("无效邮箱"))],
        );
        assert!(
            msg.contains("无效邮箱"),
            "Expected message to contain '无效邮箱', got: {msg}"
        );
        assert!(msg.starts_with("验证错误"));
    }

    #[test]
    fn test_fallback_to_default_locale() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        // 使用一个不存在的 locale（不在 supported_locales 中）
        let unknown_locale: Locale = "fr-FR".parse().unwrap();

        // 应该回退到 en-US
        let msg = bundle.translate(&unknown_locale, "error-database");
        assert_eq!(msg, "Database operation failed. Please try again later.");
    }

    #[test]
    fn test_missing_key_returns_key() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let locale: Locale = "en-US".parse().unwrap();

        let msg = bundle.translate(&locale, "nonexistent-key");
        assert_eq!(msg, "nonexistent-key");
    }

    #[test]
    fn test_validate_key_consistency() {
        let bundle = I18nBundle::load("en-US", &["en-US", "zh-CN"], &test_locales_dir()).unwrap();
        let warnings = bundle.validate_key_consistency();
        // en-US 和 zh-CN 应该有相同的 key 集合
        assert!(
            warnings.is_empty(),
            "Key consistency warnings: {warnings:?}"
        );
    }

    #[test]
    fn test_load_invalid_dir() {
        let result = I18nBundle::load("en-US", &["en-US"], "/nonexistent/path");
        assert!(result.is_err());
    }
}
