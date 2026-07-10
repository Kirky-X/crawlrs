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

    #[test]
    fn test_redis_client_config_debug() {
        let config = RedisClientConfig {
            max_connections: 30,
            connection_timeout: 15,
            recycle_timeout: 8,
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("30"));
        assert!(debug_str.contains("15"));
        assert!(debug_str.contains("8"));
    }

    #[test]
    fn test_redis_client_config_custom() {
        let config = RedisClientConfig {
            max_connections: 50,
            connection_timeout: 20,
            recycle_timeout: 10,
        };
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.connection_timeout, 20);
        assert_eq!(config.recycle_timeout, 10);
    }

    #[test]
    fn test_redis_client_new_succeeds_without_server() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let config = client.config();
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.connection_timeout, 10);
        assert_eq!(config.recycle_timeout, 5);
    }

    #[test]
    fn test_redis_client_with_config_custom() {
        let config = RedisClientConfig {
            max_connections: 5,
            connection_timeout: 3,
            recycle_timeout: 2,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        let retrieved = client.config();
        assert_eq!(retrieved.max_connections, 5);
        assert_eq!(retrieved.connection_timeout, 3);
        assert_eq!(retrieved.recycle_timeout, 2);
    }

    #[test]
    fn test_redis_client_from_settings_with_values() {
        let settings = crate::config::RedisSettings {
            url: "redis://localhost:6379".to_string(),
            max_connections: Some(15),
            min_connections: Some(3),
            connection_timeout: Some(7),
            idle_timeout: Some(120),
        };
        let client = RedisClient::from_settings(&settings).unwrap();
        let config = client.config();
        assert_eq!(config.max_connections, 15);
        assert_eq!(config.connection_timeout, 7);
        assert_eq!(config.recycle_timeout, 120);
    }

    #[test]
    fn test_redis_client_from_settings_with_none_defaults() {
        let settings = crate::config::RedisSettings {
            url: "redis://localhost:6379".to_string(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            idle_timeout: None,
        };
        let client = RedisClient::from_settings(&settings).unwrap();
        let config = client.config();
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.connection_timeout, 10);
        assert_eq!(config.recycle_timeout, 300);
    }

    #[test]
    fn test_redis_client_pool_status() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let _status = client.pool_status();
    }

    #[tokio::test]
    async fn test_redis_client_mget_empty_keys() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let result = client.mget(&[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_redis_client_clone() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let cloned = client.clone();
        assert_eq!(
            client.config().max_connections,
            cloned.config().max_connections
        );
    }

    // ========== RedisClientConfig edge cases ==========

    #[test]
    fn test_redis_client_config_zero_connections() {
        let config = RedisClientConfig {
            max_connections: 0,
            connection_timeout: 5,
            recycle_timeout: 3,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        assert_eq!(client.config().max_connections, 0);
    }

    #[test]
    fn test_redis_client_config_large_values() {
        let config = RedisClientConfig {
            max_connections: 10000,
            connection_timeout: 3600,
            recycle_timeout: 1800,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        let retrieved = client.config();
        assert_eq!(retrieved.max_connections, 10000);
        assert_eq!(retrieved.connection_timeout, 3600);
        assert_eq!(retrieved.recycle_timeout, 1800);
    }

    #[test]
    fn test_redis_client_config_one_connection() {
        let config = RedisClientConfig {
            max_connections: 1,
            connection_timeout: 1,
            recycle_timeout: 1,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        assert_eq!(client.config().max_connections, 1);
        assert_eq!(client.config().connection_timeout, 1);
        assert_eq!(client.config().recycle_timeout, 1);
    }

    #[test]
    fn test_redis_client_config_clone_preserves_values() {
        let config = RedisClientConfig {
            max_connections: 42,
            connection_timeout: 99,
            recycle_timeout: 77,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_connections, 42);
        assert_eq!(cloned.connection_timeout, 99);
        assert_eq!(cloned.recycle_timeout, 77);
    }

    // ========== RedisClient with different URLs ==========

    #[test]
    fn test_redis_client_with_rediss_url() {
        // rediss:// is Redis over TLS; without TLS feature enabled, builder returns error
        let result = RedisClient::new("rediss://localhost:6379");
        assert!(result.is_err(), "rediss:// should fail without TLS feature");
    }

    #[test]
    fn test_redis_client_with_unix_socket_url() {
        // Unix socket URL should create a pool successfully (lazy connection)
        let client = RedisClient::new("redis+unix:///tmp/redis.sock").unwrap();
        assert_eq!(client.config().max_connections, 20);
    }

    #[test]
    fn test_redis_client_with_password_url() {
        // URL with credentials should work
        let client = RedisClient::new("redis://:password@localhost:6379").unwrap();
        let config = client.config();
        assert_eq!(config.connection_timeout, 10);
    }

    #[test]
    fn test_redis_client_with_db_index_url() {
        // URL with database index
        let client = RedisClient::new("redis://localhost:6379/3").unwrap();
        assert_eq!(client.config().max_connections, 20);
    }

    #[test]
    fn test_redis_client_with_custom_config_and_rediss() {
        let config = RedisClientConfig {
            max_connections: 8,
            connection_timeout: 4,
            recycle_timeout: 2,
        };
        // rediss:// requires TLS feature which is not enabled; builder returns error
        let result = RedisClient::with_config("rediss://secure-redis:6380", config);
        assert!(result.is_err(), "rediss:// should fail without TLS feature");
    }

    // ========== from_settings with various configurations ==========

    #[test]
    fn test_from_settings_minimal_config() {
        let settings = crate::config::RedisSettings {
            url: "redis://localhost:6379".to_string(),
            max_connections: Some(1),
            min_connections: Some(0),
            connection_timeout: Some(1),
            idle_timeout: Some(1),
        };
        let client = RedisClient::from_settings(&settings).unwrap();
        let config = client.config();
        assert_eq!(config.max_connections, 1);
        assert_eq!(config.connection_timeout, 1);
        assert_eq!(config.recycle_timeout, 1);
    }

    #[test]
    fn test_from_settings_partial_none() {
        let settings = crate::config::RedisSettings {
            url: "redis://localhost:6379".to_string(),
            max_connections: Some(25),
            min_connections: None,
            connection_timeout: None,
            idle_timeout: Some(60),
        };
        let client = RedisClient::from_settings(&settings).unwrap();
        let config = client.config();
        // max_connections from setting, connection_timeout and recycle_timeout from defaults
        assert_eq!(config.max_connections, 25);
        assert_eq!(config.connection_timeout, 10); // default
        assert_eq!(config.recycle_timeout, 60); // from idle_timeout
    }

    #[test]
    fn test_from_settings_url_accessed_correctly() {
        let test_url = "redis://myhost:6380/5".to_string();
        let settings = crate::config::RedisSettings {
            url: test_url.clone(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            idle_timeout: None,
        };
        // Verify the URL is accessible via settings.url()
        assert_eq!(settings.url(), test_url);
        let client = RedisClient::from_settings(&settings).unwrap();
        // The client should be created successfully with the given URL
        let config = client.config();
        assert_eq!(config.max_connections, 20); // default
    }

    // ========== pool_status ==========

    #[test]
    fn test_pool_status_available_size() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let status = client.pool_status();
        // A freshly created pool with lazy connections has max_size set but
        // no actual connections created yet (available=0, size=0)
        assert_eq!(status.max_size, 20);
        assert_eq!(status.size, 0);
        assert_eq!(status.available, 0);
        assert_eq!(status.waiting, 0);
    }

    #[test]
    fn test_pool_status_after_clone() {
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let cloned = client.clone();
        let status = cloned.pool_status();
        // Clone shares the same pool, so max_size is preserved
        assert_eq!(status.max_size, 20);
        assert_eq!(status.size, 0);
    }

    // ========== config() accessor ==========

    #[test]
    fn test_config_accessor_returns_reference() {
        let config = RedisClientConfig {
            max_connections: 7,
            connection_timeout: 12,
            recycle_timeout: 8,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        let config_ref = client.config();
        assert_eq!(config_ref.max_connections, 7);
        assert_eq!(config_ref.connection_timeout, 12);
        assert_eq!(config_ref.recycle_timeout, 8);
    }

    // ========== Default config verification ==========

    #[test]
    fn test_default_config_constants_match_documentation() {
        let config = RedisClientConfig::default();
        // Verify default values match the documented defaults
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.connection_timeout, 10);
        assert_eq!(config.recycle_timeout, 5);
    }

    #[test]
    fn test_config_debug_format_contains_all_fields() {
        let config = RedisClientConfig {
            max_connections: 100,
            connection_timeout: 30,
            recycle_timeout: 15,
        };
        let debug = format!("{:?}", config);
        assert!(debug.contains("max_connections"));
        assert!(debug.contains("100"));
        assert!(debug.contains("connection_timeout"));
        assert!(debug.contains("30"));
        assert!(debug.contains("recycle_timeout"));
        assert!(debug.contains("15"));
    }
}
