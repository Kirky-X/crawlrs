// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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
