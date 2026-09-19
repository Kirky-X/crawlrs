// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

/// DNS基础设施模块
pub mod dns_cache;
pub mod ipv4_resolver;

pub use dns_cache::{DnsCacheService, DnsCacheStats};
pub use ipv4_resolver::{
    create_ipv4_only_resolver, create_ipv4_only_resolver_with_cache, Ipv4OnlyResolver,
};
