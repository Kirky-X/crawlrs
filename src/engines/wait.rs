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

/// ResponseBody 等待的单响应体匹配检查上限（对齐默认 max_response_body_bytes）
const WAIT_BODY_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// NetworkIdle 所需的三个网络事件流。
type NetworkEventStreams = (
    chromiumoxide::listeners::EventStream<
        chromiumoxide::cdp::browser_protocol::network::EventRequestWillBeSent,
    >,
    chromiumoxide::listeners::EventStream<
        chromiumoxide::cdp::browser_protocol::network::EventLoadingFinished,
    >,
    chromiumoxide::listeners::EventStream<
        chromiumoxide::cdp::browser_protocol::network::EventLoadingFailed,
    >,
);

/// ResponseBody 等待中「待取体」的响应表（request_id → encoded_data_length）。
type BodyPendingResponses =
    std::collections::HashMap<chromiumoxide::cdp::browser_protocol::network::RequestId, u64>;

/// 启用 Network domain 并订阅三个网络事件流；任一失败返回错误字符串。
async fn subscribe_network_events(page: &Page) -> Result<NetworkEventStreams, String> {
    use chromiumoxide::cdp::browser_protocol::network::{
        EnableParams as NetworkEnableParams, EventLoadingFailed, EventLoadingFinished,
        EventRequestWillBeSent,
    };

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
    Ok((started, finished, failed))
}

/// best-effort 关闭 Network domain（tab 池复用下避免事件常开分发）。
async fn disable_network(page: &Page) {
    let _ = page
        .execute(chromiumoxide::cdp::browser_protocol::network::DisableParams::default())
        .await;
}

/// 记录一个 ResponseReceived 响应：`encoded_data_length` 未知时为 -1
/// （cast 溢出 u64::MAX）——负值按 0 处理（放行取体，由返回体本身大小决定）。
fn record_pending_response(
    ev: std::sync::Arc<chromiumoxide::cdp::browser_protocol::network::EventResponseReceived>,
    pending: &mut BodyPendingResponses,
) {
    let len = (ev.response.encoded_data_length.max(0.0)) as u64;
    if len <= WAIT_BODY_MAX_BYTES {
        pending.insert(ev.request_id.clone(), len);
    }
}

/// LoadingFinished 后取体并匹配 needle；命中返回 `true`。
///
/// 取体失败（已 evicted/空体等）按不匹配处理，等待下一个响应。
async fn finished_body_matches(
    page: &Page,
    fin: std::sync::Arc<chromiumoxide::cdp::browser_protocol::network::EventLoadingFinished>,
    pending: &mut BodyPendingResponses,
    needle: &str,
) -> bool {
    use chromiumoxide::cdp::browser_protocol::network::GetResponseBodyParams;

    let Some(len) = pending.remove(&fin.request_id) else {
        return false;
    };
    let _ = len;
    if let Ok(returns) = page
        .execute(GetResponseBodyParams::new(fin.request_id.clone()))
        .await
    {
        let text = if returns.base64_encoded {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(returns.body.as_bytes())
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
        } else {
            Some(returns.body.clone())
        };
        if let Some(text) = text {
            if text.contains(needle) {
                return true;
            }
        }
    }
    // 取体失败（已 evicted/空体等）：跳过该响应，等下一个
    false
}

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
            Self::ResponseUrl(pattern) => Self::wait_response_url(page, pattern, timeout).await,
            Self::ResponseBody(needle) => Self::wait_response_body(page, needle, timeout).await,
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
        // 事件流订阅是异步的，任何失败都 fail-open 到固定 sleep
        let (mut started, mut finished, mut failed) = match subscribe_network_events(page).await {
            Ok(streams) => streams,
            Err(e) => {
                // 复查 L2：enable 可能已成功而 listener 订阅失败——先关闭
                // Network domain，避免页面存活期事件常开分发
                disable_network(page).await;
                log::warn!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "wait-network-idle-subscribe-failed",
                        &[("error", e.to_string())],
                    )
                );
                tokio::time::sleep(quiet_window.min(timeout)).await;
                return Ok(());
            }
        };

        let mut tracker = NetworkIdleTracker::default();
        let deadline = Instant::now() + timeout;

        // 统一出口：等待结束时 best-effort Network.disable——tab 池复用下
        // Network domain 常开会让浏览器为页面剩余生命周期持续分发网络事件
        let result = Self::wait_idle_events(
            &mut started,
            &mut finished,
            &mut failed,
            &mut tracker,
            quiet_window,
            deadline,
        )
        .await;

        disable_network(page).await;
        result
    }

    /// 网络事件循环（自 [`Self::wait_network_idle`] 提取）：维护 in-flight
    /// 计数直到 `quiet_window` 空闲或到达 deadline。
    async fn wait_idle_events(
        started: &mut chromiumoxide::listeners::EventStream<
            chromiumoxide::cdp::browser_protocol::network::EventRequestWillBeSent,
        >,
        finished: &mut chromiumoxide::listeners::EventStream<
            chromiumoxide::cdp::browser_protocol::network::EventLoadingFinished,
        >,
        failed: &mut chromiumoxide::listeners::EventStream<
            chromiumoxide::cdp::browser_protocol::network::EventLoadingFailed,
        >,
        tracker: &mut NetworkIdleTracker,
        quiet_window: Duration,
        deadline: Instant,
    ) -> Result<(), EngineError> {
        use futures::StreamExt;

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
        result
    }

    /// ResponseUrl：等待某响应 URL 匹配 regex（吸收 moli --wait-response-url 语义）
    ///
    /// 监听 `EventResponseReceived` 事件流，URL regex 命中即就绪；超时返回错误
    /// （显式信号未满足，与 Selector 同语义——区别于 NetworkIdle 的尽力而为）。
    /// 订阅失败 fail-closed 返回错误（响应等待是显式请求，静默跳过等于谎报就绪）。
    async fn wait_response_url(
        page: &Page,
        pattern: &str,
        timeout: Duration,
    ) -> Result<(), EngineError> {
        use chromiumoxide::cdp::browser_protocol::network::{
            DisableParams as NetworkDisableParams, EnableParams as NetworkEnableParams,
            EventResponseReceived,
        };
        use futures::StreamExt;

        let regex = regex::Regex::new(pattern).map_err(|e| {
            EngineError::BrowserError(format!("wait_response_url regex invalid: {e}"))
        })?;
        page.execute(NetworkEnableParams::default())
            .await
            .map_err(|e| EngineError::BrowserError(format!("network enable failed: {e}")))?;
        let result: Result<(), EngineError> = async {
            let mut events = page
                .event_listener::<EventResponseReceived>()
                .await
                .map_err(|e| EngineError::BrowserError(format!("subscribe failed: {e}")))?;
            let deadline = Instant::now() + timeout;
            loop {
                if Instant::now() >= deadline {
                    return Err(EngineError::BrowserError(format!(
                        "wait timeout: response_url pattern '{pattern}' not matched after {}ms",
                        timeout.as_millis()
                    )));
                }
                match tokio::time::timeout(NETWORK_EVENT_POLL_INTERVAL, events.next()).await {
                    Ok(Some(ev)) => {
                        if regex.is_match(&ev.response.url) {
                            return Ok(());
                        }
                    }
                    Ok(None) => {
                        return Err(EngineError::BrowserError(
                            "response event stream ended prematurely".to_string(),
                        ));
                    }
                    Err(_) => {}
                }
            }
        }
        .await;
        let _ = page.execute(NetworkDisableParams::default()).await;
        result
    }

    /// ResponseBody：等待某响应体包含指定文本（吸收 moli --wait-response-body 语义）
    ///
    /// 逐响应取体做子串匹配；单响应体匹配检查上限 [`WAIT_BODY_MAX_BYTES`]
    ///（对齐默认 max_response_body_bytes），超出跳过不做全量缓冲；
    /// base64 编码的响应体解码失败按不匹配处理（二进制体无子串语义）。
    async fn wait_response_body(
        page: &Page,
        needle: &str,
        timeout: Duration,
    ) -> Result<(), EngineError> {
        use chromiumoxide::cdp::browser_protocol::network::{
            DisableParams as NetworkDisableParams, EnableParams as NetworkEnableParams,
            EventResponseReceived,
        };
        use futures::StreamExt;

        page.execute(NetworkEnableParams::default())
            .await
            .map_err(|e| EngineError::BrowserError(format!("network enable failed: {e}")))?;
        let result: Result<(), EngineError> = async {
            use chromiumoxide::cdp::browser_protocol::network::EventLoadingFinished;

            let mut events = page
                .event_listener::<EventResponseReceived>()
                .await
                .map_err(|e| EngineError::BrowserError(format!("subscribe failed: {e}")))?;
            let mut finished = page
                .event_listener::<EventLoadingFinished>()
                .await
                .map_err(|e| EngineError::BrowserError(format!("subscribe failed: {e}")))?;
            // 响应体只在 LoadingFinished 后进入 DevTools 缓冲（ResponseReceived 时
            // 体尚未到位，立即取体报 No data found）——以 finished 事件驱动取体
            let mut pending: BodyPendingResponses = BodyPendingResponses::new();
            let deadline = Instant::now() + timeout;
            loop {
                if Instant::now() >= deadline {
                    return Err(EngineError::BrowserError(format!(
                        "wait timeout: response_body '{}' not found after {}ms",
                        needle,
                        timeout.as_millis()
                    )));
                }
                tokio::select! {
                    ev = events.next() => {
                        match ev {
                            Some(ev) => record_pending_response(ev, &mut pending),
                            None => {
                                return Err(EngineError::BrowserError(
                                    "response event stream ended prematurely".to_string(),
                                ));
                            }
                        }
                    }
                    fin = finished.next() => {
                        match fin {
                            Some(fin) => {
                                if finished_body_matches(page, fin, &mut pending, needle).await {
                                    return Ok(());
                                }
                            }
                            None => {
                                return Err(EngineError::BrowserError(
                                    "loading-finished stream ended prematurely".to_string(),
                                ));
                            }
                        }
                    }
                    _ = tokio::time::sleep(NETWORK_EVENT_POLL_INTERVAL) => {}
                }
            }
        }
        .await;
        let _ = page.execute(NetworkDisableParams::default()).await;
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
        validate_dom_stable_duration(stable_duration)?;
        let check_expr = "() => document.body ? document.body.innerHTML.length : -1";
        let deadline = Instant::now() + timeout;
        let mut last_len: i64 = -1;
        let mut stable_since: Option<Instant> = None;
        loop {
            if Instant::now() >= deadline {
                log_dom_stable_timeout(timeout);
                return Ok(());
            }
            let len = read_body_inner_html_len(page, check_expr).await?;
            if dom_length_stabilized(len, &mut last_len, &mut stable_since, stable_duration) {
                return Ok(());
            }
            tokio::time::sleep(DOM_STABLE_POLL_INTERVAL).await;
        }
    }
}

/// stable_duration 上限校验（自 [`WaitFor::wait_dom_stable`] 提取）
fn validate_dom_stable_duration(stable_duration: Duration) -> Result<(), EngineError> {
    if stable_duration > DOM_STABLE_MAX_DURATION {
        return Err(EngineError::BrowserError(format!(
            "dom_stable duration {}ms exceeds max {}ms",
            stable_duration.as_millis(),
            DOM_STABLE_MAX_DURATION.as_millis()
        )));
    }
    Ok(())
}

/// 超时降级日志（自 [`WaitFor::wait_dom_stable`] 提取）
///
/// 复查 H3：对齐 NetworkIdle 的尽力语义——超时降级为 Ok 而非
/// 整个 scrape 失败（stable_duration 超过等待预算时循环数学上
/// 不可能满足，此前会把 API 合法输入变成确定性失败）
fn log_dom_stable_timeout(timeout: Duration) {
    log::warn!(
        "{}",
        crate::i18n::tr_log_args(
            "wait-dom-unstable-degraded",
            &[("timeout", (timeout.as_millis() as i64).to_string(),)],
        )
    );
}

/// 读取 body.innerHTML 长度（自 [`WaitFor::wait_dom_stable`] 提取）
async fn read_body_inner_html_len(page: &Page, check_expr: &str) -> Result<i64, EngineError> {
    page.evaluate(check_expr)
        .await
        .map_err(|e| EngineError::BrowserError(format!("dom eval failed: {e}")))?
        .into_value::<i64>()
        .map_err(|e| EngineError::BrowserError(format!("dom eval decode failed: {e}")))
}

/// 稳定性状态推进（自 [`WaitFor::wait_dom_stable`] 提取）：
/// 返回 true 表示长度已连续 stable_duration 不变。
fn dom_length_stabilized(
    len: i64,
    last_len: &mut i64,
    stable_since: &mut Option<Instant>,
    stable_duration: Duration,
) -> bool {
    if len == *last_len {
        if stable_since.is_none() {
            *stable_since = Some(Instant::now());
        }
        if let Some(since) = *stable_since {
            if since.elapsed() >= stable_duration {
                return true;
            }
        }
    } else {
        *last_len = len;
        *stable_since = None;
    }
    false
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

    // escape_js_string

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

    // WaitFor 枚举

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

    // wait_network_idle（无需真实 Page，内部仅 sleep）

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

    // NetworkIdleTracker（P07 状态机单测）

    fn t(ms: u64) -> Instant {
        // 用相对构造的 Instant 序列验证状态机（Instant 只能取 now，
        // 这里以单调递增的 now 偏移模拟时间推进）
        let mut base = Instant::now();
        base += Duration::from_millis(ms);
        base
    }

    // ---- ResponseUrl / ResponseBody 活体钉测（本机 Chrome，先例 xhr_capture_e2e）----
    // 运行：cargo test --features engine-playwright --lib response_wait -- --ignored --test-threads=1

    /// 起本地 HTTP 服务：/page 300ms 后 fetch /api/payload（body 含 READY_TOKEN）
    async fn spawn_wait_test_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 1024];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let (status, body) = if req.starts_with("GET /page") {
                        (
                            "200 OK",
                            r#"<html><body>wait probe<script>setTimeout(()=>fetch('/api/payload'),300)</script></body></html>"#.to_string(),
                        )
                    } else if req.starts_with("GET /api/payload") {
                        ("200 OK", r#"{"status":"READY_TOKEN"}"#.to_string())
                    } else {
                        ("404 Not Found", String::new())
                    };
                    // keep-alive（不写 Connection: close）：close 连接下 Chrome 不为
                    // 响应保留 DevTools body 缓冲，getResponseBody 报 No data found
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                });
            }
        });
        (addr, handle)
    }

    fn find_playwright_chromium() -> Option<std::path::PathBuf> {
        let base = std::path::PathBuf::from(std::env::var("HOME").ok()?)
            .join("Library/Caches/ms-playwright");
        let mut entries: Vec<_> = std::fs::read_dir(&base)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("chromium-"))
            })
            .collect();
        entries.sort();
        let chrome = entries
            .last()?
            .join("chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
        chrome.is_file().then_some(chrome)
    }

    async fn launch_page() -> (chromiumoxide::Browser, chromiumoxide::page::Page) {
        let profile_dir = std::env::temp_dir().join(format!(
            "crawlrs-wait-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&profile_dir).expect("profile dir");
        let mut builder = chromiumoxide::BrowserConfig::builder()
            .no_sandbox()
            .arg("--disable-gpu")
            .user_data_dir(&profile_dir);
        // 本机无系统 Chrome 时回退 playwright 缓存的 Chromium（CRAWLRS_TEST_CHROME 可覆盖）
        if let Ok(bin) = std::env::var("CRAWLRS_TEST_CHROME") {
            builder = builder.chrome_executable(bin);
        } else if let Some(bin) = find_playwright_chromium() {
            builder = builder.chrome_executable(bin);
        }
        let config = builder.build().expect("config");
        let (browser, mut handler) = chromiumoxide::Browser::launch(config)
            .await
            .expect("launch");
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(h) = handler.next().await {
                if h.is_err() {
                    break;
                }
            }
        });
        let page = browser.new_page("about:blank").await.expect("page");
        (browser, page)
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features engine-playwright --lib response_wait -- --ignored --test-threads=1"]
    async fn response_wait_url_hit_returns_early() {
        let (addr, server) = spawn_wait_test_server().await;
        let (_browser, page) = launch_page().await;
        page.goto(format!("http://{addr}/page")).await.unwrap();
        let started = Instant::now();
        let result = WaitFor::ResponseUrl("api/payload".to_string())
            .wait(&page, Duration::from_secs(10))
            .await;
        server.abort();
        assert!(result.is_ok(), "命中必须提前返回: {:?}", result.err());
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "提前返回不得等到超时边界"
        );
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features engine-playwright --lib response_wait -- --ignored --test-threads=1"]
    async fn response_wait_body_substring_hit() {
        let (addr, server) = spawn_wait_test_server().await;
        let (_browser, page) = launch_page().await;
        page.goto(format!("http://{addr}/page")).await.unwrap();
        let result = WaitFor::ResponseBody("READY_TOKEN".to_string())
            .wait(&page, Duration::from_secs(10))
            .await;
        server.abort();
        assert!(
            result.is_ok(),
            "body 子串命中必须返回 Ok: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features engine-playwright --lib response_wait -- --ignored --test-threads=1"]
    async fn response_wait_url_timeout_errors() {
        let (addr, server) = spawn_wait_test_server().await;
        let (_browser, page) = launch_page().await;
        page.goto(format!("http://{addr}/page")).await.unwrap();
        let result = WaitFor::ResponseUrl("this-never-matches".to_string())
            .wait(&page, Duration::from_secs(1))
            .await;
        server.abort();
        match result {
            Err(EngineError::BrowserError(msg)) => {
                assert!(
                    msg.contains("wait timeout"),
                    "超时文案应指明 wait timeout: {msg}"
                );
            }
            other => panic!("期望超时错误，实际: {:?}", other),
        }
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
