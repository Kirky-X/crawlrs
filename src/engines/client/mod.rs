// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 基础 HTTP 引擎模块 (始终可用)
pub mod reqwest;

/// Playwright 浏览器自动化引擎
#[cfg(feature = "engine-playwright")]
pub mod playwright;

/// Fire CDP 引擎模块
#[cfg(feature = "engine-fire-cdp")]
pub mod fire_cdp;

/// Fire TLS 引擎模块
#[cfg(feature = "engine-fire-tls")]
pub mod fire_tls;

/// FlareSolverr 引擎模块
#[cfg(feature = "engine-flaresolverr")]
pub mod flare_solverr;

// Re-exports

/// Reqwest 引擎 (始终可用)
pub use self::reqwest::ReqwestEngine;

/// Playwright 引擎
#[cfg(feature = "engine-playwright")]
pub use self::playwright::PlaywrightEngine;

/// Fire CDP 引擎
#[cfg(feature = "engine-fire-cdp")]
pub use self::fire_cdp::FireEngineCdp;

/// Fire TLS 引擎
#[cfg(feature = "engine-fire-tls")]
pub use self::fire_tls::FireEngineTls;

/// FlareSolverr 引擎
#[cfg(feature = "engine-flaresolverr")]
pub use self::flare_solverr::FlareSolverrEngine;
