// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// 数据库后端特性说明：
// - 当前仅支持 PostgreSQL（通过 dbnexus-postgres 特性启用）
// - 原 dbnexus-sqlite 特性已删除（不再支持 SQLite）
// - 若未来需要重新支持 SQLite，需重新引入 dbnexus-sqlite 特性并恢复相关测试
// 注意：未启用任何 db 特性时，依赖 Settings/仓储的代码将不可用，
// 但 lib 本身仍可编译（用于仅使用 engine/search 等子模块的场景）

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
pub mod presentation;

/// 工具模块
///
/// 提供通用的工具函数和辅助功能
pub mod utils;

/// 工作器模块
///
/// 实现后台任务处理和工作器管理
pub mod workers;

/// 搜索模块
///
/// 提供统一的搜索引擎客户端和多种搜索引擎实现
pub mod search;

/// 队列模块
///
/// 提供任务队列接口和实现
pub mod queue;

/// 引导模块
///
/// 提供应用程序初始化的结构化方式
pub mod bootstrap;

/// 依赖注入模块
///
/// 提供基于 trait-kit 的依赖注入框架
pub mod di;
