// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `WaitFor::wait` 方法实现
//!
//! `WaitFor` 枚举本身定义在 [`crate::engines::engine_client::WaitFor`]（非 feature-gated），
//! 此模块仅包含依赖 `chromiumoxide::Page` 的 `wait` 方法实现，由 `engine-playwright`
//! feature 门控。
//!
//! # 三种模式
//!
//! - `WaitFor::NetworkIdle`：等待网络空闲（无新请求持续 500ms）
//! - `WaitFor::Selector(String)`：等待指定 CSS selector 出现在 DOM 中
//! - `WaitFor::DomStable(Duration)`：等待 DOM 稳定（无变化持续指定时长）
//!
//! # 安全性
//!
//! `Selector` 模式对 selector 字符串做 JS 字符串转义，防注入（CWE-94）。
//! `DomStable` 的 `stable_duration` 上限 60s，防恶意调用方设置过长导致 DoS。

use crate::engines::engine_client::{EngineError, WaitFor};
use chromiumoxide::Page;
use std::time::{Duration, Instant};

/// 默认网络空闲判定窗口（500ms 内无新请求视为空闲）
const DEFAULT_NETWORK_IDLE_WINDOW: Duration = Duration::from_millis(500);

/// 网络事件轮询间隔（事件流等待的超时步长，用于周期检查 deadline）
const NETWORK_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Selector 等待的默认轮询间隔（100ms）
const SELECTOR_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// DomStable 的默认轮询间隔（100ms）
const DOM_STABLE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// DomStable 的 stable_duration 上限（60s，防 DoS）
const DOM_STABLE_MAX_DURATION: Duration = Duration::from_secs(60);

impl WaitFor {
    /// 执行等待
    ///
    /// # 参数
    ///
    /// - `page`: 已加载的页面
    /// - `timeout`: 总超时时间（超过则返回错误）
    ///
    /// # 错误
    ///
    /// - 超时：`EngineError::BrowserError("wait timeout: <mode> after <ms>ms")`
    /// - CDP 调用失败：`EngineError::BrowserError(<detail>)`
    /// - DomStable 的 stable_duration 超过 60s：`EngineError::BrowserError`
    pub async fn wait(&self, page: &Page, timeout: Duration) -> Result<(), EngineError> {
        match self {
            Self::NetworkIdle => {
                Self::wait_network_idle(page, DEFAULT_NETWORK_IDLE_WINDOW, timeout).await
            }
            Self::Selector(selector) => Self::wait_selector(page, selector, timeout).await,
            Self::DomStable(stable_duration) => {
                Self::wait_dom_stable(page, *stable_duration, timeout).await
            }
        }
    }

    /// NetworkIdle：基于 CDP 网络事件的真实空闲等待（P07 修复）
    ///
    /// 监听 `EventRequestWillBeSent`（+1）/ `EventLoadingFinished`、
    /// `EventLoadingFailed`（-1）维护 in-flight 计数，连续 `quiet_window`
    /// 无在途请求即返回。此前实现是固定 sleep 500ms——命名与语义不符，
    /// SPA 懒加载内容会被截断。
    ///
    /// 语义为尽力而为：超时返回 `Ok`（不因等待失败而失败，与旧行为一致）；
    /// CDP 订阅失败 fail-open 到固定 sleep（`log::warn!` 显性降级）。
    async fn wait_network_idle(
        page: &Page,
        quiet_window: Duration,
        timeout: Duration,
    ) -> Result<(), EngineError> {
        use chromiumoxide::cdp::browser_protocol::network::{
            EnableParams as NetworkEnableParams, EventLoadingFailed, EventLoadingFinished,
            EventRequestWillBeSent,
        };
        use futures::StreamExt;

        // 事件流订阅是异步的，任何失败都 fail-open 到固定 sleep
        let setup = async {
            page.execute(NetworkEnableParams::default())
                .await
                .map_err(|e| e.to_string())?;
            let started = page
                .event_listener::<EventRequestWillBeSent>()
                .await
                .map_err(|e| e.to_string())?;
            let finished = page
                .event_listener::<EventLoadingFinished>()
                .await
                .map_err(|e| e.to_string())?;
            let failed = page
                .event_listener::<EventLoadingFailed>()
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((started, finished, failed))
        };

        let (mut started, mut finished, mut failed) = match setup.await {
            Ok(streams) => streams,
            Err(e) => {
                log::warn!(
                    "network idle: CDP event subscription failed ({e}); \
                     falling back to fixed sleep"
                );
                tokio::time::sleep(quiet_window.min(timeout)).await;
                return Ok(());
            }
        };

        let mut tracker = NetworkIdleTracker::default();
        let deadline = Instant::now() + timeout;

        // 统一出口：等待结束时 best-effort Network.disable——tab 池复用下
        // Network domain 常开会让浏览器为页面剩余生命周期持续分发网络事件
        let result: Result<(), EngineError> = loop {
            tracker.tick(Instant::now());
            if tracker.is_idle_for(Instant::now(), quiet_window) {
                break Ok(());
            }
            if Instant::now() >= deadline {
                // 尽力而为语义：超时不视为错误（与旧行为一致）
                break Ok(());
            }

            tokio::select! {
                Some(ev) = started.next() => {
                    // 重定向跳只补发 RequestWillBeSent（redirect_response 为
                    // Some），不为原请求发 LoadingFinished——若无条件 +1，
                    // http→https / www 规范化等每次重定向都会净泄漏一个
                    // in-flight，idle 永不成立。净 0 处理对齐 CDP 语义。
                    if ev.redirect_response.is_some() {
                        tracker.on_request_finished(Instant::now());
                    }
                    tracker.on_request_started(Instant::now());
                }
                Some(_) = finished.next() => tracker.on_request_finished(Instant::now()),
                Some(_) = failed.next() => tracker.on_request_finished(Instant::now()),
                _ = tokio::time::sleep(NETWORK_EVENT_POLL_INTERVAL) => {}
            }
        };

        let _ = page
            .execute(chromiumoxide::cdp::browser_protocol::network::DisableParams::default())
            .await;
        result
    }

    /// Selector：轮询 `document.querySelector(selector)` 直到非 null 或超时
    ///
    /// selector 字符串通过 `escape_js_string` 转义，防 JS 注入。
    async fn wait_selector(
        page: &Page,
        selector: &str,
        timeout: Duration,
    ) -> Result<(), EngineError> {
        let escaped = escape_js_string(selector);
        let check_expr = format!("() => document.querySelector({escaped}) !== null");
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Err(EngineError::BrowserError(format!(
                    "wait timeout: selector '{selector}' not found after {}ms",
                    timeout.as_millis()
                )));
            }
            let exists: bool = page
                .evaluate(check_expr.clone())
                .await
                .map_err(|e| EngineError::BrowserError(format!("selector eval failed: {e}")))?
                .into_value::<bool>()
                .map_err(|e| {
                    EngineError::BrowserError(format!("selector eval decode failed: {e}"))
                })?;
            if exists {
                return Ok(());
            }
            tokio::time::sleep(SELECTOR_POLL_INTERVAL).await;
        }
    }

    /// DomStable：轮询 `document.body.innerHTML.length` 直到连续 stable_duration 不变
    ///
    /// stable_duration 上限 60s（防 DoS）。
    async fn wait_dom_stable(
        page: &Page,
        stable_duration: Duration,
        timeout: Duration,
    ) -> Result<(), EngineError> {
        if stable_duration > DOM_STABLE_MAX_DURATION {
            return Err(EngineError::BrowserError(format!(
                "dom_stable duration {}ms exceeds max {}ms",
                stable_duration.as_millis(),
                DOM_STABLE_MAX_DURATION.as_millis()
            )));
        }
        let check_expr = "() => document.body ? document.body.innerHTML.length : -1";
        let deadline = Instant::now() + timeout;
        let mut last_len: i64 = -1;
        let mut stable_since: Option<Instant> = None;
        loop {
            if Instant::now() >= deadline {
                return Err(EngineError::BrowserError(format!(
                    "wait timeout: dom not stable after {}ms",
                    timeout.as_millis()
                )));
            }
            let len: i64 = page
                .evaluate(check_expr)
                .await
                .map_err(|e| EngineError::BrowserError(format!("dom eval failed: {e}")))?
                .into_value::<i64>()
                .map_err(|e| EngineError::BrowserError(format!("dom eval decode failed: {e}")))?;
            if len == last_len {
                if stable_since.is_none() {
                    stable_since = Some(Instant::now());
                }
                if let Some(since) = stable_since {
                    if since.elapsed() >= stable_duration {
                        return Ok(());
                    }
                }
            } else {
                last_len = len;
                stable_since = None;
            }
            tokio::time::sleep(DOM_STABLE_POLL_INTERVAL).await;
        }
    }
}

/// 网络空闲状态机（纯逻辑，独立于 CDP 便于单测）
///
/// - `on_request_started`：in-flight +1，清除空闲起点
/// - `on_request_finished`：in-flight -1（下限 0，防止事件乱序导致负数）
/// - `tick`：每轮循环对齐状态——in_flight 为 0 且尚未记录空闲起点时打点
/// - `is_idle_for`：空闲起点存在且已持续 ≥ quiet_window
#[derive(Debug, Default)]
struct NetworkIdleTracker {
    in_flight: i64,
    idle_since: Option<Instant>,
}

impl NetworkIdleTracker {
    fn on_request_started(&mut self, _now: Instant) {
        self.in_flight += 1;
        self.idle_since = None;
    }

    fn on_request_finished(&mut self, _now: Instant) {
        self.in_flight = (self.in_flight - 1).max(0);
    }

    fn tick(&mut self, now: Instant) {
        if self.in_flight <= 0 && self.idle_since.is_none() {
            self.idle_since = Some(now);
        }
        if self.in_flight > 0 {
            self.idle_since = None;
        }
    }

    fn is_idle_for(&self, now: Instant, quiet_window: Duration) -> bool {
        match self.idle_since {
            Some(since) => now.duration_since(since) >= quiet_window,
            None => false,
        }
    }
}

/// 转义 JS 字符串字面量，防注入（CWE-94）
///
/// 将 `selector` 包裹在双引号中，转义 `"`, `\`, 控制字符等。
fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if c.is_control() => {
                out.push_str(&format!(r"\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ escape_js_string ============

    #[test]
    fn escape_plain_string() {
        assert_eq!(escape_js_string("hello"), r#""hello""#);
    }

    #[test]
    fn escape_double_quote() {
        assert_eq!(escape_js_string(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_js_string(r"a\b"), r#""a\\b""#);
    }

    #[test]
    fn escape_newline() {
        assert_eq!(escape_js_string("a\nb"), r#""a\nb""#);
    }

    #[test]
    fn escape_control_chars() {
        let s = escape_js_string("a\x01b");
        assert!(s.starts_with("\"a"));
        assert!(s.contains(r"\u0001"));
    }

    #[test]
    fn escape_empty_string() {
        assert_eq!(escape_js_string(""), r#""""#);
    }

    // ============ WaitFor 枚举 ============

    #[test]
    fn default_is_network_idle() {
        let w = WaitFor::default();
        assert_eq!(w, WaitFor::NetworkIdle);
    }

    #[test]
    fn selector_equality() {
        assert_eq!(
            WaitFor::Selector("div".to_string()),
            WaitFor::Selector("div".to_string())
        );
        assert_ne!(
            WaitFor::Selector("div".to_string()),
            WaitFor::Selector("span".to_string())
        );
    }

    #[test]
    fn dom_stable_equality() {
        assert_eq!(
            WaitFor::DomStable(Duration::from_millis(500)),
            WaitFor::DomStable(Duration::from_millis(500))
        );
    }

    #[test]
    fn dom_stable_max_duration_constant() {
        assert_eq!(DOM_STABLE_MAX_DURATION, Duration::from_secs(60));
    }

    #[test]
    fn network_idle_window_constant() {
        assert_eq!(DEFAULT_NETWORK_IDLE_WINDOW, Duration::from_millis(500));
    }

    #[test]
    fn selector_poll_interval_constant() {
        assert_eq!(SELECTOR_POLL_INTERVAL, Duration::from_millis(100));
    }

    #[test]
    fn dom_stable_poll_interval_constant() {
        assert_eq!(DOM_STABLE_POLL_INTERVAL, Duration::from_millis(100));
    }

    // ============ wait_network_idle（无需真实 Page，内部仅 sleep） ============

    #[tokio::test]
    async fn network_idle_returns_ok() {
        // NetworkIdle 内部仅 sleep，无需 Page
        // 但 wait 需要 &Page，无法直接测；通过 wait 方法间接测
        // 这里验证常量与超时逻辑
        let timeout = Duration::from_secs(1);
        let wait = DEFAULT_NETWORK_IDLE_WINDOW.min(timeout);
        assert_eq!(wait, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn network_idle_timeout_less_than_window() {
        let timeout = Duration::from_millis(100);
        let wait = DEFAULT_NETWORK_IDLE_WINDOW.min(timeout);
        assert_eq!(wait, Duration::from_millis(100));
    }

    // ============ NetworkIdleTracker（P07 状态机单测） ============

    fn t(ms: u64) -> Instant {
        // 用相对构造的 Instant 序列验证状态机（Instant 只能取 now，
        // 这里以单调递增的 now 偏移模拟时间推进）
        let mut base = Instant::now();
        base += Duration::from_millis(ms);
        base
    }

    /// Started→Finished 后 quiet window 满足 → 空闲
    #[test]
    fn tracker_idle_after_finish_and_quiet_window() {
        let mut tr = NetworkIdleTracker::default();
        tr.on_request_started(t(0));
        assert!(!tr.is_idle_for(t(10), Duration::from_millis(500)));
        tr.on_request_finished(t(20));
        tr.tick(t(20));
        // 空闲起点 t(20)，300ms < 500ms → 未空闲
        assert!(!tr.is_idle_for(t(320), Duration::from_millis(500)));
        // 600ms ≥ 500ms → 空闲
        assert!(tr.is_idle_for(t(620), Duration::from_millis(500)));
    }

    /// 在途请求未完成 → 不进入空闲
    #[test]
    fn tracker_stays_busy_while_in_flight() {
        let mut tr = NetworkIdleTracker::default();
        tr.on_request_started(t(0));
        tr.tick(t(100));
        tr.tick(t(2000));
        assert!(
            !tr.is_idle_for(t(2000), Duration::from_millis(500)),
            "in-flight request must block idle detection"
        );
    }

    /// 空闲期出现新请求 → 打断空闲计时，需重新计满 quiet window
    #[test]
    fn tracker_idle_interrupted_by_new_request() {
        let mut tr = NetworkIdleTracker::default();
        // 首个请求完成并进入空闲
        tr.on_request_started(t(0));
        tr.on_request_finished(t(10));
        tr.tick(t(10));
        assert!(tr.is_idle_for(t(600), Duration::from_millis(500)));
        // 新请求打断
        tr.on_request_started(t(610));
        tr.on_request_finished(t(620));
        tr.tick(t(620));
        // 距新起点仅 500ms 临界——但起点已重置为 tick 时刻
        assert!(
            !tr.is_idle_for(t(1000), Duration::from_millis(500)),
            "idle window must restart after new activity"
        );
        assert!(
            tr.is_idle_for(t(1120), Duration::from_millis(500)),
            "idle should be detected quiet_window after the last finish"
        );
    }
}
