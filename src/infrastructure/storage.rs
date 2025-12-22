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
            data: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
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
