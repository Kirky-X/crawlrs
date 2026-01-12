// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 缓存管理模块
pub mod cache_manager;

/// 缓存策略模块
pub mod cache_strategy;

/// Redis 客户端模块
#[cfg(feature = "redis-cache")]
pub mod redis_client;
