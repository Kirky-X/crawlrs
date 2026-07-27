// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! JS 注入模块（design.md §6，R-jsrender-002）
//!
//! 在浏览器页面导航前 / 加载后注入指定 JavaScript 脚本：
//! - `stealth`：覆盖 `navigator.webdriver` 等反爬指纹属性
//! - `cleanup`：移除 consent 弹窗 / 遮罩元素 / 展平 shadow DOM
//!
//! Gated by `engine-playwright` feature（依赖 chromiumoxide::Page）。

mod injector;

pub use injector::{InjectPhase, JsInjector};
