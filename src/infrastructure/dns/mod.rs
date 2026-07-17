// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// DNS基础设施模块
#[cfg(feature = "oxcache-cache")]
pub mod dns_cache;

#[cfg(feature = "oxcache-cache")]
pub use dns_cache::{DnsCacheService, DnsCacheStats};
