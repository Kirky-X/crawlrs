// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 通用模块
//!
//! 提供应用程序的通用功能，包括错误类型、常量定义等

pub mod constants;
pub mod error;
pub mod time_utils;

pub use constants::*;
pub use error::{AppError, AppResult};
pub use time_utils::{
    from_db_datetime, from_db_datetime_opt, to_db_datetime, to_db_datetime_opt, UTC_OFFSET,
};

/// Test support utilities shared across modules
#[cfg(test)]
pub(crate) mod test_support {
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    /// Global mutex to serialize tests that manipulate environment variables.
    /// Environment variables are process-global, so all test modules that
    /// set/unset env vars must lock this mutex to prevent race conditions.
    pub static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    /// Testcontainers integration test fixtures.
    ///
    /// Provides on-demand PostgreSQL and Redis containers for tests that
    /// require real external services. Containers are started per-test and
    /// torn down on drop.
    ///
    /// # Requirements
    ///
    /// - Docker must be available on the host.
    /// - Tests using these fixtures should detect Docker availability and
    ///   early-return if unavailable (see [`docker_available`]).
    pub mod testcontainers_fixtures {
        use testcontainers::core::IntoContainerPort;
        use testcontainers::ImageExt;
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;
        use testcontainers_modules::redis::Redis;

        /// Check whether Docker is available on the host.
        ///
        /// Tests that depend on testcontainers should call this first and
        /// skip execution (return early) when it returns `false`.
        pub async fn docker_available() -> bool {
            // testcontainers internally uses bollard to talk to the Docker
            // daemon. We attempt a lightweight ping via `docker info`.
            tokio::process::Command::new("docker")
                .arg("info")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false)
        }

        /// A running PostgreSQL container with its mapped port.
        ///
        /// The container is stopped when this struct is dropped.
        pub struct PgHandle {
            /// Host port mapped to PostgreSQL's 5432.
            #[allow(dead_code)]
            pub port: u16,
            /// Full connection URL (postgres://postgres:postgres@127.0.0.1:PORT/test).
            pub url: String,
            // Keep the container alive; dropped last.
            _container: Option<testcontainers::ContainerAsync<Postgres>>,
        }

        impl PgHandle {
            /// Start a fresh PostgreSQL container and return a handle.
            ///
            /// Uses the `postgres:16-alpine` image. The container exposes
            /// port 5432 with credentials `postgres:postgres` and a default
            /// database named `postgres` (matching the image default).
            pub async fn start() -> anyhow::Result<Self> {
                let image = Postgres::default();
                let container = image
                    .with_tag("16-alpine")
                    .start()
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to start postgres container: {e}"))?;
                let port = container
                    .get_host_port_ipv4(5432.tcp())
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to get postgres port: {e}"))?;
                // Use the default `postgres` database created by the image.
                let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
                Ok(Self {
                    port,
                    url,
                    _container: Some(container),
                })
            }
        }

        /// A running Redis container with its mapped port.
        ///
        /// The container is stopped when this struct is dropped.
        pub struct RedisHandle {
            /// Host port mapped to Redis' 6379.
            #[allow(dead_code)]
            pub port: u16,
            /// Full connection URL (redis://127.0.0.1:PORT).
            pub url: String,
            _container: Option<testcontainers::ContainerAsync<Redis>>,
        }

        impl RedisHandle {
            /// Start a fresh Redis container and return a handle.
            ///
            /// Uses the `redis:7-alpine` image.
            pub async fn start() -> anyhow::Result<Self> {
                let image = Redis::default();
                let container = image
                    .with_tag("7-alpine")
                    .start()
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to start redis container: {e}"))?;
                let port = container
                    .get_host_port_ipv4(6379.tcp())
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to get redis port: {e}"))?;
                let url = format!("redis://127.0.0.1:{port}");
                Ok(Self {
                    port,
                    url,
                    _container: Some(container),
                })
            }
        }

        /// A combined fixture that starts both PostgreSQL and Redis.
        pub struct DbRedisHandle {
            pub pg: PgHandle,
            pub redis: RedisHandle,
        }

        impl DbRedisHandle {
            /// Start both PostgreSQL and Redis containers concurrently.
            pub async fn start() -> anyhow::Result<Self> {
                let (pg, redis) = tokio::try_join!(PgHandle::start(), RedisHandle::start())?;
                Ok(Self { pg, redis })
            }
        }

        /// Build a `crate::config::DatabaseSettings` pointing at the given URL.
        pub fn database_settings(url: &str) -> crate::config::DatabaseSettings {
            crate::config::DatabaseSettings {
                url: url.to_string(),
                max_connections: Some(5),
                min_connections: Some(1),
                connect_timeout: Some(30),
                idle_timeout: Some(300),
                max_lifetime: Some(1800),
                connection_keepalive: Some(30),
                health_check_interval: Some(60),
            }
        }

        /// Build a `crate::config::RedisSettings` pointing at the given URL.
        pub fn redis_settings(url: &str) -> crate::config::RedisSettings {
            crate::config::RedisSettings {
                url: url.to_string(),
                max_connections: Some(5),
                min_connections: Some(1),
                connection_timeout: Some(10),
                idle_timeout: Some(300),
            }
        }

        /// Build app `Settings` patched with the given database and Redis URLs.
        ///
        /// Loads the default configuration file, then overrides the database
        /// and Redis URLs to point at the testcontainers instances.
        pub fn settings_with_urls(db_url: &str, redis_url: &str) -> anyhow::Result<crate::config::Settings> {
            let mut settings = crate::bootstrap::config::load_settings()?;
            settings.database = database_settings(db_url);
            settings.redis = redis_settings(redis_url);
            Ok(settings)
        }
    }
}
