// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bootstrap module for application initialization.
//!
//! This module provides a structured way to initialize all application components
//! in a clear, testable, and maintainable manner.
//!
//! # Module Structure
//!
//! - [`telemetry`](telemetry) - Telemetry and metrics initialization
//! - [`config`](config) - Configuration loading and validation
//! - [`infrastructure`](infrastructure) - Database, Redis, repositories
//! - [`engines`](engines) - Scraper engines and router
//! - [`services`](services) - Application services
//! - [`routes`](routes) - Route configuration and application builder

pub mod config;
pub mod engines;
pub mod infrastructure;
pub mod routes;
pub mod services;
pub mod telemetry;
