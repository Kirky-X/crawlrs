// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 通用模块
//!
//! 提供应用程序的通用功能，包括错误类型、常量定义等

pub mod constants;
pub mod error;

pub use constants::*;
pub use error::{AppError, AppResult};