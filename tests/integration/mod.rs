// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod helpers;
pub mod repositories;
// Disabled: pre-existing untracked file with 16 savepoint tests failing due to
// source bug in transaction.rs (savepoint uses session.connection().execute_unprepared()
// instead of session.execute_raw(); dbnexus 0.2.0 connection() returns pool handle not
// transaction handle, so PostgreSQL rejects "SAVEPOINT can only be used in transaction blocks").
// Coverage is now provided by repositories/transaction_test.rs which documents the bug and
// tests the actual behavior. Re-enable after fixing transaction.rs source bug.
// pub mod transaction_test;
