// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 共享测试工具：CapturingLogger + ensure_debug_logger()
//!
//! 多个 `#[cfg(test)]` 模块需要安装全局 logger 以覆盖 `log::debug!`/`log::error!`
//! 等宏的格式化参数求值（代码覆盖率目的）。此模块提供统一的 `CapturingLogger`
//! 和 `ensure_debug_logger()` 函数，消除 10+ 处重复定义。
//!
//! # 用法
//!
//! ```ignore
//! // 在 #[cfg(test)] mod tests 内：
//! use crate::test_utils::ensure_debug_logger;
//!
//! #[test]
//! fn test_something() {
//!     ensure_debug_logger();
//!     // ... 触发 log::debug! 的代码 ...
//! }
//! ```

use log::{LevelFilter, Log, Metadata, Record};
use std::sync::Once;

/// 空操作 logger，接受所有日志级别以覆盖宏参数求值。
///
/// `log` 方法为空实现——不存储/输出日志，仅确保 `log::debug!`/`log::error!`
/// 等宏的格式化参数被求值（代码覆盖率工具统计需要）。
struct CapturingLogger;

impl Log for CapturingLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn log(&self, _record: &Record) {}
    fn flush(&self) {}
}

static LOGGER_INIT: Once = Once::new();

/// 安装全局 debug 级别 logger（幂等，多次调用安全）。
///
/// 内部通过 `std::sync::Once` 保证 `log::set_logger` 只调用一次。
/// `log::set_max_level(Debug)` 使 `log::debug!`/`log::info!`/`log::warn!`/
/// `log::error!` 的格式化参数被求值，覆盖代码覆盖率工具统计的"宏展开行"。
///
/// # Panics
///
/// 不会 panic——`log::set_logger` 返回 `Result`，此处忽略错误
/// （其他测试模块可能已先安装 logger）。
pub fn ensure_debug_logger() {
    LOGGER_INIT.call_once(|| {
        static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
        let _ = log::set_logger(&CAPTURING_LOGGER);
        log::set_max_level(LevelFilter::Debug);
    });
}
