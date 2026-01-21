// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
#[cfg(feature = "storage-s3")]
use aws_sdk_s3::primitives::ByteStream;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::config::StorageSettings;
use crate::domain::repositories::storage_repository::{StorageError, StorageRepository};

#[cfg(feature = "storage-s3")]
mod s3_storage {
    use super::*;
    use async_trait::async_trait;

    /// S3 对象存储实现
    pub struct S3Storage {
        client: aws_sdk_s3::Client,
        bucket: String,
    }

    impl S3Storage {
        pub fn new(
            region: String,
            bucket: String,
            access_key: String,
            secret_key: String,
            endpoint: Option<String>,
        ) -> Self {
            let credentials =
                aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "static");

            let mut config_builder = aws_sdk_s3::config::Builder::new()
                .region(aws_sdk_s3::config::Region::new(region))
                .credentials_provider(credentials)
                .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest());

            if let Some(ep) = endpoint {
                config_builder = config_builder.endpoint_url(ep).force_path_style(true);
            }

            let config = config_builder.build();
            let client = aws_sdk_s3::Client::from_conf(config);

            Self { client, bucket }
        }
    }

    #[async_trait]
    impl StorageRepository for S3Storage {
        async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(key)
                .body(ByteStream::from(data.to_vec()))
                .send()
                .await
                .map_err(|e| StorageError::Other(e.to_string()))?;
            Ok(())
        }

        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            match self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
            {
                Ok(output) => {
                    let data = output
                        .body
                        .collect()
                        .await
                        .map_err(|e| StorageError::Other(e.to_string()))?
                        .into_bytes();
                    Ok(Some(data.to_vec()))
                }
                Err(e) => {
                    let service_error = e.into_service_error();
                    if service_error.is_no_such_key() {
                        Ok(None)
                    } else {
                        Err(StorageError::Other(service_error.to_string()))
                    }
                }
            }
        }

        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| StorageError::Other(e.to_string()))?;
            Ok(())
        }

        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            match self
                .client
                .head_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    let service_error = e.into_service_error();
                    if service_error.is_not_found() {
                        Ok(false)
                    } else {
                        Err(StorageError::Other(service_error.to_string()))
                    }
                }
            }
        }
    }
}

/// 本地文件系统存储实现
pub struct LocalStorage {
    base_path: String,
}

impl LocalStorage {
    pub fn new(base_path: String) -> Self {
        Self { base_path }
    }

    fn validate_key(&self, key: &str) -> Result<(), StorageError> {
        if key.is_empty() {
            return Err(StorageError::InvalidKey("Key cannot be empty".to_string()));
        }

        if key.starts_with('/') || key.starts_with('\\') {
            return Err(StorageError::InvalidKey(
                "Key cannot start with absolute path".to_string(),
            ));
        }

        if key.contains("..") || key.contains("./") || key.contains(".\\") {
            return Err(StorageError::InvalidKey(
                "Key contains invalid path traversal characters".to_string(),
            ));
        }

        Ok(())
    }

    fn get_full_path(&self, key: &str) -> Result<String, StorageError> {
        self.validate_key(key)?;

        let base = Path::new(&self.base_path);
        let joined = base.join(key);

        // Use ? operator to flatten nested conditions
        let canonical = joined.canonicalize().map_err(|e| {
            StorageError::InvalidKey(format!("Path canonicalization failed: {}", e))
        })?;
        let base_canonical = base.canonicalize().map_err(|e| {
            StorageError::InvalidKey(format!("Base path canonicalization failed: {}", e))
        })?;

        if !canonical.starts_with(&base_canonical) {
            return Err(StorageError::InvalidKey(
                "Path escapes base directory".to_string(),
            ));
        }

        Ok(joined.to_string_lossy().to_string())
    }
}

#[async_trait]
impl StorageRepository for LocalStorage {
    async fn save(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let full_path = self.get_full_path(key)?;

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
        let full_path = self.get_full_path(key)?;

        match fs::read(&full_path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let full_path = self.get_full_path(key)?;

        match fs::remove_file(&full_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let full_path = self.get_full_path(key)?;
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
        "s3" => {
            #[cfg(feature = "storage-s3")]
            {
                let region = settings
                    .s3_region
                    .as_ref()
                    .ok_or_else(|| StorageError::Other("Missing s3_region configuration".to_string()))?
                    .clone();
                let bucket = settings
                    .s3_bucket
                    .as_ref()
                    .ok_or_else(|| StorageError::Other("Missing s3_bucket configuration".to_string()))?
                    .clone();
                let access_key = settings
                    .s3_access_key
                    .as_ref()
                    .ok_or_else(|| {
                        StorageError::Other("Missing s3_access_key configuration".to_string())
                    })?
                    .clone();
                let secret_key = settings
                    .s3_secret_key
                    .as_ref()
                    .ok_or_else(|| {
                        StorageError::Other("Missing s3_secret_key configuration".to_string())
                    })?
                    .clone();
                let endpoint = settings.s3_endpoint.clone();

                Ok(Box::new(s3_storage::S3Storage::new(
                    region, bucket, access_key, secret_key, endpoint,
                )))
            }
            #[cfg(not(feature = "storage-s3"))]
            {
                Err(StorageError::Other(
                    "S3 storage requires 'storage-s3' feature to be enabled. \
                     Please rebuild with --features storage-s3 or full"
                        .to_string(),
                ))
            }
        }
        other => Err(StorageError::Other(format!(
            "Unsupported storage type: {}. Supported types: local, s3",
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
                std::collections::HashMap::with_capacity(1024),
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
