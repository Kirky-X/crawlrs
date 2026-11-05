// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! MoliEngine — 经 CDP 控制外部 `moli serve` 的 JS 渲染引擎（端点模式）
//!
//! PoC 结论（specmark/changes/moli-engine-adoption/poc-report.md，已随归档提交于 scripts/poc/）：
//! - 四类指令可用；**截图要求端点以 `moli serve --layout` 启动**（否则显式报错 -32000）
//! - **禁发 `Browser.close`**：moli serve 收到即整个退出——收尾只 `page.close()`（关 target）
//! - 支持 needs_js / wait_for / needs_screenshot；不支持交互 actions（v1 未实现，score=0 诚实出局）
//!
//! 内存水位调度：本引擎的 `support_score` 被 router 侧 `apply_memory_policy` 按水位修正
//! （Tight +30、Critical 仅 moli 保留），引擎自身不感知水位。

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::Browser;
use futures::StreamExt;
use std::time::Duration;

use crate::engines::engine_client::{
    EngineError, HttpMethod, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine, WaitFor,
};

/// moli 端点发现路径（CDP 标准 HTTP 面）
const CDP_VERSION_PATH: &str = "/json/version";
/// NetworkIdle 的就绪后附加静默窗口：readyState=complete 后等待网络静置的近似
const NETWORK_IDLE_SETTLE_MS: u64 = 500;
/// Selector/DomStable 轮询周期
const WAIT_POLL_INTERVAL_MS: u64 = 100;

/// MoliEngine 配置（从 `[engines.moli]` 装配注入）
#[derive(Debug, Clone)]
pub struct MoliEngineConfig {
    /// moli serve CDP 端点（如 `http://127.0.0.1:9222`）
    pub endpoint: String,
    /// CDP 建连超时（含 /json/version 发现）
    pub connect_timeout: Duration,
    /// 单次抓取超时（导航+等待+取 DOM+截图），即引擎 MRT
    pub scrape_timeout: Duration,
}

impl MoliEngineConfig {
    /// 从 settings 构造（配置校验已在配置层完成：enabled 时 endpoint 必可解析）
    pub fn from_settings(settings: &crate::config::engines::MoliEngineSettings) -> Self {
        Self {
            endpoint: settings.endpoint.clone(),
            connect_timeout: Duration::from_millis(settings.connect_timeout_ms),
            scrape_timeout: Duration::from_millis(settings.scrape_timeout_ms),
        }
    }
}

/// moli CDP 引擎
pub struct MoliEngine {
    config: MoliEngineConfig,
    /// 端点发现用的 HTTP 客户端（GET /json/version 取 webSocketDebuggerUrl）
    http_client: reqwest::Client,
}

impl MoliEngine {
    pub fn new(config: MoliEngineConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    /// GET {endpoint}/json/version 解析 webSocketDebuggerUrl
    async fn discover_ws_url(&self) -> Result<String, EngineError> {
        let url = format!(
            "{}{}",
            self.config.endpoint.trim_end_matches('/'),
            CDP_VERSION_PATH
        );
        let resp = tokio::time::timeout(
            self.config.connect_timeout,
            self.http_client.get(&url).send(),
        )
        .await
        .map_err(|_| {
            let e = format!(
                "moli endpoint discovery timed out after {:?}",
                self.config.connect_timeout
            );
            log::error!(target: "crawlrs::engine::moli", "{}", e);
            EngineError::Other(e)
        })?
        .map_err(|e| {
            log::error!(target: "crawlrs::engine::moli", "moli endpoint unreachable ({}): {}", url, e);
            EngineError::Other("moli endpoint unreachable".to_string())
        })?;
        let text = resp.text().await.map_err(|e| {
            let e = format!("moli endpoint discovery body read failed: {}", e);
            log::error!(target: "crawlrs::engine::moli", "{}", e);
            EngineError::Other(e)
        })?;
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("webSocketDebuggerUrl")
                    .and_then(|s| s.as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                log::error!(target: "crawlrs::engine::moli", "moli endpoint {} returned no webSocketDebuggerUrl", url);
                EngineError::Other("moli endpoint returned no webSocketDebuggerUrl".to_string())
            })
    }

    /// 独立会话：全路径关闭页面 target（成功/错误 alike），分段超时保证有界收敛。
    /// owned 输入使其可 spawn（'static），调用方取消/超时不影响收尾。
    async fn scrape_session(
        config: MoliEngineConfig,
        http_client: reqwest::Client,
        request: InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let engine = MoliEngine {
            config: config.clone(),
            http_client,
        };
        let ws_url = engine.discover_ws_url().await?;
        let (browser, mut handler) =
            tokio::time::timeout(config.connect_timeout, Browser::connect(ws_url))
                .await
                .map_err(|_| {
                    log::error!(target: "crawlrs::engine::moli", "moli CDP connect timed out");
                    EngineError::Other("moli CDP connect timed out".to_string())
                })?
                .map_err(|e| {
                    // 日志保留完整错误（运维侧）；上抛消息剥离端点细节（内网拓扑不外泄）
                    log::error!(target: "crawlrs::engine::moli", "moli CDP connect failed: {}", e);
                    EngineError::Other("moli CDP connect failed".to_string())
                })?;
        let handler_task = tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if h.is_err() {
                    break;
                }
            }
        });

        let res = async {
            let page = browser.new_page("about:blank").await.map_err(|e| {
                log::error!(target: "crawlrs::engine::moli", "moli new_page failed: {}", e);
                EngineError::Other("moli new_page failed".to_string())
            })?;
            // 嵌套 outcome 块：任何 `?` 早退都会先落回本层，随后统一 page.close
            let outcome = async {
                // 导航分段超时（超时也走统一收尾，不泄漏 target）
                tokio::time::timeout(config.scrape_timeout, page.goto(request.url.as_str()))
                    .await
                    .map_err(|_| {
                        log::error!(
                            target: "crawlrs::engine::moli",
                            "moli navigation to {} timed out", request.url
                        );
                        EngineError::Timeout(config.scrape_timeout)
                    })?
                    .map_err(|e| {
                        log::error!(
                            target: "crawlrs::engine::moli",
                            "moli navigation to {} failed: {}", request.url, e
                        );
                        EngineError::BrowserError("moli navigation failed".to_string())
                    })?;
                engine.wait_for_ready(&page, &request.wait_for).await?;
                let html = page
                    .evaluate("() => document.documentElement.outerHTML")
                    .await
                    .map_err(|e| {
                        log::error!(target: "crawlrs::engine::moli", "moli outerHTML read failed: {}", e);
                        EngineError::Other("moli outerHTML read failed".to_string())
                    })?
                    .into_value::<String>()
                    .map_err(|e| {
                        log::error!(target: "crawlrs::engine::moli", "moli outerHTML decode failed: {}", e);
                        EngineError::Other("moli outerHTML decode failed".to_string())
                    })?;
                validate_html(&html)?;

                let screenshot = if request.needs_screenshot {
                    let params = chromiumoxide::page::ScreenshotParams::builder()
                        .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
                        .build();
                    let bytes = page.screenshot(params).await.map_err(|e| {
                        log::error!(
                            target: "crawlrs::engine::moli",
                            "moli screenshot failed (endpoint must run `moli serve --layout`): {}", e
                        );
                        EngineError::BrowserError("moli screenshot failed".to_string())
                    })?;
                    Some(BASE64.encode(bytes))
                } else {
                    None
                };

                Ok::<InternalScrapeResponse, EngineError>(InternalScrapeResponse {
                    status_code: 200,
                    content: html,
                    screenshot,
                    content_type: "text/html".to_string(),
                    headers: std::collections::HashMap::new(),
                    response_time_ms: 0,
                    final_url: None,
                    captured_responses: Vec::new(),
                    js_degraded: false,
                })
            }
            .await;
            // 全路径收尾（成功/错误 alike）：先关页面 target 再断连接。
            // 禁发 Browser.close（会终止 moli serve 进程）——只 page.close。
            let _ = page.close().await;
            outcome
        }
        .await;
        drop(browser);
        handler_task.abort();
        res
    }

    /// 按请求 `wait_for` 等待页面就绪（v1 近似语义，注释逐条说明）：
    /// - `None` / `NetworkIdle`：等 `document.readyState == "complete"` + 固定静默窗
    ///   （CDP Network 事件计数的精确 idle 未在 PoC 验证面内，近似实现诚实标注）
    /// - `Selector(s)`：轮询 `document.querySelector(s)` 非空
    /// - `DomStable(d)`：轮询 outerHTML 长度在 d 内不再变化
    /// - `ResponseUrl` / `ResponseBody`：变体于 wait-signals 变更（Phase 3）落地时在此消费
    async fn wait_for_ready(
        &self,
        page: &chromiumoxide::Page,
        wait_for: &Option<WaitFor>,
    ) -> Result<(), EngineError> {
        match wait_for {
            None | Some(WaitFor::NetworkIdle) => {
                let deadline = tokio::time::Instant::now() + self.config.scrape_timeout;
                loop {
                    let ready = page
                        .evaluate("() => document.readyState")
                        .await
                        .ok()
                        .and_then(|v| v.into_value::<String>().ok())
                        .is_some_and(|s| s == "complete");
                    if ready {
                        tokio::time::sleep(Duration::from_millis(NETWORK_IDLE_SETTLE_MS)).await;
                        return Ok(());
                    }
                    if tokio::time::Instant::now() >= deadline {
                        let e = "moli page never reached readyState=complete".to_string();
                        log::error!(target: "crawlrs::engine::moli", "{}", e);
                        return Err(EngineError::Timeout(self.config.scrape_timeout));
                    }
                    tokio::time::sleep(Duration::from_millis(WAIT_POLL_INTERVAL_MS)).await;
                }
            }
            Some(WaitFor::Selector(selector)) => {
                let expr = format!(r#"() => document.querySelector({:?}) !== null"#, selector);
                self.poll_until(page, &expr, "selector never appeared")
                    .await
            }
            Some(WaitFor::DomStable(stable_for)) => {
                let mut last_len: Option<usize> = None;
                let mut stable_since: Option<tokio::time::Instant> = None;
                let deadline = tokio::time::Instant::now() + self.config.scrape_timeout;
                loop {
                    let len = page
                        .evaluate("() => document.documentElement.outerHTML.length")
                        .await
                        .ok()
                        .and_then(|v| v.into_value::<f64>().ok())
                        .map(|f| f as usize);
                    if let (Some(prev), Some(cur)) = (last_len, len) {
                        if prev == cur {
                            let since = stable_since.get_or_insert(tokio::time::Instant::now());
                            if since.elapsed() >= *stable_for {
                                return Ok(());
                            }
                        } else {
                            stable_since = None;
                        }
                    }
                    last_len = len;
                    if tokio::time::Instant::now() >= deadline {
                        let e = "moli page DOM never stabilized".to_string();
                        log::error!(target: "crawlrs::engine::moli", "{}", e);
                        return Err(EngineError::Timeout(self.config.scrape_timeout));
                    }
                    tokio::time::sleep(Duration::from_millis(WAIT_POLL_INTERVAL_MS)).await;
                }
            }
            Some(WaitFor::ResponseUrl(_)) | Some(WaitFor::ResponseBody(_)) => {
                // 正常不可达：scrape() 已在 I/O 前早退 Unsupported；此处防御性兜底
                Err(EngineError::Unsupported(
                    "response-level wait unsupported by moli".to_string(),
                ))
            }
        }
    }

    /// 通用表达式轮询（Selector 等待用）
    async fn poll_until(
        &self,
        page: &chromiumoxide::Page,
        expr: &str,
        timeout_msg: &str,
    ) -> Result<(), EngineError> {
        let deadline = tokio::time::Instant::now() + self.config.scrape_timeout;
        loop {
            let hit = page
                .evaluate(expr)
                .await
                .ok()
                .and_then(|v| v.into_value::<bool>().ok())
                .unwrap_or(false);
            if hit {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                log::error!(target: "crawlrs::engine::moli", "{}", timeout_msg);
                return Err(EngineError::Timeout(self.config.scrape_timeout));
            }
            tokio::time::sleep(Duration::from_millis(WAIT_POLL_INTERVAL_MS)).await;
        }
    }
}

/// 空 HTML 校验（独立纯逻辑便于单测）：trim 后为空视为引擎侧异常，不返回空成功
fn validate_html(html: &str) -> Result<(), EngineError> {
    if html.trim().is_empty() {
        let e = "moli returned empty HTML".to_string();
        log::error!(target: "crawlrs::engine::moli", "{}", e);
        Err(EngineError::Other(e))
    } else {
        Ok(())
    }
}

#[async_trait::async_trait]
impl ScraperEngine for MoliEngine {
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        if request.url.trim().is_empty() {
            return Err(EngineError::InvalidUrl("empty url".to_string()));
        }
        // 响应级等待变体：moli 的 CDP Network 事件面实测不可达（Network.enable
        // 被接受但 0 事件，scripts/poc/moli/network-probe-output.txt）——按
        // R-wait-signals-002 返回 Unsupported，不静默吞变体。任何 I/O 前早退。
        if matches!(
            request.wait_for,
            Some(WaitFor::ResponseUrl(_)) | Some(WaitFor::ResponseBody(_))
        ) {
            log::error!(
                target: "crawlrs::engine::moli",
                "response-level wait variants unsupported by moli CDP (Network events unavailable)"
            );
            return Err(EngineError::Unsupported(
                "response-level wait (ResponseUrl/ResponseBody) unsupported by moli".to_string(),
            ));
        }
        let started = std::time::Instant::now();
        // 会话任务独立 spawn（owned 输入）：外层超时/调用方丢弃 future 时，任务仍按
        // 内部分段超时走完收尾（全路径 page.close），杜绝错误/取消路径泄漏 page
        // target——chromiumoxide 的 Page 无 Drop，泄漏的 tab 会在 moli serve 进程
        // 存活至重启（三维审查 P2-1）。
        let session = tokio::spawn(MoliEngine::scrape_session(
            self.config.clone(),
            self.http_client.clone(),
            request.clone(),
        ));
        match tokio::time::timeout(self.config.scrape_timeout, session).await {
            Ok(Ok(Ok(mut resp))) => {
                resp.response_time_ms = started.elapsed().as_millis() as u64;
                Ok(resp)
            }
            Ok(Ok(Err(e))) => Err(e),
            // 会话任务 panic：向上如实报告而非吞成成功
            Ok(Err(join_err)) => Err(EngineError::Other(format!(
                "moli session task failed: {}",
                join_err
            ))),
            Err(_) => {
                // 超时仅解除调用方等待；spawn 的会话任务自行收尾（分段超时 bounded）
                log::error!(
                    target: "crawlrs::engine::moli",
                    "moli scrape timed out after {:?} (session will self-clean)", self.config.scrape_timeout
                );
                Err(EngineError::Timeout(self.config.scrape_timeout))
            }
        }
    }

    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.method != HttpMethod::Get {
            return 0;
        }
        // 交互动作 v1 未实现（诚实出局，feature filter 会按 actions 丢弃）
        if !request.actions.is_empty() {
            return 0;
        }
        if request.needs_screenshot {
            // PoC 校准：可用（--layout 前置）但低于 Playwright 基线
            return 80;
        }
        if request.needs_js {
            // PoC 校准：高分但低于 Playwright（100）10 分——Abundant 水位下作 Playwright
            // 的第一回退位；Tight 水位 +30 后自然反超
            return 90;
        }
        // 静态内容让位静态引擎
        0
    }

    fn name(&self) -> &'static str {
        "moli"
    }

    fn supports_tls_fingerprint(&self) -> bool {
        false
    }

    fn max_response_time(&self) -> Duration {
        self.config.scrape_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> MoliEngine {
        MoliEngine::new(MoliEngineConfig {
            endpoint: "http://127.0.0.1:9222".to_string(),
            connect_timeout: Duration::from_millis(50),
            scrape_timeout: Duration::from_millis(120),
        })
    }

    fn request(needs_js: bool, needs_screenshot: bool) -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            headers: Default::default(),
            timeout: Duration::from_secs(30),
            needs_js,
            needs_screenshot,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
            needs_mllm: false,
        }
    }

    #[test]
    fn trait_semantics_name_and_mrt() {
        let e = engine();
        assert_eq!(e.name(), "moli");
        assert_eq!(e.max_response_time(), Duration::from_millis(120));
        assert!(!e.supports_tls_fingerprint());
    }

    #[test]
    fn support_score_static_and_js_branches() {
        let e = engine();
        assert_eq!(
            e.support_score(&request(false, false)),
            0,
            "静态请求 score=0"
        );
        assert_eq!(
            e.support_score(&request(true, false)),
            90,
            "needs_js 高分但低于 Playwright"
        );
        assert_eq!(
            e.support_score(&request(true, true)),
            80,
            "截图场景低于 Playwright 基线"
        );
        assert_eq!(
            e.support_score(&request(false, true)),
            80,
            "仅截图（无 needs_js）也给截图分"
        );
        let mut r = request(true, false);
        r.method = HttpMethod::Post;
        assert_eq!(e.support_score(&r), 0, "非 GET 不支持");
        let mut r = request(true, false);
        r.actions
            .push(crate::engines::types::InternalPageAction::Click {
                selector: "#x".to_string(),
            });
        assert_eq!(
            e.support_score(&r),
            0,
            "交互动作 v1 未实现，score=0 诚实出局"
        );
    }

    #[test]
    fn validate_html_rejects_empty() {
        assert!(validate_html("").is_err());
        assert!(validate_html("   \n\t ").is_err());
        assert!(validate_html("<html></html>").is_ok());
    }

    #[tokio::test]
    async fn scrape_empty_url_rejected_before_any_io() {
        let e = engine();
        let mut r = request(true, false);
        r.url = "  ".to_string();
        match e.scrape(&r).await {
            Err(EngineError::InvalidUrl(_)) => {}
            other => panic!("期望 InvalidUrl，实际: {:?}", other.err()),
        }
    }

    /// 响应级等待变体在 moli 侧显式 Unsupported（R-wait-signals-002/R-engines-moli-005）：
    /// 早退发生在任何 CDP I/O 之前——用不可达端点验证返回的是 Unsupported 而非连接错误
    #[tokio::test]
    async fn response_wait_variants_unsupported_before_io() {
        let e = MoliEngine::new(MoliEngineConfig {
            endpoint: "http://127.0.0.1:1".to_string(),
            connect_timeout: Duration::from_millis(200),
            scrape_timeout: Duration::from_millis(400),
        });
        for wait in [
            WaitFor::ResponseUrl("api".to_string()),
            WaitFor::ResponseBody("token".to_string()),
        ] {
            let mut r = request(true, false);
            r.wait_for = Some(wait);
            match e.scrape(&r).await {
                Err(EngineError::Unsupported(msg)) => {
                    assert!(
                        msg.contains("response-level"),
                        "文案应指明响应级等待: {msg}"
                    );
                }
                other => panic!("期望 Unsupported，实际: {:?}", other.err()),
            }
        }
    }

    /// 端点拒绝连接：本地未监听端口，真实 TCP 失败（无 mock 库依赖）
    #[tokio::test]
    async fn scrape_connection_refused_errors() {
        let e = MoliEngine::new(MoliEngineConfig {
            endpoint: "http://127.0.0.1:1".to_string(),
            connect_timeout: Duration::from_millis(300),
            scrape_timeout: Duration::from_millis(500),
        });
        let result = e.scrape(&request(true, false)).await;
        assert!(result.is_err(), "端点不可达必须是错误而非空成功");
        assert!(!matches!(result, Ok(_)));
    }

    /// 端点存在但非 CDP（返回 404）：发现路径失败显式报错
    #[tokio::test]
    async fn scrape_non_cdp_endpoint_errors() {
        // 起一个立刻 404 的本地 HTTP stub（真实 TCP，不用 mock 库）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 512];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        let e = MoliEngine::new(MoliEngineConfig {
            endpoint: format!("http://{}", addr),
            connect_timeout: Duration::from_millis(500),
            scrape_timeout: Duration::from_millis(800),
        });
        let result = e.scrape(&request(true, false)).await;
        server.abort();
        assert!(result.is_err(), "非 CDP 端点必须报错而非空成功");
    }

    /// 响应超时：TCP stub 接受连接后挂起不发数据，发现阶段超时
    #[tokio::test]
    async fn scrape_discovery_timeout_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                match listener.accept().await {
                    Ok((sock, _)) => held.push(sock),
                    Err(_) => break,
                }
            }
        });
        let e = MoliEngine::new(MoliEngineConfig {
            endpoint: format!("http://{}", addr),
            connect_timeout: Duration::from_millis(200),
            scrape_timeout: Duration::from_millis(600),
        });
        let started = std::time::Instant::now();
        let result = e.scrape(&request(true, false)).await;
        server.abort();
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "超时必须在 connect_timeout 量级返回，而非挂到外层超时"
        );
    }

    /// 端到端成功路径：连真实 moli serve（PoC 环境）。CI 无 moli，标 #[ignore]
    /// 由本地验证（见 change 目录验证记录）。目标 URL 经 MOLI_POC_LIVE_URL 覆盖
    /// （默认 baidu——本机出口对部分国际站点间歇不可达，非引擎缺陷）。
    #[tokio::test]
    #[ignore = "requires local `moli serve --layout` on :9222（scripts/poc/moli/setup.sh）"]
    async fn scrape_live_moli_serves_html() {
        let e = MoliEngine::new(MoliEngineConfig {
            endpoint: "http://127.0.0.1:9222".to_string(),
            connect_timeout: Duration::from_secs(3),
            scrape_timeout: Duration::from_secs(30),
        });
        let mut req = request(true, true);
        req.url = std::env::var("MOLI_POC_LIVE_URL")
            .unwrap_or_else(|_| "https://www.baidu.com".to_string());
        let resp = e.scrape(&req).await.expect("live moli 应成功");
        assert!(!resp.js_degraded);
        assert!(resp.content.contains("<html"));
        assert!(resp.response_time_ms > 0);
        // 截图路径（--layout 前提下）：base64 解码后须为 PNG 魔数
        let shot = resp
            .screenshot
            .expect("needs_screenshot=true 不得静默省略截图");
        let bytes = BASE64.decode(shot).expect("截图须为合法 base64");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "截图须为 PNG");
    }
}
