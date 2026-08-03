// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Chrome CDP Tab 池（T068，R-jsrender-004）
//!
//! 移植 spider `tab_pool.rs`，提供 [`chromiumoxide::Page`]（tab）级复用：
//!
//! - **parking_lot Mutex + Vec LIFO 栈**：短临界区锁，尾端 push/pop
//! - **acquire**：优先从池中弹出最近归还的 Page（LIFO），池空时调用
//!   `browser.new_page("about:blank")` 新建
//! - **release**：将 Page 导航到 `about:blank` 清理状态（5s 超时），再压回栈
//! - **容量限制**：达到 `max_size` 时多余 Page 直接 drop（关闭 tab）
//!
//! # 设计动机
//!
//! BrowserPool 复用 Browser 实例（约 500ms-2s 启动延迟），但每次请求仍调用
//! `browser.new_page("about:blank")` 创建新 tab（约 50-200ms）。TabPool 在此基础上
//! 进一步复用 Page，消除 tab 创建开销。
//!
//! # 多 Browser 安全
//!
//! TabPool 不绑定特定 Browser：`acquire` 接受 `&Browser` 参数，若池空则用该 Browser
//! 新建 Page；若池非空则弹出已存在的 Page（可能属于其他 Browser）。
//!
//! **跨 Browser 使用风险**：chromiumoxide 的 Page 持有 CDP session，session 与
//! Browser 关联，跨 Browser 使用会在 CDP 调用时失败。调用方应保证 TabPool 的
//! 生命周期与单个 Browser 一致（per-Browser 实例化），或在多 Browser 场景下
//! 由上层（如 [`super::playwright_pool::BrowserPool`]）按 instance_id 路由。
//!
//! # 参考
//!
//! - spider `spider/src/utils/tab_pool.rs`
//! - design.md §17

use chromiumoxide::{Browser, Page};
use std::time::Duration;

/// Page 导航到 `about:blank` 清理状态的超时时间
///
/// 超过此时间认为 Page 不可清理，直接 drop（关闭 tab）。
/// 与 spider 默认值一致（5s）。
const RESET_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(5);

/// Chrome CDP Tab 池（T068，R-jsrender-004）
///
/// parking_lot::Mutex 保护 Vec 尾端 push/pop 实现 LIFO，
/// 短临界区锁（无 await 在锁内），性能优于 DashMap per-shard 开销。
///
/// # 示例
///
/// ```ignore
/// use crate::engines::client::tab_pool::TabPool;
/// use chromiumoxide::Browser;
///
/// let pool = TabPool::new(10);
/// // 获取 Page（池空时新建）
/// let page = pool.acquire(&browser).await?;
/// // ... 使用 page ...
/// // 归还（导航到 about:blank 清理后压栈）
/// pool.release(page).await;
/// ```
pub struct TabPool {
    /// LIFO 栈：尾端 push/pop。Mutex 保护并发访问。
    pages: parking_lot::Mutex<Vec<Page>>,
    /// 最大池容量
    max_size: usize,
}

impl TabPool {
    /// 创建指定最大容量的 TabPool
    ///
    /// # 参数
    ///
    /// - `max_size`: 池最大容量。0 表示禁用复用（所有 acquire 直接 new_page，
    ///   所有 release 直接 drop）
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            pages: parking_lot::Mutex::new(Vec::with_capacity(max_size)),
            max_size,
        }
    }

    /// 从池中获取一个 Page，池空时通过 `browser.new_page("about:blank")` 新建
    ///
    /// LIFO 语义：优先弹出最近归还的 Page（栈顶）。
    ///
    /// # 错误
    ///
    /// 池空且 `browser.new_page` 失败时返回 [`chromiumoxide::error::CdpError`]。
    pub async fn acquire(&self, browser: &Browser) -> Result<Page, chromiumoxide::error::CdpError> {
        // 短临界区锁：pop 后立即释放
        {
            let mut pages = self.pages.lock();
            if let Some(page) = pages.pop() {
                return Ok(page);
            }
        }
        // 池空 — 锁已释放，安全 await
        browser.new_page("about:blank").await
    }

    /// 归还 Page 到池中
    ///
    /// # 行为
    ///
    /// 1. 若池已达 `max_size`，直接 drop Page（关闭 tab）
    /// 2. 导航到 `about:blank` 清理状态（5s 超时）
    /// 3. 导航失败/超时则 drop Page
    /// 4. CAS 循环压栈到下一个可用 slot
    ///
    /// # 注意
    ///
    /// 此方法是 async 的（需要导航），不能在 Drop 中直接 await。
    /// 上层应通过 channel + 后台 task 的方式异步调用（参考 [`super::playwright_pool::BrowserInstance`]）。
    pub async fn release(&self, page: Page) {
        // 容量检查：已达上限直接 drop
        {
            let pages = self.pages.lock();
            if pages.len() >= self.max_size {
                return; // page drop 在函数返回时
            }
        }

        // 导航到 about:blank 清理状态（5s 超时）
        let ok = matches!(
            tokio::time::timeout(RESET_NAVIGATION_TIMEOUT, page.goto("about:blank")).await,
            Ok(Ok(_))
        );

        if !ok {
            return; // 导航失败/超时，drop Page
        }

        // 短临界区锁：push 后立即释放
        let mut pages = self.pages.lock();
        if pages.len() < self.max_size {
            pages.push(page);
        }
        // 锁在作用域结束时自动释放
    }

    /// 清空池中所有 Page（drop 全部缓存 tab）
    ///
    /// 正在被 acquire 的 Page 不受影响（已被取出）。
    pub fn clear(&self) {
        self.pages.lock().clear();
    }

    /// 当前池中缓存（空闲）的 Page 数量
    ///
    /// 返回 Mutex 保护下的快照值，并发场景下仅供参考。
    #[must_use]
    pub fn pool_size(&self) -> usize {
        self.pages.lock().len()
    }

    /// 池的最大容量
    #[must_use]
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 池是否为空（无缓存 Page）
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pool_size() == 0
    }

    /// 池是否已达容量上限
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.pool_size() >= self.max_size
    }
}

impl std::fmt::Debug for TabPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabPool")
            .field("pool_size", &self.pool_size())
            .field("max_size", &self.max_size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ 构造与基础属性 ============

    #[test]
    fn new_pool_is_empty() {
        let pool = TabPool::new(5);
        assert!(pool.is_empty());
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn new_pool_not_full() {
        let pool = TabPool::new(5);
        assert!(!pool.is_full());
    }

    #[test]
    fn max_size_zero_disables_reuse() {
        // max_size=0 → 禁用复用，所有 acquire 走 new_page，所有 release 直接 drop
        let pool = TabPool::new(0);
        assert_eq!(pool.max_size(), 0);
        assert!(pool.is_empty());
        assert!(pool.is_full()); // 0 >= 0
    }

    #[test]
    fn max_size_positive() {
        let pool = TabPool::new(100);
        assert_eq!(pool.max_size(), 100);
        assert!(!pool.is_full());
    }

    #[test]
    fn debug_format_contains_size_and_max() {
        let pool = TabPool::new(10);
        let s = format!("{:?}", pool);
        assert!(s.contains("pool_size"));
        assert!(s.contains("max_size"));
        assert!(s.contains("0"));
        assert!(s.contains("10"));
    }

    // ============ clear ============

    #[test]
    fn clear_empty_pool_no_op() {
        let pool = TabPool::new(5);
        pool.clear();
        assert!(pool.is_empty());
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn clear_resets_head_to_zero() {
        // clear 不依赖 slots 内容，仅重置 head 和清空 map
        // 无 Page 也能调用 clear（验证不会 panic）
        let pool = TabPool::new(5);
        pool.clear();
        assert_eq!(pool.pool_size(), 0);
    }

    // ============ is_full 边界 ============

    #[test]
    fn is_full_when_size_equals_max() {
        // 模拟 is_full 判定（无法真正插入 Page，测纯逻辑）
        // max_size=0 → 任何状态都 is_full（0 >= 0）
        let pool = TabPool::new(0);
        assert!(pool.is_full());

        // max_size=1 但 pool_size=0 → 不满
        let pool2 = TabPool::new(1);
        assert!(!pool2.is_full());
    }

    // ============ 常量 ============

    #[test]
    fn reset_navigation_timeout_is_5s() {
        // 与 spider 默认值一致
        assert_eq!(RESET_NAVIGATION_TIMEOUT, Duration::from_secs(5));
    }
}
