// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 领域层模块
///
/// 该模块包含系统的核心业务逻辑，包括：
/// - 领域模型（models）：核心业务实体和数据结构
/// - 仓库接口（repositories）：数据持久化抽象接口
/// - 服务（services）：领域服务和业务规则
/// - 用例（use_cases）：具体的业务操作和流程
///
/// 领域层是系统的核心，不依赖于任何外部实现，
/// 体现了纯粹的业务逻辑和业务规则。
pub mod models;
pub mod repositories;
pub mod search;
pub mod services;
pub mod use_cases;
