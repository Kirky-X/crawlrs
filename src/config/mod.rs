// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置模块
//!
//! 处理应用程序的配置设置，包括数据库、Redis、服务器等配置
//! 配置结构体按功能分组到子模块中：

pub mod app;
pub mod engines;
pub mod llm;
pub mod logging;
pub mod runtime;
pub mod search;

// 重新导出子模块中的类型，保持向后兼容
pub use app::ConcurrencySettings;
pub use app::DatabaseSettings;
pub use app::RateLimitingSettings;
pub use app::RedisSettings;
pub use app::ServerSettings;

pub use engines::{EngineSettings, FireCdpSettings, FireTlsSettings, FlareSolverrSettings};

pub use logging::{ConsoleLoggingSettings, FileLoggingSettings, LoggingSettings};

pub use search::BingSearchSettings;
pub use search::SearchSettings;

pub use llm::LLMSettings;

pub use runtime::RuntimeConfig;
pub use settings::{
    CacheSettings, ProxySettings, TimeoutSettings, WebhookSettings, WorkerCount, WorkerSettings,
};

// 主配置结构体
pub mod settings;
pub use settings::Settings;
pub use settings::TrustedProxySettings;
