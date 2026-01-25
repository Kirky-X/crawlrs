// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in project root for full license information.

/// 缓存管理模块
pub mod cache_manager;

/// 缓存策略模块
pub mod cache_strategy;
pub use cache_strategy::{CacheStrategyConfig, CacheType};

/// 缓存统计收集器模块
pub mod stats_collector;

/// 缓存类型定义模块
pub mod types;

/// Redis 客户端模块
#[cfg(feature = "redis-cache")]
pub mod redis_client;
