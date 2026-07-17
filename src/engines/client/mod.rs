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
/// 三个旧引擎名作为类型别名保留，以保持向后兼容（仅类型层面，构造方法需替换）：
/// - `FireEngineCdp` = `FlareSolverrEngine`（Cdp 模式）
/// - `FireEngineTls` = `FlareSolverrEngine`（Tls 模式）
/// - `FlareSolverrEngine` = 自身（Full 模式）
///
/// 使用时通过 `FlareSolverrEngine::with_cdp_mode()` / `with_tls_mode()` / `new()` 选择模式。
#[cfg(any(
    feature = "engine-fire-cdp",
    feature = "engine-fire-tls",
    feature = "engine-flaresolverr"
))]
pub use self::flare_solverr::{FlareSolverrConfig, FlareSolverrEngine, FlareSolverrEngineBuilder, FlareSolverrMode};

/// 向后兼容类型别名：FireEngineCdp 现在是 FlareSolverrEngine 的别名
#[cfg(feature = "engine-fire-cdp")]
#[deprecated(
    since = "0.2.0",
    note = "FireEngineCdp 已合并到 FlareSolverrEngine，请使用 FlareSolverrEngine::with_cdp_mode()"
)]
pub type FireEngineCdp = FlareSolverrEngine;

/// 向后兼容类型别名：FireEngineTls 现在是 FlareSolverrEngine 的别名
#[cfg(feature = "engine-fire-tls")]
#[deprecated(
    since = "0.2.0",
    note = "FireEngineTls 已合并到 FlareSolverrEngine，请使用 FlareSolverrEngine::with_tls_mode()"
)]
pub type FireEngineTls = FlareSolverrEngine;
