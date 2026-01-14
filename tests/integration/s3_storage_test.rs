// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! S3 存储集成测试
//!
//! 测试 S3 兼容对象存储功能（使用 MinIO 或 AWS S3）
//!
//! 运行测试前需要启动 MinIO:
//! ```bash
//! docker-compose -f docker/docker-compose.minio.yml up -d
//! ```
//! 或者设置环境变量使用 AWS S3:
//! ```bash
//! export AWS_ACCESS_KEY_ID=your_access_key
//! export AWS_SECRET_ACCESS_KEY=your_secret_key
//! export AWS_REGION=us-east-1
//! export S3_BUCKET=your-bucket
//! ```

use crawlrs::config::storage::StorageSettings;
use crawlrs::domain::repositories::storage_repository::StorageRepository;
use crawlrs::infrastructure::storage::{create_storage_repository, S3Storage};
use std::env;

/// 获取 S3/MinIO 凭据（优先使用环境变量）
fn get_s3_credentials() -> (String, String, String) {
    let access_key = env::var("S3_ACCESS_KEY_ID").unwrap_or_else(|_| {
        env::var("MINIO_ROOT_USER").unwrap_or_else(|_| "minioadmin".to_string())
    });
    let secret_key = env::var("S3_SECRET_ACCESS_KEY").unwrap_or_else(|_| {
        env::var("MINIO_ROOT_PASSWORD").unwrap_or_else(|_| "minioadmin123".to_string())
    });
    let endpoint = env::var("S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    (access_key, secret_key, endpoint)
}

/// 创建 S3 存储实例
async fn create_s3_storage() -> Option<S3Storage> {
    let (access_key, secret_key, endpoint) = get_s3_credentials();
    let storage = S3Storage::new(
        "us-east-1".to_string(),
        "crawlrs".to_string(),
        access_key,
        secret_key,
        Some(endpoint),
    );
    // 验证连接
    if storage.exists("health-check").await.is_ok() {
        Some(storage)
    } else {
        None
    }
}

/// 检查 S3/MinIO 是否可用
async fn is_s3_available() -> bool {
    // 检查环境变量
    if env::var("SKIP_S3_TESTS").is_ok() {
        return false;
    }
    create_s3_storage().await.is_some()
}

/// 跳过测试的辅助宏
macro_rules! skip_if_s3_unavailable {
    () => {
        if !is_s3_available().await {
            println!("⚠️  S3 storage test skipped - MinIO not available at localhost:9000");
            println!("   To run these tests, start MinIO:");
            println!("   docker-compose -f docker/docker-compose.minio.yml up -d");
            return;
        }
    };
}

#[tokio::test]
async fn test_s3_storage_save_and_get() {
    skip_if_s3_unavailable!();

    // 初始化日志
    let _ = tracing_subscriber::fmt::try_init();

    // 使用辅助函数创建存储实例
    let storage = create_s3_storage()
        .await
        .expect("S3 storage should be available");

    // 测试数据
    let key = "test/file.txt";
    let data = b"Hello, S3 Storage!";

    // 保存数据
    let result = storage.save(key, data).await;
    assert!(result.is_ok(), "Failed to save data: {:?}", result.err());

    // 获取数据
    let result = storage.get(key).await;
    assert!(result.is_ok(), "Failed to get data: {:?}", result.err());

    let retrieved_data = result.unwrap();
    assert_eq!(
        retrieved_data,
        Some(data.to_vec()),
        "Retrieved data does not match"
    );

    // 清理
    let _ = storage.delete(key).await;
}

#[tokio::test]
async fn test_s3_storage_exists() {
    skip_if_s3_unavailable!();
    let _ = tracing_subscriber::fmt::try_init();

    let storage = create_s3_storage()
        .await
        .expect("S3 storage should be available");

    let key = "test/exists.txt";
    let data = b"Test exists";

    // 检查不存在的文件
    let result = storage.exists(key).await;
    assert!(
        result.is_ok(),
        "Failed to check existence: {:?}",
        result.err()
    );
    assert!(!result.unwrap(), "File should not exist yet");

    // 保存文件
    storage.save(key, data).await.unwrap();

    // 检查存在的文件
    let result = storage.exists(key).await;
    assert!(
        result.is_ok(),
        "Failed to check existence: {:?}",
        result.err()
    );
    assert!(result.unwrap(), "File should exist now");

    // 清理
    let _ = storage.delete(key).await;
}

#[tokio::test]
async fn test_s3_storage_delete() {
    skip_if_s3_unavailable!();
    let _ = tracing_subscriber::fmt::try_init();

    let storage = create_s3_storage()
        .await
        .expect("S3 storage should be available");

    let key = "test/delete.txt";
    let data = b"Test delete";

    // 保存文件
    storage.save(key, data).await.unwrap();

    // 确认文件存在
    assert!(storage.exists(key).await.unwrap());

    // 删除文件
    let result = storage.delete(key).await;
    assert!(result.is_ok(), "Failed to delete: {:?}", result.err());

    // 确认文件不存在
    assert!(!storage.exists(key).await.unwrap());
}

#[tokio::test]
async fn test_s3_storage_large_file() {
    skip_if_s3_unavailable!();
    let _ = tracing_subscriber::fmt::try_init();

    let storage = create_s3_storage()
        .await
        .expect("S3 storage should be available");

    // 创建 1MB 的测试数据
    let key = "test/large.bin";
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

    // 保存大文件
    let result = storage.save(key, &data).await;
    assert!(
        result.is_ok(),
        "Failed to save large file: {:?}",
        result.err()
    );

    // 获取大文件
    let result = storage.get(key).await;
    assert!(
        result.is_ok(),
        "Failed to get large file: {:?}",
        result.err()
    );

    let retrieved_data = result.unwrap();
    assert_eq!(retrieved_data, Some(data), "Large file data does not match");

    // 清理
    let _ = storage.delete(key).await;
}

#[tokio::test]
async fn test_create_storage_repository_s3() {
    skip_if_s3_unavailable!();
    let _ = tracing_subscriber::fmt::try_init();

    let (access_key, secret_key, endpoint) = get_s3_credentials();
    let settings = StorageSettings {
        storage_type: "s3".to_string(),
        local_path: None,
        s3_region: Some("us-east-1".to_string()),
        s3_bucket: Some("crawlrs".to_string()),
        s3_access_key: Some(access_key),
        s3_secret_key: Some(secret_key),
        s3_endpoint: Some(endpoint),
    };

    let result = create_storage_repository(&settings);
    assert!(
        result.is_ok(),
        "Failed to create S3 repository: {:?}",
        result.err()
    );

    let storage = result.unwrap();

    // 测试基本操作
    let key = "test/factory.txt";
    let data = b"Factory test";

    storage.save(key, data).await.unwrap();
    let retrieved = storage.get(key).await.unwrap();
    assert_eq!(retrieved, Some(data.to_vec()));

    // 清理
    let _ = storage.delete(key).await;
}

#[tokio::test]
async fn test_create_storage_repository_missing_config() {
    let _ = tracing_subscriber::fmt::try_init();

    // 测试缺少 s3_region
    let (access_key, secret_key, endpoint) = get_s3_credentials();
    let settings = StorageSettings {
        storage_type: "s3".to_string(),
        local_path: None,
        s3_region: None,
        s3_bucket: Some("crawlrs".to_string()),
        s3_access_key: Some(access_key),
        s3_secret_key: Some(secret_key),
        s3_endpoint: Some(endpoint),
    };

    let result = create_storage_repository(&settings);
    assert!(result.is_err(), "Should fail with missing s3_region");

    // 测试缺少 s3_bucket
    let settings = StorageSettings {
        storage_type: "s3".to_string(),
        local_path: None,
        s3_region: Some("us-east-1".to_string()),
        s3_bucket: None,
        s3_access_key: Some("minioadmin".to_string()),
        s3_secret_key: Some("minioadmin123".to_string()),
        s3_endpoint: Some("http://localhost:9000".to_string()),
    };

    let result = create_storage_repository(&settings);
    assert!(result.is_err(), "Should fail with missing s3_bucket");
}

#[tokio::test]
async fn test_create_storage_repository_unsupported_type() {
    let _ = tracing_subscriber::fmt::try_init();

    let settings = StorageSettings {
        storage_type: "invalid_type".to_string(),
        local_path: None,
        s3_region: None,
        s3_bucket: None,
        s3_access_key: None,
        s3_secret_key: None,
        s3_endpoint: None,
    };

    let result = create_storage_repository(&settings);
    assert!(result.is_err(), "Should fail with unsupported type");
}
