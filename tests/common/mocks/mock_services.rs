// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T043: 统一 mock 服务实现 — MockWebhookService, MockCacheService

#![allow(dead_code)]

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Mutex;

use crawlrs::domain::models::webhook::WebhookEvent;
use crawlrs::domain::models::Task;
use crawlrs::domain::services::webhook_service::WebhookService;
use crawlrs::infrastructure::oxcache::CacheService;

// ============================================================================
// MockWebhookService
// ============================================================================

/// Noop `WebhookService` — all methods return Ok(()).
pub struct MockWebhookService;

#[async_trait::async_trait]
impl WebhookService for MockWebhookService {
    async fn send_webhook(&self, _event: &WebhookEvent) -> anyhow::Result<()> {
        Ok(())
    }
    async fn trigger_completion(&self, _task: &Task) -> anyhow::Result<()> {
        Ok(())
    }
    async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> anyhow::Result<()> {
        Ok(())
    }
}

// ============================================================================
// MockCacheService
// ============================================================================

/// In-memory `CacheService` mock backed by a `Mutex<HashMap>`.
pub struct MockCacheService {
    data: Mutex<HashMap<String, String>>,
}

impl MockCacheService {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
    }

    /// Create with a pre-populated entry.
    pub fn with_entry(key: &str, value: &str) -> Self {
        let s = Self::new();
        s.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        s
    }
}

impl Default for MockCacheService {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheService for MockCacheService {
    fn get(
        &self,
        key: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>>
    {
        let key = key.to_string();
        Box::pin(async move {
            Ok(self
                .data
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?
                .get(&key)
                .cloned())
        })
    }

    fn set(
        &self,
        key: &str,
        value: &str,
        _ttl_seconds: u64,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let key = key.to_string();
        let value = value.to_string();
        Box::pin(async move {
            self.data
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?
                .insert(key, value);
            Ok(())
        })
    }

    fn delete(
        &self,
        key: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            self.data
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?
                .remove(&key);
            Ok(())
        })
    }

    fn exists(
        &self,
        key: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            Ok(self
                .data
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {}", e))?
                .contains_key(&key))
        })
    }
}
