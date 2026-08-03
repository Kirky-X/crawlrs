// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

//! i18n 集成测试
//!
//! 覆盖：locale 文件加载、Accept-Language 协商、错误响应本地化、
//! 回退链、缺失 key 处理、参数替换、key 一致性验证。

use crawlrs::i18n::{
    negotiate_locale, parse_accept_language, t, t_with_args, I18nBundle, Locale,
};
use fluent_bundle::FluentValue;
use std::sync::Arc;

// ===========================================================================
// 辅助函数
// ===========================================================================

fn locales_dir() -> String {
    format!("{}/locales", env!("CARGO_MANIFEST_DIR"))
}

fn test_bundle() -> I18nBundle {
    I18nBundle::load("en-US", &["en-US", "zh-CN"], &locales_dir()).unwrap()
}

fn en_locale() -> Locale {
    "en-US".parse().unwrap()
}

fn zh_locale() -> Locale {
    "zh-CN".parse().unwrap()
}

// ===========================================================================
// 1. Locale 文件加载
// ===========================================================================

#[test]
fn test_load_both_locales() {
    let bundle = test_bundle();
    assert_eq!(bundle.default_locale().to_string(), "en-US");
    assert_eq!(bundle.supported_locales().len(), 2);
}

#[test]
fn test_load_invalid_dir_returns_error() {
    let result = I18nBundle::load("en-US", &["en-US"], "/nonexistent/locales");
    assert!(result.is_err());
}

#[test]
fn test_load_invalid_locale_string() {
    let result = I18nBundle::load("not-a-locale!!!", &["not-a-locale!!!"], &locales_dir());
    assert!(result.is_err());
}

// ===========================================================================
// 2. 基本翻译（无参数）
// ===========================================================================

#[test]
fn test_translate_en_us() {
    let bundle = test_bundle();
    let locale = en_locale();

    assert_eq!(
        bundle.translate(&locale, "error-database"),
        "Database operation failed. Please try again later."
    );
    assert_eq!(bundle.translate(&locale, "error-permission"), "Permission denied.");
    assert_eq!(
        bundle.translate(&locale, "error-timeout"),
        "Request timed out. Please try again later."
    );
}

#[test]
fn test_translate_zh_cn() {
    let bundle = test_bundle();
    let locale = zh_locale();

    assert_eq!(
        bundle.translate(&locale, "error-database"),
        "数据库操作失败，请稍后重试。"
    );
    assert_eq!(bundle.translate(&locale, "error-permission"), "权限不足。");
}

#[test]
fn test_translate_api_keys() {
    let bundle = test_bundle();
    let locale = en_locale();

    assert_eq!(bundle.translate(&locale, "api-access-denied"), "Access denied.");
    assert_eq!(
        bundle.translate(&locale, "api-internal-error"),
        "Internal server error."
    );
    assert_eq!(
        bundle.translate(&locale, "api-task-not-found"),
        "Task not found."
    );
}

#[test]
fn test_translate_domain_error_keys() {
    let bundle = test_bundle();
    let locale = en_locale();

    assert_eq!(
        bundle.translate(&locale, "domain-error-task-expired"),
        "Task expired."
    );
    assert_eq!(
        bundle.translate(&locale, "domain-error-webhook-delivery-failed"),
        "Webhook delivery failed."
    );
    assert_eq!(
        bundle.translate(&locale, "domain-error-llm-extraction-failed"),
        "LLM extraction failed."
    );
}

// ===========================================================================
// 3. 参数替换
// ===========================================================================

#[test]
fn test_translate_with_args_en() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = bundle.translate_with_args(
        &locale,
        "error-validation",
        &[("message", FluentValue::from("invalid email"))],
    );
    // Fluent 在参数周围插入 Unicode 隔离标记（U+2068/U+2069）
    assert!(
        msg.contains("invalid email"),
        "Expected 'invalid email' in: {msg}"
    );
    assert!(msg.starts_with("Validation error:"));
}

#[test]
fn test_translate_with_args_zh() {
    let bundle = test_bundle();
    let locale = zh_locale();

    let msg = bundle.translate_with_args(
        &locale,
        "error-validation",
        &[("message", FluentValue::from("无效邮箱"))],
    );
    assert!(
        msg.contains("无效邮箱"),
        "Expected '无效邮箱' in: {msg}"
    );
    assert!(msg.starts_with("验证错误"));
}

#[test]
fn test_t_helper_function() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = t(&locale, &bundle, "error-permission");
    assert_eq!(msg, "Permission denied.");
}

#[test]
fn test_t_with_args_helper_function() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = t_with_args(
        &locale,
        &bundle,
        "error-not-found",
        &[("resource", FluentValue::from("user 42"))],
    );
    assert!(msg.contains("user 42"), "Expected 'user 42' in: {msg}");
    assert!(msg.starts_with("Resource not found:"));
}

#[test]
fn test_domain_error_args() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = bundle.translate_with_args(
        &locale,
        "domain-error-depth-exceeded",
        &[
            ("max", FluentValue::from(5i64)),
            ("requested", FluentValue::from(10i64)),
        ],
    );
    assert!(msg.contains("5"), "Expected '5' in: {msg}");
    assert!(msg.contains("10"), "Expected '10' in: {msg}");
}

// ===========================================================================
// 4. 回退链
// ===========================================================================

#[test]
fn test_fallback_unknown_locale_to_default() {
    let bundle = test_bundle();
    let unknown: Locale = "fr-FR".parse().unwrap();

    // fr-FR 不在 bundles 中，应回退到 en-US
    let msg = bundle.translate(&unknown, "error-database");
    assert_eq!(msg, "Database operation failed. Please try again later.");
}

#[test]
fn test_fallback_with_args_unknown_locale() {
    let bundle = test_bundle();
    let unknown: Locale = "ja-JP".parse().unwrap();

    let msg = bundle.translate_with_args(
        &unknown,
        "error-validation",
        &[("message", FluentValue::from("bad input"))],
    );
    assert!(msg.contains("bad input"), "Expected fallback to en-US, got: {msg}");
    assert!(msg.starts_with("Validation error:"));
}

// ===========================================================================
// 5. 缺失 key 处理
// ===========================================================================

#[test]
fn test_missing_key_returns_key_itself() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = bundle.translate(&locale, "nonexistent-key-xyz");
    assert_eq!(msg, "nonexistent-key-xyz");
}

#[test]
fn test_missing_key_with_args_returns_key_itself() {
    let bundle = test_bundle();
    let locale = en_locale();

    let msg = bundle.translate_with_args(
        &locale,
        "also-nonexistent",
        &[("foo", FluentValue::from("bar"))],
    );
    assert_eq!(msg, "also-nonexistent");
}

// ===========================================================================
// 6. Accept-Language 解析与协商
// ===========================================================================

#[test]
fn test_parse_accept_language_standard() {
    let locales = parse_accept_language("en-US,en;q=0.9,zh-CN;q=0.8");
    assert_eq!(locales.len(), 3);
    assert_eq!(locales[0].to_string(), "en-US");
    assert_eq!(locales[1].to_string(), "en");
    assert_eq!(locales[2].to_string(), "zh-CN");
}

#[test]
fn test_parse_accept_language_empty() {
    let locales = parse_accept_language("");
    assert!(locales.is_empty());
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
    // "en" 应匹配 "en-US"
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
fn test_negotiate_empty_preferred_returns_default() {
    let preferred: Vec<Locale> = vec![];
    let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
    let default: Locale = "en-US".parse().unwrap();

    let result = negotiate_locale(&preferred, &supported, &default);
    assert_eq!(result.to_string(), "en-US");
}

#[test]
fn test_negotiate_priority_order() {
    // 第一个偏好不匹配，第二个匹配 zh-CN
    let preferred: Vec<Locale> = vec!["fr-FR".parse().unwrap(), "zh-CN".parse().unwrap()];
    let supported: Vec<Locale> = vec!["en-US".parse().unwrap(), "zh-CN".parse().unwrap()];
    let default: Locale = "en-US".parse().unwrap();

    let result = negotiate_locale(&preferred, &supported, &default);
    assert_eq!(result.to_string(), "zh-CN");
}

// ===========================================================================
// 7. Key 一致性验证
// ===========================================================================

#[test]
fn test_key_consistency_no_warnings() {
    let bundle = test_bundle();
    let warnings = bundle.validate_key_consistency();
    assert!(
        warnings.is_empty(),
        "en-US 和 zh-CN 应有相同 key 集合，但有警告: {warnings:?}"
    );
}

// ===========================================================================
// 8. CrawlRsError::user_message_locale 集成测试
// ===========================================================================

#[test]
fn test_crawlrs_error_locale_database_en() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = CrawlRsError::Database(sea_orm::DbErr::Custom("test".to_string()));

    let msg = err.user_message_locale(&locale, &bundle);
    assert_eq!(msg, "Database operation failed. Please try again later.");
}

#[test]
fn test_crawlrs_error_locale_database_zh() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = zh_locale();
    let err = CrawlRsError::Database(sea_orm::DbErr::Custom("test".to_string()));

    let msg = err.user_message_locale(&locale, &bundle);
    assert_eq!(msg, "数据库操作失败，请稍后重试。");
}

#[test]
fn test_crawlrs_error_locale_validation_with_args() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = CrawlRsError::Validation("bad email".to_string());

    let msg = err.user_message_locale(&locale, &bundle);
    assert!(msg.contains("bad email"), "Expected 'bad email' in: {msg}");
    assert!(msg.starts_with("Validation error:"));
}

#[test]
fn test_crawlrs_error_locale_permission_en() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = CrawlRsError::PermissionDenied("no access".to_string());

    let msg = err.user_message_locale(&locale, &bundle);
    assert_eq!(msg, "Permission denied.");
}

#[test]
fn test_crawlrs_error_locale_permission_zh() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = zh_locale();
    let err = CrawlRsError::PermissionDenied("no access".to_string());

    let msg = err.user_message_locale(&locale, &bundle);
    assert_eq!(msg, "权限不足。");
}

#[test]
fn test_crawlrs_error_locale_all_simple_variants_en() {
    use crawlrs::common::error::CrawlRsError;

    let bundle = test_bundle();
    let locale = en_locale();

    // 测试所有无参数（simple translate）变体
    let cases: Vec<(CrawlRsError, &str)> = vec![
        (CrawlRsError::Network("x".into()), "External service unavailable."),
        (CrawlRsError::Config("x".into()), "Configuration error."),
        (CrawlRsError::Timeout("x".into()), "Request timed out."),
        (CrawlRsError::ServiceUnavailable("x".into()), "Service unavailable."),
        (CrawlRsError::RateLimit("x".into()), "Rate limit exceeded."),
        (CrawlRsError::Cache("x".into()), "Cache service unavailable."),
        (CrawlRsError::Task("x".into()), "Task processing error."),
        (CrawlRsError::Other("x".into()), "Internal server error."),
    ];

    for (err, expected_fragment) in cases {
        let msg = err.user_message_locale(&locale, &bundle);
        assert!(
            msg.contains(expected_fragment),
            "Expected '{expected_fragment}' in '{msg}' for error {:?}",
            err
        );
    }
}

// ===========================================================================
// 9. DomainError::user_message_locale 集成测试
// ===========================================================================

#[test]
fn test_domain_error_locale_crawl_config_en() {
    use crawlrs::domain::errors::DomainError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = DomainError::crawl_config("missing field");

    let msg = err.user_message_locale(&locale, &bundle);
    assert!(msg.contains("missing field"), "Expected 'missing field' in: {msg}");
    assert!(
        msg.contains("configuration error") || msg.contains("Crawler configuration"),
        "Expected 'configuration error' in: {msg}"
    );
}

#[test]
fn test_domain_error_locale_crawl_config_zh() {
    use crawlrs::domain::errors::DomainError;

    let bundle = test_bundle();
    let locale = zh_locale();
    let err = DomainError::crawl_config("缺少字段");

    let msg = err.user_message_locale(&locale, &bundle);
    assert!(msg.contains("缺少字段"), "Expected '缺少字段' in: {msg}");
}

#[test]
fn test_domain_error_locale_insufficient_credits_en() {
    use crawlrs::domain::errors::DomainError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = DomainError::InsufficientCredits {
        required: 100,
        available: 50,
    };

    let msg = err.user_message_locale(&locale, &bundle);
    assert!(msg.contains("100"), "Expected '100' in: {msg}");
    assert!(msg.contains("50"), "Expected '50' in: {msg}");
    assert!(
        msg.contains("Insufficient credits") || msg.contains("insufficient"),
        "Expected 'Insufficient credits' in: {msg}"
    );
}

#[test]
fn test_domain_error_locale_task_expired_en() {
    use crawlrs::domain::errors::DomainError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = DomainError::TaskExpired {
        created_at: chrono::Utc::now(),
        timeout_seconds: 60,
    };

    let msg = err.user_message_locale(&locale, &bundle);
    assert_eq!(msg, "Task expired.");
}

#[test]
fn test_domain_error_locale_validation_en() {
    use crawlrs::domain::errors::DomainError;

    let bundle = test_bundle();
    let locale = en_locale();
    let err = DomainError::validation("email", "format is wrong");

    let msg = err.user_message_locale(&locale, &bundle);
    assert!(msg.contains("email"), "Expected 'email' in: {msg}");
    assert!(
        msg.contains("format is wrong"),
        "Expected 'format is wrong' in: {msg}"
    );
}

// ===========================================================================
// 10. Arc<I18nBundle> 线程安全验证
// ===========================================================================

#[test]
fn test_bundle_is_send_sync() {
    let bundle = test_bundle();
    let arc_bundle: Arc<I18nBundle> = Arc::new(bundle);

    // 验证可以在线程间共享
    let handle = std::thread::spawn(move || {
        let locale: Locale = "en-US".parse().unwrap();
        arc_bundle.translate(&locale, "error-database")
    });

    let result = handle.join().unwrap();
    assert_eq!(result, "Database operation failed. Please try again later.");
}

#[test]
fn test_concurrent_translation() {
    let bundle = Arc::new(test_bundle());
    let mut handles = vec![];

    for _ in 0..4 {
        let b = Arc::clone(&bundle);
        handles.push(std::thread::spawn(move || {
            let locale: Locale = "en-US".parse().unwrap();
            for _ in 0..100 {
                let msg = b.translate(&locale, "error-database");
                assert_eq!(msg, "Database operation failed. Please try again later.");
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
