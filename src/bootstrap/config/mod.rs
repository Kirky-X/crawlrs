// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Configuration loading, validation, and port detection.
//!
//! 此模块负责在应用启动早期进行配置和环境变量的安全验证

// ---------------------------------------------------------------------------
// 子模块声明
// ---------------------------------------------------------------------------

pub mod config_loader;
pub mod config_validator;

// ---------------------------------------------------------------------------
// Re-export — 保持所有原有公共路径可用
// ---------------------------------------------------------------------------

pub use config_loader::{detect_available_port, load_settings};
pub use config_validator::{load_and_configure, validate_environment, validate_security};
