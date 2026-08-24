// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 国际化（i18n）模块
//!
//! 基于 Mozilla Fluent 系统提供多语言翻译支持。
//! 翻译文件位于项目根目录 `locales/` 下，按 locale 子目录组织。

mod bundle;
mod locale;
#[cfg(any(feature = "platform", feature = "web-axum"))]
mod middleware;
mod translate;

pub use bundle::{I18nBundle, Locale};
pub use locale::{negotiate_locale, parse_accept_language};
#[cfg(any(feature = "platform", feature = "web-axum"))]
pub use middleware::i18n_middleware;
pub use translate::{t, t_with_args};
