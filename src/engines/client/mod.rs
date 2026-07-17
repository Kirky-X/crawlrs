// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 基础 HTTP 引擎模块 (始终可用)
pub mod reqwest;

/// Playwright 浏览器自动化引擎
#[cfg(feature = "engine-playwright")]
pub mod playwright;

/// Playwright 浏览器实例池
#[cfg(feature = "engine-playwright")]
pub mod playwright_pool;

/// FlareSolverr 引擎模块（合并了原 fire_cdp / fire_tls / flaresolverr 三引擎）
///
/// 通过 `FlareSolverrMode` 枚举区分 Full / Cdp / Tls 三种工作模式：
/// - `Full`：完整 FlareSolverr 客户端（原 flaresolverr）
/// - `Cdp`：CDP 模式（原 fire_cdp）
/// - `Tls`：TLS 指纹模式（原 fire_tls）
#[cfg(any(
    feature = "engine-fire-cdp",
    feature = "engine-fire-tls",
    feature = "engine-flaresolverr"
))]
pub mod flare_solverr;

/// 共享的 FlareSolverr 类型定义
#[cfg(any(
    feature = "engine-fire-cdp",
    feature = "engine-fire-tls",
    feature = "engine-flaresolverr"
))]
pub mod flaresolverr_types;

// Re-exports

/// Reqwest 引擎 (始终可用)
pub use self::reqwest::ReqwestEngine;

/// Playwright 引擎
#[cfg(feature = "engine-playwright")]
pub use self::playwright::PlaywrightEngine;

/// 浏览器池
#[cfg(feature = "engine-playwright")]
pub use self::playwright_pool::{
    get_global_pool, init_global_pool, shutdown_global_pool, BrowserInstance, BrowserPool,
    BrowserPoolConfig, BrowserPoolStats,
};

/// 统一的 FlareSolverr 引擎（合并原 FireEngineCdp / FireEngineTls / FlareSolverrEngine）
///
/// 原 `FireEngineCdp` / `FireEngineTls` / `FlareSolverrEngine` 三个独立引擎
/// 已合并为 `FlareSolverrEngine` + `FlareSolverrMode` 枚举。原类型别名已删除
/// （无生产代码使用，避免误导用户继续使用旧 API）。
///
/// 使用方式：
/// - `FlareSolverrEngine::new()` — Full 模式（原 FlareSolverrEngine）
/// - `FlareSolverrEngine::with_cdp_mode()` — Cdp 模式（原 FireEngineCdp）
/// - `FlareSolverrEngine::with_tls_mode()` — Tls 模式（原 FireEngineTls）
#[cfg(any(
    feature = "engine-fire-cdp",
    feature = "engine-fire-tls",
    feature = "engine-flaresolverr"
))]
pub use self::flare_solverr::{FlareSolverrConfig, FlareSolverrEngine, FlareSolverrEngineBuilder, FlareSolverrMode};
