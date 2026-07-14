// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Implementation of [`ApiKeyScope`] and [`ScopePermission`].

use super::{ApiKeyScope, ScopePermission};

impl Default for ApiKeyScope {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }
}

impl std::fmt::Display for ApiKeyScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ApiKeyScope(read={}, write={}, admin={}, search_limit={}, scrape_limit={})",
            self.read, self.write, self.admin, self.search_limit, self.scrape_limit
        )
    }
}

impl ApiKeyScope {
    /// Create a new scope with all permissions disabled
    pub fn denied() -> Self {
        Self {
            read: false,
            write: false,
            admin: false,
            search_limit: 0,
            scrape_limit: 0,
        }
    }

    /// Create a new scope with read-only access
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }

    /// Create a new scope with full access
    pub fn full_access() -> Self {
        Self {
            read: true,
            write: true,
            admin: true,
            search_limit: u32::MAX,
            scrape_limit: u32::MAX,
        }
    }

    /// Check if the scope has permission for a required scope
    pub fn has_permission(&self, permission: ScopePermission) -> bool {
        match permission {
            ScopePermission::Read => self.read,
            ScopePermission::Write => self.write,
            ScopePermission::Admin => self.admin,
        }
    }

    /// Check if the scope allows the requested search count
    pub fn allows_search_count(&self, count: u32) -> bool {
        self.search_limit == u32::MAX || count <= self.search_limit
    }

    /// Check if the scope allows the requested scrape count
    pub fn allows_scrape_count(&self, count: u32) -> bool {
        self.scrape_limit == u32::MAX || count <= self.scrape_limit
    }
}

impl std::fmt::Display for ScopePermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopePermission::Read => write!(f, "read"),
            ScopePermission::Write => write!(f, "write"),
            ScopePermission::Admin => write!(f, "admin"),
        }
    }
}

impl From<ScopePermission> for ApiKeyScope {
    fn from(permission: ScopePermission) -> Self {
        match permission {
            ScopePermission::Read => Self {
                read: true,
                write: false,
                admin: false,
                search_limit: 100,
                scrape_limit: 50,
            },
            ScopePermission::Write => Self {
                read: true,
                write: true,
                admin: false,
                search_limit: 100,
                scrape_limit: 50,
            },
            ScopePermission::Admin => Self {
                read: true,
                write: true,
                admin: true,
                search_limit: 100,
                scrape_limit: 50,
            },
        }
    }
}
