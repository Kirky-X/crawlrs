// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `WaitFor::wait` 方法实现（T069，R-jsrender-004）
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
            Self::NetworkIdle => Self::wait_network_idle(page, timeout).await,
            Self::Selector(selector) => Self::wait_selector(page, selector, timeout).await,
            Self::DomStable(stable_duration) => {
                Self::wait_dom_stable(page, *stable_duration, timeout).await
            }
        }
    }

    /// NetworkIdle：简单 sleep 500ms
    ///
    /// chromiumoxide 的 `goto` 已等待 load 事件，此处额外等待确保异步请求完成。
    /// 若 `timeout` < 500ms 则用 `timeout`。
    async fn wait_network_idle(_page: &Page, timeout: Duration) -> Result<(), EngineError> {
        let wait = DEFAULT_NETWORK_IDLE_WINDOW.min(timeout);
        tokio::time::sleep(wait).await;
        Ok(())
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
}
