// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理提供者抽象（H2 修复：DIP 违反）
//!
//! `ReqwestEngine` 不再直接依赖具体 `ProxyPool`，改为依赖 `ProxyProvider` trait。
//! 这样：
//! - 未来可扩展 `NoopProvider`（不使用代理）、`ChainedProvider`（多池链路）等
//! - 测试时可注入 mock provider，无需构造完整 ProxyPool
//!
//! `ProxyCategory` 与 `ProxyProvider` 同文件定义：trait 方法签名引用 `ProxyCategory`，
//! 逻辑上属于 trait 的命名空间。具体 `impl ProxyProvider for ProxyPool` 位于
//! [`crate::engines::proxy_pool`] 模块（impl block 与 struct 定义同文件，符合规则10）。

/// 代理类别（§12.3：媒体 vs HTML 走不同子池）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxyCategory {
    /// HTML 页面爬取（高频小流量）
    Html,
    /// 媒体资源下载（低频大流量）
    Media,
}

/// 代理提供者抽象
///
/// 所有方法签名与 `ProxyPool` 现有方法一一对应，便于直接 impl。
///
/// # ISP（LOW-1 修复）
///
/// `mark_failure` / `mark_success` 提供默认空实现，让不需要状态回填的实现
/// （如 `NoopProvider`）只需实现 `next` / `sticky`，不被逼着实现不需要的方法。
pub trait ProxyProvider: Send + Sync {
    /// RoundRobin 取下一个可用代理（跳过冷却中），按 `category` 过滤
    ///
    /// 全部冷却或 category 无匹配时返回 `None`。
    fn next(&self, category: ProxyCategory) -> Option<String>;

    /// 粘性会话：TTL 内返回同一代理；TTL 过期或代理冷却中时重选
    ///
    /// 空池或全部冷却时返回 `None`。
    fn sticky(&self, session_id: &str) -> Option<String>;

    /// 标记失败，代理进入冷却
    ///
    /// 默认空实现：不需要状态回填的实现（如 `NoopProvider`）可跳过。
    /// `ProxyPool` 覆盖此方法以执行实际冷却逻辑。
    fn mark_failure(&self, _url: &str) {}

    /// 标记成功，恢复代理健康
    ///
    /// 默认空实现：不需要状态回填的实现（如 `NoopProvider`）可跳过。
    /// `ProxyPool` 覆盖此方法以执行实际恢复逻辑。
    fn mark_success(&self, _url: &str) {}
}
