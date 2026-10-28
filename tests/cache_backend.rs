// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Cache backend fail-closed tests (spec: redis-shared-cache,
//! R-redis-shared-cache-001).
//!
//! `[cache] backend = "memory" | "redis"`：
//! - `backend=redis` 且 redis feature 未编译时，启动（`load_settings`）拒绝；
//! - `backend=redis` 且 `redis_url` 缺失时，启动拒绝；
//! - 缺省 `backend=memory` 行为不变。
//!
//! 环境变量经 confers `EnvSource`（`CRAWLRS__CACHE__` 前缀）注入，
//! 与生产启动路径（env 覆盖 → 校验）同链路。

use std::sync::{Mutex, MutexGuard};

/// 环境变量是进程全局状态；本文件内用例持锁串行，避免并行用例互相污染。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 用例期间设置的环境变量，drop 时统一清理。
struct EnvVars {
    _guard: MutexGuard<'static, ()>,
}

impl EnvVars {
    /// 在持锁前提下写入用例环境变量。
    fn with(backend: &str, redis_url: Option<&str>) -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CRAWLRS__CACHE__BACKEND", backend);
        match redis_url {
            Some(url) => std::env::set_var("CRAWLRS__CACHE__REDIS_URL", url),
            None => std::env::remove_var("CRAWLRS__CACHE__REDIS_URL"),
        }
        Self { _guard: guard }
    }
}

impl Drop for EnvVars {
    fn drop(&mut self) {
        std::env::remove_var("CRAWLRS__CACHE__BACKEND");
        std::env::remove_var("CRAWLRS__CACHE__REDIS_URL");
    }
}

/// `backend=redis` 且 redis feature 未编译：启动必须返回配置错误。
///
/// 默认构建（default features）不含 `cache-redis`，本用例在该构建下
/// 断言拒绝；编译 `cache-redis` 后改由 `redis_url` 缺失判定。
#[cfg(not(feature = "cache-redis"))]
#[test]
fn redis_backend_rejected_without_compiled_feature() {
    let _env = EnvVars::with("redis", None);

    let result = crawlrs::bootstrap::config::load_settings();

    let err = result.err().unwrap_or_else(|| {
        panic!(
            "backend=redis must be rejected at startup when the redis feature \
             is not compiled (fail-closed), but load_settings returned Ok"
        )
    });
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("redis"),
        "error must mention the redis backend, got: {msg}"
    );
}

/// `backend=redis` 且提供了 `redis_url`，但 redis feature 仍未编译：
/// 编译期能力缺口不能靠 URL 绕过，启动必须拒绝。
#[cfg(not(feature = "cache-redis"))]
#[test]
fn redis_backend_rejected_with_url_but_without_compiled_feature() {
    let _env = EnvVars::with("redis", Some("rediss://127.0.0.1:6379"));

    let result = crawlrs::bootstrap::config::load_settings();

    assert!(
        result.is_err(),
        "backend=redis with redis_url must still be rejected when the redis \
         feature is not compiled (fail-closed), but load_settings returned Ok"
    );
}

/// `backend=redis` 且 feature 已编译：提供 `redis_url` 时应通过校验。
#[cfg(feature = "cache-redis")]
#[test]
fn redis_backend_accepted_with_url_when_feature_compiled() {
    let _env = EnvVars::with("redis", Some("rediss://127.0.0.1:6379"));

    let result = crawlrs::bootstrap::config::load_settings();

    assert!(
        result.is_ok(),
        "backend=redis with redis_url and compiled feature must load, got: {:?}",
        result.err()
    );
}

/// `backend=redis` 且 feature 已编译但 `redis_url` 缺失：启动拒绝。
#[cfg(feature = "cache-redis")]
#[test]
fn redis_backend_rejected_without_url_when_feature_compiled() {
    let _env = EnvVars::with("redis", None);

    let result = crawlrs::bootstrap::config::load_settings();

    let err = result.err().unwrap_or_else(|| {
        panic!(
            "backend=redis without redis_url must be rejected at startup \
             (fail-closed), but load_settings returned Ok"
        )
    });
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("redis"),
        "error must mention the redis backend, got: {msg}"
    );
}

/// 未知 backend 值：fail-closed 拒绝，不允许静默回退 memory。
#[test]
fn unknown_backend_value_rejected() {
    let _env = EnvVars::with("memcached", None);

    let result = crawlrs::bootstrap::config::load_settings();

    assert!(
        result.is_err(),
        "unknown backend value must be rejected at startup, but load_settings returned Ok"
    );
}

/// 缺省 backend（memory）：启动行为不变（R-redis-shared-cache-001 第三条）。
#[test]
fn default_backend_memory_still_loads() {
    let _env = {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("CRAWLRS__CACHE__BACKEND");
        std::env::remove_var("CRAWLRS__CACHE__REDIS_URL");
        guard
    };

    let result = crawlrs::bootstrap::config::load_settings();

    assert!(
        result.is_ok(),
        "default (memory) backend must keep loading, got: {:?}",
        result.err()
    );
}
