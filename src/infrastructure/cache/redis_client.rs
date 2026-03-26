// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use deadpool_redis::{Config as DeadpoolConfig, Pool, Runtime, Timeouts};
use redis::AsyncCommands;
use std::time::Duration;

/// Redis客户端配置
#[derive(Debug, Clone)]
pub struct RedisClientConfig {
    /// 最大连接数
    pub max_connections: u32,
    /// 连接超时时间（秒）- 用于创建新连接和等待可用连接
    pub connection_timeout: u64,
    /// 空闲连接回收超时时间（秒）- 用于回收连接时的超时
    pub recycle_timeout: u64,
}

impl Default for RedisClientConfig {
    fn default() -> Self {
        Self {
            max_connections: 20,
            connection_timeout: 10,
            recycle_timeout: 5,
        }
    }
}

/// Redis客户端
///
/// 提供对Redis数据库的异步操作接口，使用连接池管理连接
#[derive(Clone)]
pub struct RedisClient {
    /// Redis连接池
    pool: Pool,
    /// 配置
    config: RedisClientConfig,
}

impl RedisClient {
    /// 创建新的Redis客户端实例（使用默认配置）
    ///
    /// # 参数
    ///
    /// * `redis_url` - Redis连接URL
    ///
    /// # 返回值
    ///
    /// * `Ok(RedisClient)` - Redis客户端实例
    /// * `Err(anyhow::Error)` - 创建过程中出现的错误
    pub fn new(redis_url: &str) -> Result<Self> {
        Self::with_config(redis_url, RedisClientConfig::default())
    }

    /// 使用自定义配置创建Redis客户端实例
    ///
    /// # 参数
    ///
    /// * `redis_url` - Redis连接URL
    /// * `config` - Redis客户端配置
    ///
    /// # 返回值
    ///
    /// * `Ok(RedisClient)` - Redis客户端实例
    /// * `Err(anyhow::Error)` - 创建过程中出现的错误
    ///
    /// # 配置说明
    ///
    /// 连接池配置：
    /// - max_connections: 连接池最大连接数
    /// - connection_timeout: 创建新连接和等待可用连接的超时时间
    /// - recycle_timeout: 回收连接时的超时时间
    pub fn with_config(redis_url: &str, config: RedisClientConfig) -> Result<Self> {
        let cfg = DeadpoolConfig::from_url(redis_url);

        // 配置超时参数
        let timeouts = Timeouts {
            wait: Some(Duration::from_secs(config.connection_timeout)),
            create: Some(Duration::from_secs(config.connection_timeout)),
            recycle: Some(Duration::from_secs(config.recycle_timeout)),
        };

        let pool = cfg
            .builder()?
            .max_size(config.max_connections as usize)
            .timeouts(timeouts)
            .runtime(Runtime::Tokio1)
            .build()?;

        Ok(Self { pool, config })
    }

    /// 从 Settings 创建 Redis 客户端
    ///
    /// # 参数
    ///
    /// * `settings` - Redis 配置设置
    ///
    /// # 返回值
    ///
    /// * `Ok(RedisClient)` - Redis客户端实例
    /// * `Err(anyhow::Error)` - 创建过程中出现的错误
    ///
    /// # 配置映射
    ///
    /// - max_connections -> max_connections
    /// - connection_timeout -> connection_timeout (用于创建和等待连接)
    /// - idle_timeout -> recycle_timeout (用于连接回收)
    /// - min_connections: 不支持（deadpool 不提供连接预热功能）
    pub fn from_settings(settings: &crate::config::RedisSettings) -> Result<Self> {
        let config = RedisClientConfig {
            max_connections: settings.max_connections(),
            connection_timeout: settings.connection_timeout(),
            recycle_timeout: settings.idle_timeout(), // 使用 idle_timeout 作为 recycle 超时
        };
        Self::with_config(settings.url(), config)
    }

    /// 获取连接池状态
    pub fn pool_status(&self) -> deadpool_redis::Status {
        self.pool.status()
    }

    /// 获取配置
    pub fn config(&self) -> &RedisClientConfig {
        &self.config
    }

    /// 获取指定键的值
    ///
    /// # 参数
    ///
    /// * `key` - 键
    ///
    /// # 返回值
    ///
    /// * `Ok(Option<String>)` - 键对应的值，如果不存在则返回None
    /// * `Err(anyhow::Error)` - 获取过程中出现的错误
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.pool.get().await?;
        let value: Option<String> = conn.get(key).await?;
        Ok(value)
    }

    /// 批量获取多个键的值 (MGET)
    ///
    /// 性能优化：使用 Redis MGET 命令在单次网络往返中获取多个键
    ///
    /// # 参数
    ///
    /// * `keys` - 键列表
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<Option<String>>)` - 每个键对应的值，不存在的键返回 None
    /// * `Err(anyhow::Error)` - 获取过程中出现的错误
    pub async fn mget(&self, keys: &[String]) -> Result<Vec<Option<String>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.pool.get().await?;

        // 使用 MGET 命令批量获取
        let mut cmd = redis::cmd("MGET");
        cmd.arg(keys);
        let values: Vec<Option<String>> = cmd.query_async(&mut conn).await?;
        Ok(values)
    }

    /// 设置键值对并指定过期时间
    ///
    /// # 参数
    ///
    /// * `key` - 键
    /// * `value` - 值
    /// * `ttl_seconds` - 过期时间（秒）
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 设置成功
    /// * `Err(anyhow::Error)` - 设置过程中出现的错误
    pub async fn set(&self, key: &str, value: &str, ttl_seconds: usize) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.set_ex::<_, _, ()>(key, value, ttl_seconds as u64)
            .await?;
        Ok(())
    }

    /// 永久设置键值对
    ///
    /// # 参数
    ///
    /// * `key` - 键
    /// * `value` - 值
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 设置成功
    /// * `Err(anyhow::Error)` - 设置过程中出现的错误
    pub async fn set_forever(&self, key: &str, value: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.set::<_, _, ()>(key, value).await?;
        Ok(())
    }

    /// 设置键的过期时间
    ///
    /// # 参数
    ///
    /// * `key` - 键
    /// * `seconds` - 过期时间（秒）
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 设置成功
    /// * `Err(anyhow::Error)` - 设置过程中出现的错误
    pub async fn expire(&self, key: &str, seconds: usize) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.expire::<_, ()>(key, seconds as i64).await?;
        Ok(())
    }

    /// 向有序集合添加成员
    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.zadd::<_, _, _, ()>(key, member, score).await?;
        Ok(())
    }

    /// 从有序集合移除成员
    pub async fn zrem(&self, key: &str, member: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.zrem::<_, _, ()>(key, member).await?;
        Ok(())
    }

    /// 获取有序集合的成员数量
    pub async fn zcard(&self, key: &str) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let count: u64 = conn.zcard(key).await?;
        Ok(count)
    }

    /// 移除有序集合中指定分数范围的成员
    pub async fn zrembyscore(&self, key: &str, min: f64, max: f64) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let count: u64 = conn.zrembyscore(key, min, max).await?;
        Ok(count)
    }

    /// 获取有序集合中成员的排名（从0开始，按分数从小到大）
    pub async fn zrank(&self, key: &str, member: &str) -> Result<Option<usize>> {
        let mut conn = self.pool.get().await?;
        let rank: Option<usize> = conn.zrank(key, member).await?;
        Ok(rank)
    }

    /// 增加键的值
    ///
    /// # 参数
    ///
    /// * `key` - 键
    ///
    /// # 返回值
    ///
    /// * `Ok(i64)` - 增加后的值
    /// * `Err(anyhow::Error)` - 增加过程中出现的错误
    pub async fn incr(&self, key: &str) -> Result<i64> {
        let mut conn = self.pool.get().await?;
        let value: i64 = conn.incr(key, 1).await?;
        Ok(value)
    }

    /// 增加键的值 (指定增量)
    pub async fn incr_by(&self, key: &str, delta: i64) -> Result<i64> {
        let mut conn = self.pool.get().await?;
        let value: i64 = conn.incr(key, delta).await?;
        Ok(value)
    }

    /// 减少键的值
    ///
    /// # 参数
    ///
    /// * `key` - 键
    ///
    /// # 返回值
    ///
    /// * `Ok(i64)` - 减少后的值
    /// * `Err(anyhow::Error)` - 减少过程中出现的错误
    pub async fn decr(&self, key: &str) -> Result<i64> {
        let mut conn = self.pool.get().await?;
        let value: i64 = conn.decr(key, 1).await?;
        Ok(value)
    }

    /// 获取Redis连接（从连接池）
    ///
    /// # 返回值
    ///
    /// * `Ok(deadpool_redis::Connection)` - Redis连接
    /// * `Err(anyhow::Error)` - 获取连接过程中出现的错误
    pub async fn get_connection(&self) -> Result<deadpool_redis::Connection> {
        self.pool.get().await.map_err(|e| anyhow::anyhow!(e))
    }

    /// 删除指定键
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        conn.del::<_, ()>(key).await?;
        Ok(())
    }

    /// 扫描匹配模式的键
    ///
    /// # 参数
    ///
    /// * `pattern` - 键模式 (例如 "rate_limit:*")
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<String>)` - 匹配的键列表
    /// * `Err(anyhow::Error)` - 扫描过程中出现的错误
    pub async fn scan_pattern(&self, pattern: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get().await?;
        let mut cursor = 0i64;
        let mut keys = Vec::new();

        loop {
            let (new_cursor, batch): (i64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await?;

            keys.extend(batch);
            cursor = new_cursor;

            if cursor == 0 {
                break;
            }
        }

        Ok(keys)
    }

    /// 执行 Lua 脚本
    pub async fn eval(&self, script: &str, keys: &[&str], args: &[&str]) -> Result<String> {
        let mut conn = self.pool.get().await?;
        let mut cmd = redis::cmd("EVAL");
        cmd.arg(script).arg(keys.len());

        for key in keys {
            cmd.arg(key);
        }

        for arg in args {
            cmd.arg(arg);
        }

        let result: String = cmd.query_async(&mut conn).await?;
        Ok(result)
    }

    /// 检查键是否存在
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let exists: bool = conn.exists(key).await?;
        Ok(exists)
    }

    /// 检查Redis连接是否健康
    ///
    /// # 返回值
    ///
    /// * `Ok(true)` - 连接健康
    /// * `Err(anyhow::Error)` - 连接失败
    pub async fn ping(&self) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let result: String = conn.ping().await?;
        Ok(result == "PONG")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redis_client_config_default() {
        let config = RedisClientConfig::default();
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.connection_timeout, 10);
        assert_eq!(config.recycle_timeout, 5);
    }
}
