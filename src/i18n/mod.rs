// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 国际化（i18n）模块
//!
//! 基于 Mozilla Fluent 系统提供多语言翻译支持。
//! 翻译文件位于项目根目录 `locales/` 下，按 locale 子目录组织。
//!
//! 另提供启动期全局束（[`init_startup_i18n`] / [`startup_i18n`]）：主程序在
//! 读取配置后立即初始化，供无请求上下文的对外出口（`CrawlRsError` 错误响应、
//! RAG 配置校验消息、worker 运维日志）经 Fluent 输出。

mod bundle;
mod locale;
#[cfg(feature = "platform")]
mod middleware;
mod translate;

use std::sync::{Arc, OnceLock};

use crate::config::settings::I18nSettings;

pub use bundle::{I18nBundle, Locale};
pub use locale::{
    detect_locale_from, detect_system_locale, negotiate_locale, parse_accept_language,
    resolve_startup_default_locale, BUILTIN_DEFAULT_LOCALE,
};
#[cfg(feature = "platform")]
pub use middleware::i18n_middleware;
pub use translate::{t, t_with_args, tr_log, tr_log_args};

/// 启动期全局翻译束（主程序启动时初始化一次，进程内只读）
static STARTUP_BUNDLE: OnceLock<Arc<I18nBundle>> = OnceLock::new();
/// 启动期全局默认 locale（检测链决议结果；先于 bundle 写入）
static STARTUP_LOCALE: OnceLock<Locale> = OnceLock::new();

/// 从 i18n 配置构建启动 bundle（default_locale 经检测链决议）
///
/// 加载失败（locales 目录缺失/FTL 解析错误）返回 `None`，调用方保持未初始化
/// 状态，各出口回退英文规范串/键名。
pub fn build_startup_bundle(cfg: &I18nSettings) -> Option<Arc<I18nBundle>> {
    let supported: Vec<&str> = cfg.supported_locales.iter().map(|s| s.as_str()).collect();
    let default_locale = resolve_startup_default_locale(&cfg.default_locale, &supported);
    I18nBundle::load(&default_locale.to_string(), &supported, &cfg.locales_dir)
        .map(Arc::new)
        .ok()
}

/// 幂等初始化启动期全局束（`OnceLock`，重复调用为 no-op）
pub fn init_startup_i18n(bundle: Arc<I18nBundle>) {
    let _ = STARTUP_LOCALE.set(bundle.default_locale().clone());
    let _ = STARTUP_BUNDLE.set(bundle);
}

/// 只读访问启动期全局束（未初始化时返回 `None`）
pub fn startup_i18n() -> Option<(&'static Locale, &'static Arc<I18nBundle>)> {
    let locale = STARTUP_LOCALE.get()?;
    let bundle = STARTUP_BUNDLE.get()?;
    Some((locale, bundle))
}
