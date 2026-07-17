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

    /// Create a new scope with custom permission flags and rate limits
    ///
    /// Allows external callers (including integration tests) to construct an
    /// `ApiKeyScope` with custom `search_limit` / `scrape_limit` values without
    /// directly accessing the `pub(crate)` fields.
    pub fn with_custom_limits(
        read: bool,
        write: bool,
        admin: bool,
        search_limit: u32,
        scrape_limit: u32,
    ) -> Self {
        Self {
            read,
            write,
            admin,
            search_limit,
            scrape_limit,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scope() {
        let scope = ApiKeyScope::default();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_denied_scope() {
        let scope = ApiKeyScope::denied();
        assert!(!scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 0);
        assert_eq!(scope.scrape_limit, 0);
    }

    #[test]
    fn test_read_only_scope() {
        let scope = ApiKeyScope::read_only();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_full_access_scope() {
        let scope = ApiKeyScope::full_access();
        assert!(scope.read);
        assert!(scope.write);
        assert!(scope.admin);
        assert_eq!(scope.search_limit, u32::MAX);
        assert_eq!(scope.scrape_limit, u32::MAX);
    }

    #[test]
    fn test_with_custom_limits() {
        let scope = ApiKeyScope::with_custom_limits(true, false, false, 200, 100);
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 200);
        assert_eq!(scope.scrape_limit, 100);
    }

    #[test]
    fn test_has_permission_read() {
        let read_only = ApiKeyScope::read_only();
        assert!(read_only.has_permission(ScopePermission::Read));
        assert!(!read_only.has_permission(ScopePermission::Write));
        assert!(!read_only.has_permission(ScopePermission::Admin));
    }

    #[test]
    fn test_has_permission_write() {
        let scope: ApiKeyScope = ScopePermission::Write.into();
        assert!(scope.has_permission(ScopePermission::Read));
        assert!(scope.has_permission(ScopePermission::Write));
        assert!(!scope.has_permission(ScopePermission::Admin));
    }

    #[test]
    fn test_has_permission_admin() {
        let scope: ApiKeyScope = ScopePermission::Admin.into();
        assert!(scope.has_permission(ScopePermission::Read));
        assert!(scope.has_permission(ScopePermission::Write));
        assert!(scope.has_permission(ScopePermission::Admin));
    }

    #[test]
    fn test_has_permission_denied() {
        let denied = ApiKeyScope::denied();
        assert!(!denied.has_permission(ScopePermission::Read));
        assert!(!denied.has_permission(ScopePermission::Write));
        assert!(!denied.has_permission(ScopePermission::Admin));
    }

    #[test]
    fn test_allows_search_count_within_limit() {
        let scope = ApiKeyScope::read_only();
        assert!(scope.allows_search_count(50));
        assert!(scope.allows_search_count(100));
        assert!(!scope.allows_search_count(101));
    }

    #[test]
    fn test_allows_search_count_unlimited() {
        let scope = ApiKeyScope::full_access();
        assert!(scope.allows_search_count(0));
        assert!(scope.allows_search_count(u32::MAX));
        assert!(scope.allows_search_count(1_000_000));
    }

    #[test]
    fn test_allows_search_count_zero_limit() {
        let scope = ApiKeyScope::denied();
        assert!(scope.allows_search_count(0));
        assert!(!scope.allows_search_count(1));
    }

    #[test]
    fn test_allows_scrape_count_within_limit() {
        let scope = ApiKeyScope::read_only();
        assert!(scope.allows_scrape_count(25));
        assert!(scope.allows_scrape_count(50));
        assert!(!scope.allows_scrape_count(51));
    }

    #[test]
    fn test_allows_scrape_count_unlimited() {
        let scope = ApiKeyScope::full_access();
        assert!(scope.allows_scrape_count(0));
        assert!(scope.allows_scrape_count(u32::MAX));
        assert!(scope.allows_scrape_count(1_000_000));
    }

    #[test]
    fn test_allows_scrape_count_zero_limit() {
        let scope = ApiKeyScope::denied();
        assert!(scope.allows_scrape_count(0));
        assert!(!scope.allows_scrape_count(1));
    }

    #[test]
    fn test_api_key_scope_display() {
        let scope = ApiKeyScope::default();
        let s = scope.to_string();
        assert!(s.contains("ApiKeyScope("));
        assert!(s.contains("read=true"));
        assert!(s.contains("write=false"));
        assert!(s.contains("admin=false"));
        assert!(s.contains("search_limit=100"));
        assert!(s.contains("scrape_limit=50"));
    }

    #[test]
    fn test_api_key_scope_display_full_access() {
        let scope = ApiKeyScope::full_access();
        let s = scope.to_string();
        assert!(s.contains("read=true"));
        assert!(s.contains("write=true"));
        assert!(s.contains("admin=true"));
    }

    #[test]
    fn test_scope_permission_display() {
        assert_eq!(ScopePermission::Read.to_string(), "read");
        assert_eq!(ScopePermission::Write.to_string(), "write");
        assert_eq!(ScopePermission::Admin.to_string(), "admin");
    }

    #[test]
    fn test_from_read_permission() {
        let scope: ApiKeyScope = ScopePermission::Read.into();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_from_write_permission() {
        let scope: ApiKeyScope = ScopePermission::Write.into();
        assert!(scope.read);
        assert!(scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_from_admin_permission() {
        let scope: ApiKeyScope = ScopePermission::Admin.into();
        assert!(scope.read);
        assert!(scope.write);
        assert!(scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }
}
