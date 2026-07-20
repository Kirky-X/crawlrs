// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// DNS基础设施模块
pub mod dns_cache;
pub mod ipv4_resolver;

pub use dns_cache::{DnsCacheService, DnsCacheStats};
pub use ipv4_resolver::{
    create_ipv4_only_resolver, create_ipv4_only_resolver_with_cache, Ipv4OnlyResolver,
};
