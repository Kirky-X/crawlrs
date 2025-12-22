// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 基础设施层模块
///
/// 该模块包含系统的技术实现细节，提供对具体技术的抽象和封装。
/// 基础设施层负责与外部系统的交互，包括数据库、缓存、存储、消息队列等。
///
/// 包含的子模块：
/// - 缓存（cache）：提供缓存功能的实现，如Redis客户端
/// - 数据库（database）：提供数据库连接和实体映射
/// - 指标（metrics）：提供系统监控和性能指标收集
/// - 仓库实现（repositories）：提供领域仓库接口的具体实现
/// - 存储（storage）：提供文件和对象存储功能
///
/// 基础设施层遵循依赖倒置原则，依赖于领域层的抽象接口，
/// 确保领域层保持纯粹的业务逻辑，不受技术实现的影响。
pub mod cache;
pub mod database;
pub mod metrics;
pub mod observability;
pub mod repositories;
pub mod search;
pub mod services;
pub mod storage;
