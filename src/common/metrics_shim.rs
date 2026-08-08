// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Metrics shim — noop macros when `metrics` feature is disabled.
//!
//! When the `metrics` feature is enabled, this module re-exports the real
//! `metrics` crate macros. When disabled, it provides noop replacements
//! that compile away to nothing, so call sites don't need `#[cfg]` guards.

#[cfg(not(feature = "metrics"))]
mod noop {
    /// Noop handle returned by counter/histogram/gauge macros.
    /// All mutation methods are no-ops.
    pub struct NoopHandle;

    impl NoopHandle {
        pub fn increment(&self, _value: u64) {}
        pub fn record<T: Into<f64>>(&self, _value: T) {}
        pub fn set<T: Into<f64>>(&self, _value: T) {}
    }

    /// Noop `counter!` macro — matches 0, 1, or 2 label pairs.
    #[macro_export]
    macro_rules! noop_counter {
        ($name:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr, $k2:expr => $v2:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr, $k2:expr => $v2:expr, $k3:expr => $v3:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
    }

    /// Noop `histogram!` macro — matches 0, 1, or 2 label pairs.
    #[macro_export]
    macro_rules! noop_histogram {
        ($name:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr, $k2:expr => $v2:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
    }

    /// Noop `gauge!` macro — matches 0 or 1 label pairs.
    #[macro_export]
    macro_rules! noop_gauge {
        ($name:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
        ($name:expr, $k1:expr => $v1:expr) => {
            $crate::common::metrics_shim::NoopHandle
        };
    }
}

#[cfg(not(feature = "metrics"))]
pub use noop::NoopHandle;

/// Conditional re-exports: real macros when `metrics` is on, noop when off.
///
/// Usage: `use crate::common::metrics_shim::{counter, histogram, gauge};`
#[cfg(feature = "metrics")]
pub use metrics::{counter, gauge, histogram};

#[cfg(not(feature = "metrics"))]
pub use crate::noop_counter as counter;
#[cfg(not(feature = "metrics"))]
pub use crate::noop_gauge as gauge;
#[cfg(not(feature = "metrics"))]
pub use crate::noop_histogram as histogram;
