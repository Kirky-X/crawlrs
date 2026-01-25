// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Storage repository module
//!
//! Defines the storage repository interface and provides a no-op implementation
//! for cases where storage is not configured.

use async_trait::async_trait;
use shaku::Interface;
use thiserror::Error;

/// 存储错误类型
#[derive(Error, Debug)]
pub enum StorageError {
    /// IO错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// 存储错误
    #[error("Storage error: {0}")]
    Other(String),
    /// 无效的存储键
    #[error("Invalid storage key: {0}")]
    InvalidKey(String),
}

/// 存储仓库特质
///
/// 定义存储数据访问接口
#[async_trait]
pub trait StorageRepository: Interface + Send + Sync {
    /// 使用指定键保存数据到存储中
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError>;

    /// 根据键从存储中检索数据
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;

    /// 根据键从存储中删除数据
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// 检查存储中是否存在指定键
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}

/// No-op storage implementation
///
/// Used when storage is not configured. All operations are no-ops that return success.
#[derive(Debug, Clone, Default)]
pub struct NoOpStorage;

#[async_trait::async_trait]
impl StorageRepository for NoOpStorage {
    async fn save(&self, _key: &str, _data: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }

    async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(None)
    }

    async fn delete(&self, _key: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
        Ok(false)
    }
}
