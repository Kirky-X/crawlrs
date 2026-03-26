// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 工作器常量
//!
//! 定义工作器模块使用的常量
//!
//! 注意：此文件中的 CONCURRENCY_CONTROL_LUA 脚本保留用于 Worker 层的分布式并发控制。
//! 虽然 limiteron 库提供了速率限制功能，但其 ConcurrencyLimiter 使用本地信号量，
//! 不适合分布式 Worker 场景。因此 Worker 层的并发控制继续使用 Redis Lua 脚本。

/// 并发控制 Lua 脚本
///
/// 使用原子操作管理任务并发控制，减少 Redis 调用次数
/// 支持心跳机制和动态限制
///
/// 该脚本实现以下功能：
/// 1. 清理过期的任务（基于时间戳）
/// 2. 从 Redis 获取限制值或使用默认值
/// 3. 检查任务是否已在集合中（心跳场景）
/// 4. 检查当前计数并在限制内获取许可
pub const CONCURRENCY_CONTROL_LUA: &str = r#"
local active_key = KEYS[1]
local limit_key = KEYS[2]
local task_id = ARGV[1]
local score = tonumber(ARGV[2])
local stale_threshold = tonumber(ARGV[3])
local default_limit = tonumber(ARGV[4])

-- 1. Cleanup stale tasks (older than threshold)
redis.call('ZREMRANGEBYSCORE', active_key, '-inf', stale_threshold)

-- 2. Get limit from Redis or use default
local limit = tonumber(redis.call('GET', limit_key) or default_limit)

-- 3. Check if task is already in set (heartbeat case)
if redis.call('ZSCORE', active_key, task_id) then
    redis.call('ZADD', active_key, score, task_id)
    return 1
end

-- 4. Check current count and acquire if within limit
local count = redis.call('ZCARD', active_key)
if count < limit then
    redis.call('ZADD', active_key, score, task_id)
    return 1
else
    return 0
end
"#;
