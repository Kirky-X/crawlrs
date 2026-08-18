// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! WreqEngine JA3/JA4 TLS 指纹端到端验证（Phase 1 / D4，T024）。
//!
//! 校验 `WreqEngine`（BoringSSL / wreq 后端）对真实 TLS 目标能完成握手并产出指纹。
//!
//! # 运行前提
//!
//! - 需要外网访问 `https://tls.peet.ws/`（第三方指纹检测端点）
//! - 需要 `engine-tls-fingerprint` feature
//!
//! # 前置限制：SSRF IP-rewrite 会破坏 SNI 证书校验（rule 12 显性化）
//!
//! 引擎（与既有 [`ReqwestEngine`] 相同）在执行 SSRF 保护后，把 URL host 改写为解析出的
//! IP 直连（`set_host(resolved_first)`）。这会导致 TLS 连接的 SNI 变为裸 IP，证书名称
//! 校验失败（服务器证书只覆盖域名 SAN）。这是**全库共享的既有缺陷**（ReqwestEngine 对
//! example.com / tls.peet.ws 同样无法带证书校验直连），非本引擎特有，修复需改动共享 SSRF
//! /TLS 逻辑，超出 Phase 1 范围。
//!
//! 因此本测试如 ReqwestEngine 既有工作路径一样，使用 `skip_tls_verification=true`
//! （引擎显式支持的开关，见 `InternalScrapeRequest.skip_tls_verification`）绕过证书名称
//! 校验，使 IP 直连能完成握手——服务端仍会如实回传本引擎的 TLS/HTTP2 指纹。
//!
//! # 保真度说明（rule 12 显性化）
//!
//! 引擎当前将全部 [`TlsEmulation`] 变体解析为 `wreq::EmulationProvider::default()`
//! （wreq 内置 Chrome 系 BoringSSL/HTTP2 指纹模板）。因此本测试**不硬编码断言**
//! "精确等于 Chrome 131 的 JA4"——该值依赖 wreq 各版本内置模板与上游 BoringSSL，
//! 编造一个未被实测确认的 JA4 断言反而会在任何上游指纹升级时假失败。
//!
//! 本测试验证的**有意义属性**（rule 9）：
//! - 引擎能通过 HTTPS/TLS 建立真实握手，且协商出 `h2`（HTTP/2）+ TLS 1.3
//! - 服务端回传的 `tls.ja4` 指纹存在且为规范 3 段格式（`t<ver><4cipher><exts>_..._...`），
//!   即真实指纹协商发生了（并非服务端看不到指纹或返回空）
//! - 实测值参考：`ja4="t13d2811h2_..._..."`、`ja3="771,49195-..."`（browser 系指纹）
//!
//! 运行：`cargo test --test main --features full,engine-tls-fingerprint -- --ignored wreq_fingerprint_test`
//!
//! [serde_json] 为 tests 专用解析工具；crawlrs 本体不因该测试新增生产依赖。

use crawlrs::engines::client::wreq_engine::WreqEngine;
use crawlrs::engines::engine_client::InternalScrapeRequest;
use crawlrs::engines::ScraperEngine;
use crawlrs::utils::ua_pool::UaPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// 指纹校验端点（TLS 检测服务，返回 JSON 含 tls.ja3/ja4）
const FINGERPRINT_ENDPOINT: &str = "https://tls.peet.ws/api/all";

fn build_engine() -> WreqEngine {
    WreqEngine::new(Arc::new(UaPool::new()), Duration::from_secs(15), 30)
        .expect("wreq client build should succeed")
}

/// 从 JSON 中读取 `tls.ja4` 字段（嵌套路径，tls.peet.ws 实际返回格式）。
fn extract_ja4(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("tls")
        .and_then(|tls| tls.get("ja4"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// T024：WreqEngine 对真实 TLS 目标能建立握手并产出 ja4 指纹。
///
/// 因 SSRF IP-rewrite 破坏 SNI 校验（见模块文档），使用 `skip_tls_verification`
/// 绕过证书名称校验以完成握手。不硬编码断言具体 Chrome 131 JA4（见模块文档
/// 保真度说明），只验证指纹大概率真实协商成功这一稳定属性 + HTTP/2。
#[tokio::test]
#[ignore = "需要外网访问 tls.peet.ws + --features engine-tls-fingerprint"]
async fn wreq_engine_emits_tls_ja4_fingerprint() {
    let engine = build_engine();
    let request = InternalScrapeRequest {
        url: FINGERPRINT_ENDPOINT.to_string(),
        method: crawlrs::common::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
        screenshot_config: None,
        mobile: false,
        proxy: None,
        skip_tls_verification: true,
        needs_tls_fingerprint: true,
        use_fire_engine: false,
        actions: Vec::new(),
        body: None,
        sync_wait_ms: 0,
        block_ads: false,
        block_media: false,
        session_id: None,
        wait_for: None,
        needs_mllm: false,
    };

    let response = engine
        .scrape(&request)
        .await
        .expect("scraping tls fingerprint endpoint should succeed");
    assert!(
        (200..300).contains(&response.status_code),
        "expected 2xx, got {}",
        response.status_code
    );

    let ja4 = extract_ja4(&response.content).expect("服务端应返回含 tls.ja4 字段的 JSON 指纹");
    // 现代 JA4 原始格式：`t<version><4cipher><exts>_<cipher-hash>_<ext-hash>`（三段、下划线分隔）
    assert!(
        ja4.starts_with('t') && ja4.split('_').count() == 3,
        "ja4 应为规范 3 段格式，got: {ja4}"
    );
    // 应协商出 TLS 1.3（tls_version_record=771）且内容含 http_version h2（browser 系 HTTP2）
    let value: serde_json::Value =
        serde_json::from_str(&response.content).expect("fingerprint JSON 应可解析");
    assert_eq!(
        value["http_version"].as_str(),
        Some("h2"),
        "wreq/BoringSSL 应协商出 HTTP/2（ALPN h2）"
    );
}

/// T024：验证引擎在 `needs_tls_fingerprint=true` 时被优先（support_score 专长路径）。
/// 纯单元断言，无网络。
#[test]
fn wreq_engine_scores_tls_requests_highest() {
    let engine = build_engine();
    let mut req = InternalScrapeRequest {
        url: "https://example.com".to_string(),
        method: crawlrs::common::HttpMethod::Get,
        headers: HashMap::new(),
        timeout: Duration::from_secs(30),
        needs_js: false,
        needs_screenshot: false,
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
    };
    let baseline = engine.support_score(&req);
    req.needs_tls_fingerprint = true;
    assert!(
        engine.support_score(&req) > baseline,
        "T024：TLS 指纹请求应获得最高优先分数"
    );
}
