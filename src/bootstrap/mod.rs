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
//! - `telemetry` - Telemetry and metrics initialization
//! - `config` - Configuration loading and validation
//! - `infrastructure` - Database, cache (oxcache), repositories
//! - `engines` - Scraper engines and router
//! - `services` - Application services
//! - `routes` - Route configuration and application builder

pub mod config;
pub mod engines;
pub mod infrastructure;
pub mod routes;
pub mod services;
pub mod telemetry;
