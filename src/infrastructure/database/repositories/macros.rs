// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Repository macro utilities for standardizing repository implementations.
//!
//! This module provides macros to reduce boilerplate in repository implementations.

/// Macro to generate a standard constructor for repositories.
///
/// # Usage
///
/// ```ignore
/// use crate::infrastructure::database::repositories::macros::repository_new;
///
/// struct MyRepository {
///     db: Arc<DatabaseConnection>,
/// }
///
/// repository_new!(MyRepository);
/// ```
///
/// This expands to:
/// ```ignore
/// impl MyRepository {
///     pub fn new(db: Arc<DatabaseConnection>) -> Self {
///         Self { db }
///     }
/// }
/// ```
#[macro_export]
macro_rules! repository_new {
    ($struct:ident) => {
        impl $struct {
            /// Creates a new repository instance.
            pub fn new(db: sea_orm::DatabaseConnection) -> Self {
                Self { db }
            }
        }
    };
}

/// Macro to generate a standard constructor for repositories with additional fields.
///
/// # Usage
///
/// ```ignore
/// use crate::infrastructure::database::repositories::macros::repository_new_with;
///
/// struct TaskRepositoryImpl {
///     db: Arc<DatabaseConnection>,
///     lock_duration: chrono::Duration,
/// }
///
/// repository_new_with!(TaskRepositoryImpl, lock_duration);
/// ```
///
/// This expands to:
/// ```ignore
/// impl TaskRepositoryImpl {
///     pub fn new(
///         db: Arc<DatabaseConnection>,
///         lock_duration: chrono::Duration,
///     ) -> Self {
///         Self { db, lock_duration }
///     }
/// }
/// ```
#[macro_export]
macro_rules! repository_new_with {
    ($struct:ident, $($field:ident),+) => {
        impl $struct {
            /// Creates a new repository instance.
            pub fn new(
                db: sea_orm::DatabaseConnection,
                $($field: chrono::Duration),+
            ) -> Self {
                Self {
                    db,
                    $($field),+
                }
            }
        }
    };
}
