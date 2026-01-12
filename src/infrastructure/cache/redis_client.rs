// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use redis::AsyncCommands;

/// Redis客户端
///
/// 提供对Redis数据库的异步操作接口
#[derive(Clone)]
pub struct RedisClient {
    /// Redis客户端
    client: redis::Client,
}

impl RedisClient {
    /// 创建新的Redis客户端实例
    ///
    /// # 参数
    ///
    /// * `redis_url` - Redis连接URL
    ///
    /// # 返回值
    ///
    /// * `Ok(RedisClient)` - Redis客户端实例
    /// * `Err(anyhow::Error)` - 创建过程中出现的错误
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self { client })
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let value: Option<String> = con.get(key).await?;
        Ok(value)
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.set_ex::<_, _, ()>(key, value, ttl_seconds as u64)
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.set::<_, _, ()>(key, value).await?;
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.expire::<_, ()>(key, seconds as i64).await?;
        Ok(())
    }

    /// 向有序集合添加成员
    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.zadd::<_, _, _, ()>(key, member, score).await?;
        Ok(())
    }

    /// 从有序集合移除成员
    pub async fn zrem(&self, key: &str, member: &str) -> Result<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.zrem::<_, _, ()>(key, member).await?;
        Ok(())
    }

    /// 获取有序集合的成员数量
    pub async fn zcard(&self, key: &str) -> Result<u64> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let count: u64 = con.zcard(key).await?;
        Ok(count)
    }

    /// 移除有序集合中指定分数范围的成员
    pub async fn zrembyscore(&self, key: &str, min: f64, max: f64) -> Result<u64> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let count: u64 = con.zrembyscore(key, min, max).await?;
        Ok(count)
    }

    /// 获取有序集合中成员的排名（从0开始，按分数从小到大）
    pub async fn zrank(&self, key: &str, member: &str) -> Result<Option<usize>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let rank: Option<usize> = con.zrank(key, member).await?;
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let value: i64 = con.incr(key, 1).await?;
        Ok(value)
    }

    /// 增加键的值 (指定增量)
    pub async fn incr_by(&self, key: &str, delta: i64) -> Result<i64> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let value: i64 = con.incr(key, delta).await?;
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let value: i64 = con.decr(key, 1).await?;
        Ok(value)
    }

    /// 获取Redis多路复用连接
    ///
    /// # 返回值
    ///
    /// * `Ok(redis::aio::MultiplexedConnection)` - Redis多路复用连接
    /// * `Err(anyhow::Error)` - 获取连接过程中出现的错误
    pub async fn get_connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// 删除指定键
    pub async fn del(&self, key: &str) -> Result<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.del::<_, ()>(key).await?;
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
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let mut cursor = 0i64;
        let mut keys = Vec::new();

        loop {
            let (new_cursor, batch): (i64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut con)
                .await?;

            keys.extend(batch);
            cursor = new_cursor;

            if cursor == 0 {
                break;
            }
        }

        Ok(keys)
    }
}
