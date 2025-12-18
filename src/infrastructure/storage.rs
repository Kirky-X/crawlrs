// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::config::settings::StorageSettings;
use crate::domain::repositories::storage_repository::{StorageError, StorageRepository};

/// 本地文件系统存储实现
pub struct LocalStorage {
    base_path: String,
}

impl LocalStorage {
    pub fn new(base_path: String) -> Self {
        Self { base_path }
    }

    fn get_full_path(&self, key: &str) -> String {
        Path::new(&self.base_path)
            .join(key)
            .to_string_lossy()
            .to_string()
    }
}

#[async_trait]
impl StorageRepository for LocalStorage {
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let full_path = self.get_full_path(key);
        
        // 确保目录存在
        if let Some(parent) = Path::new(&full_path).parent() {
            fs::create_dir_all(parent).await?;
        }
        
        let mut file = fs::File::create(&full_path).await?;
        file.write_all(data).await?;
        file.flush().await?;
        
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let full_path = self.get_full_path(key);
        
        match fs::read(&full_path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let full_path = self.get_full_path(key);
        
        match fs::remove_file(&full_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let full_path = self.get_full_path(key);
        Ok(Path::new(&full_path).exists())
    }
}

/// S3 存储实现（简化版占位符，实际使用需要正确配置 AWS SDK）
#[cfg(feature = "s3")]
pub struct S3Storage {
    bucket: String,
    _region: String,
    _endpoint: Option<String>,
}

#[cfg(feature = "s3")]
impl S3Storage {
    pub async fn new(
        region: String,
        bucket: String,
        _access_key: Option<String>,
        _secret_key: Option<String>,
        endpoint: Option<String>,
    ) -> Result<Self, StorageError> {
        // 简化的 S3 存储实现，实际使用时需要正确配置 AWS SDK
        // 这里仅作为占位符实现，避免复杂的 AWS SDK 依赖问题
        Ok(Self {
            bucket,
            _region: region,
            _endpoint: endpoint,
        })
    }
}

#[cfg(feature = "s3")]
#[async_trait]
impl StorageRepository for S3Storage {
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        // 简化的 S3 实现 - 实际使用时需要正确配置 AWS SDK
        Err(StorageError::Other(format!(
            "S3 storage not fully implemented. Would save {} bytes to bucket {} with key {}",
            data.len(),
            self.bucket,
            key
        )))
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        // 简化的 S3 实现 - 实际使用时需要正确配置 AWS SDK
        Err(StorageError::Other(format!(
            "S3 storage not fully implemented. Would get key {} from bucket {}",
            key, self.bucket
        )))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        // 简化的 S3 实现 - 实际使用时需要正确配置 AWS SDK
        Err(StorageError::Other(format!(
            "S3 storage not fully implemented. Would delete key {} from bucket {}",
            key, self.bucket
        )))
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        // 简化的 S3 实现 - 实际使用时需要正确配置 AWS SDK
        Err(StorageError::Other(format!(
            "S3 storage not fully implemented. Would check existence of key {} in bucket {}",
            key, self.bucket
        )))
    }
}

/// 存储工厂函数
pub fn create_storage_repository(
    settings: &StorageSettings,
) -> Result<Box<dyn StorageRepository + Send + Sync>, StorageError> {
    match settings.storage_type.as_str() {
        "local" => {
            let base_path = settings
                .local_path
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "./storage".to_string());
            Ok(Box::new(LocalStorage::new(base_path)))
        }
        #[cfg(feature = "s3")]
        "s3" => {
            // S3 存储需要异步创建，这里返回错误提示
            Err(StorageError::Other(
                "S3 storage requires async initialization. Use create_storage_repository_async instead.".to_string(),
            ))
        }
        other => Err(StorageError::Other(format!(
            "Unsupported storage type: {}",
            other
        ))),
    }
}

/// 异步存储工厂函数（支持 S3）
#[cfg(feature = "s3")]
pub async fn create_storage_repository_async(
    settings: &StorageSettings,
) -> Result<Box<dyn StorageRepository + Send + Sync>, StorageError> {
    match settings.storage_type.as_str() {
        "local" => {
            let base_path = settings
                .local_path
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "./storage".to_string());
            Ok(Box::new(LocalStorage::new(base_path)))
        }
        "s3" => {
            let region = settings
                .s3_region
                .as_ref()
                .ok_or_else(|| StorageError::Other("S3 region is required".to_string()))?
                .clone();
            let bucket = settings
                .s3_bucket
                .as_ref()
                .ok_or_else(|| StorageError::Other("S3 bucket is required".to_string()))?
                .clone();
            
            let storage = S3Storage::new(
                region,
                bucket,
                settings.s3_access_key.clone(),
                settings.s3_secret_key.clone(),
                settings.s3_endpoint.clone(),
            )
            .await?;
            
            Ok(Box::new(storage))
        }
        other => Err(StorageError::Other(format!(
            "Unsupported storage type: {}",
            other
        ))),
    }
}

/// 测试用的内存存储实现（用于单元测试）
pub struct InMemoryStorage {
    data: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<u8>>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageRepository for InMemoryStorage {
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut map = self.data.write().await;
        map.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let map = self.data.read().await;
        Ok(map.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let mut map = self.data.write().await;
        map.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let map = self.data.read().await;
        Ok(map.contains_key(key))
    }
}