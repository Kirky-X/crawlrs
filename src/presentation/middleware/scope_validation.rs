// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scope validation extension for request handling

use crate::domain::auth::ScopePermission;
use axum::{extract::Request, http::StatusCode};
use tracing::warn;

/// Extension to extract and validate scopes from requests
pub trait ScopeValidationExt {
    fn require_scope(&self, scope: ScopePermission) -> Result<(), StatusCode>;
    fn has_scope(&self, scope: ScopePermission) -> bool;
}

impl ScopeValidationExt for Request {
    fn require_scope(&self, scope: ScopePermission) -> Result<(), StatusCode> {
        let auth_state = self
            .extensions()
            .get::<super::auth_middleware::AuthState>()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if auth_state.scope.has_permission(scope) {
            Ok(())
        } else {
            warn!(
                "Scope validation failed: required {:?} but not granted",
                scope
            );
            Err(StatusCode::FORBIDDEN)
        }
    }

    fn has_scope(&self, scope: ScopePermission) -> bool {
        self.extensions()
            .get::<super::auth_middleware::AuthState>()
            .map(|s| s.scope.has_permission(scope))
            .unwrap_or(false)
    }
}
