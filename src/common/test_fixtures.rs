// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Testcontainers integration test fixtures.
//!
//! Provides on-demand PostgreSQL containers for tests that
//! require real external services. Containers are started per-test and
//! torn down on drop.
//!
//! # Requirements
//!
//! - Docker must be available on the host.
//! - Tests using these fixtures should detect Docker availability and
//!   early-return if unavailable (see [`docker_available`]).

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;

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

/// A test fixture that starts a PostgreSQL container.
///
/// Replaces the former `DbRedisHandle` — Redis is no longer used at runtime.
pub struct DbHandle {
    pub pg: PgHandle,
}

impl DbHandle {
    /// Start a PostgreSQL container.
    pub async fn start() -> anyhow::Result<Self> {
        let pg = PgHandle::start().await?;
        Ok(Self { pg })
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

/// Build app `Settings` patched with the given database URL.
///
/// Loads the default configuration file, then overrides the database URL
/// to point at the testcontainers PostgreSQL instance.
pub fn settings_with_urls(db_url: &str) -> anyhow::Result<crate::config::Settings> {
    let mut settings = crate::bootstrap::config::load_settings()?;
    settings.database = database_settings(db_url);
    Ok(settings)
}
