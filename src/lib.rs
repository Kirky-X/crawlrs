// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// 数据库后端特性说明：
// - 通过 db-postgres / db-sqlite / db-mysql 三组特性选择后端（每组透传
//   dbnexus + garrison + limiteron + inklog 的驱动选择），互斥由 dbnexus
//   编译期强制（embedded 与 server-side 不可混，postgres/mysql 互斥）
// - platform 面必须恰好启用一个 db-* 特性（default 已附带 db-postgres）
// - 旧 dbnexus-sqlite 已于早期移除，现经 db-sqlite 组重新支持

// platform 无驱动时 Settings/仓储代码无法编译——给出明确的单一错误而非级联报错
#[cfg(all(
    feature = "platform",
    not(any(feature = "db-postgres", feature = "db-sqlite", feature = "db-mysql"))
))]
compile_error!(
    "platform requires exactly one database driver feature: db-postgres / db-sqlite / db-mysql \
     (see the db-* feature groups in Cargo.toml)"
);

// db-* 互斥守卫：比 dbnexus 的深层 compile_error 更早、更清晰（default 已含 db-postgres，
// 追加 db-sqlite/db-mysql 时最先在此报错）
#[cfg(all(
    feature = "db-postgres",
    any(feature = "db-sqlite", feature = "db-mysql")
))]
compile_error!(
    "database driver features are mutually exclusive: pick db-postgres OR db-sqlite OR db-mysql"
);
#[cfg(all(feature = "db-sqlite", feature = "db-mysql"))]
compile_error!(
    "database driver features are mutually exclusive: pick db-postgres OR db-sqlite OR db-mysql"
);

/// 通用模块
///
/// 提供应用程序的通用功能，包括错误类型、常量定义等
pub mod common;

/// 应用程序模块
///
/// 包含应用程序的核心业务逻辑和用例
pub mod application;

/// 配置模块
///
/// 处理应用程序的配置设置和环境变量
pub mod config;

/// 领域模块
///
/// 包含核心业务实体、服务和仓库接口
pub mod domain;

/// 引擎模块
///
/// 实现各种网页爬取和抓取引擎
pub mod engines;

/// 基础设施模块
///
/// 提供外部服务集成，如数据库、缓存、存储等
pub mod infrastructure;

/// 表示层模块
///
/// 处理HTTP请求和响应，包括路由、处理器和中间件
#[cfg(feature = "platform")]
pub mod presentation;

/// 工具模块
///
/// 提供通用的工具函数和辅助功能
pub mod utils;

/// 工作器模块
///
/// 实现后台任务处理和工作器管理
#[cfg(feature = "platform")]
pub mod workers;

/// 搜索模块
///
/// 提供统一的搜索引擎客户端和多种搜索引擎实现
pub mod search;

/// 库面模块（agent-lib feature）
///
/// 面向嵌入式/agent 的最小库面：`search()` 与 `fetch()`(→Markdown)。
/// 仅依赖 engines/search/content 等轻量模块，不编译 DB/服务端。
#[cfg(feature = "agent-lib")]
pub mod agent_lib;

/// 队列模块
///
/// 提供任务队列接口和实现
#[cfg(feature = "platform")]
pub mod queue;

/// 引导模块
///
/// 提供应用程序初始化的结构化方式
#[cfg(feature = "platform")]
pub mod bootstrap;

/// 依赖注入模块
///
/// 提供基于 trait-kit 的依赖注入框架
#[cfg(feature = "platform")]
pub mod di;

/// 国际化模块
///
/// 提供多语言翻译支持（基于 Mozilla Fluent 系统）
pub mod i18n;

/// 共享测试工具（仅 `cargo test` 时编译）
///
/// 提供 `CapturingLogger` + `ensure_debug_logger()` 等工具，
/// 消除多个 `#[cfg(test)]` 模块中的重复 logger 定义。
#[cfg(test)]
pub mod test_utils;
