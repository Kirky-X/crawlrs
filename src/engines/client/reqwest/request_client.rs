// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 请求级客户端构建 — 从 [`super::ReqwestEngine`] 拆分的 partial impl block
//!
//! 包含 SSRF DNS pin 客户端、请求级 cookie Jar 与 TLS 绕过临时客户端。

use super::{build_all_proxy, ReqwestEngine};
use crate::engines::client::handle::ClientHandle;
use crate::engines::engine_client::{EngineError, InternalScrapeRequest};
use crate::utils::proxy::redact_proxy_url;
use log::error;
use std::sync::Arc;
use std::time::Duration;

impl ReqwestEngine {
    /// 构建本次请求的 SSRF 客户端与请求级 cookie Jar
    ///
    /// 自 [`ScraperEngine::scrape`] 提取，构建分支、日志与错误语义逐字保留。
    /// 返回 `(request_cookie_jar, ssrf_client)`。
    pub(super) fn build_request_client(
        &self,
        request: &InternalScrapeRequest,
        handle: &mut ClientHandle,
        host: &str,
        resolve_addrs: &[std::net::SocketAddr],
    ) -> Result<(Arc<reqwest::cookie::Jar>, reqwest::Client), EngineError> {
        let need_tls_bypass = request.skip_tls_verification;
        // SSRF DNS pin: build client with resolve override from scratch.
        // Keeps the original hostname for TLS SNI and HTTP virtual host routing,
        // while connecting to SSRF-validated IPs.
        //
        // F2：请求级 cookie Jar——reqwest 中间跳（302 等）响应不外显，其
        // Set-Cookie 只进 client cookie store；共享 handle.client 的 store
        // 无法读取，故本路径改挂可读的 `cookie::Jar`，响应后统一回取
        // （含重定向链各跳），供 session 身份 Cookie 通道累积。
        let request_cookie_jar = Arc::new(reqwest::cookie::Jar::default());
        let ssrf_client = if !host.is_empty() && !host.parse::<std::net::IpAddr>().is_ok() {
            self.build_ssrf_resolve_client(
                host,
                resolve_addrs,
                Arc::clone(&request_cookie_jar),
                need_tls_bypass,
                handle,
            )?
        } else {
            // Host is an IP literal (no DNS to override) or empty — use base client
            let temp_client = self.build_temp_tls_bypass_client(
                Arc::clone(&request_cookie_jar),
                need_tls_bypass,
                handle,
            )?;
            temp_client.unwrap_or_else(|| handle.client.clone())
        };
        Ok((request_cookie_jar, ssrf_client))
    }

    /// SSRF DNS pin 客户端构建（自 [`Self::build_request_client`] 提取）
    fn build_ssrf_resolve_client(
        &self,
        host: &str,
        resolve_addrs: &[std::net::SocketAddr],
        request_cookie_jar: Arc<reqwest::cookie::Jar>,
        need_tls_bypass: bool,
        handle: &mut ClientHandle,
    ) -> Result<reqwest::Client, EngineError> {
        let mut b = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .connect_timeout(crate::utils::http_client::connect_timeout_for(
                Duration::from_secs(self.timeout_seconds),
            ))
            .cookie_provider(Arc::clone(&request_cookie_jar))
            .local_address(Some(std::net::Ipv4Addr::UNSPECIFIED.into()))
            .dns_resolver(crate::infrastructure::dns::create_ipv4_only_resolver())
            .resolve_to_addrs(host, resolve_addrs)
            .redirect(crate::utils::http_client::create_ssrf_safe_redirect_policy(
                5,
            ));
        if need_tls_bypass {
            b = b.danger_accept_invalid_certs(true);
        }
        if let Some(proxy_url) = handle.used_proxy_url.take() {
            match build_all_proxy(&proxy_url) {
                Ok(px) => b = b.proxy(px),
                Err(e) => {
                    // 静默裸连（CWE-436）会让请求绕过代理出口且
                    // used_proxy_url 仍谎报走代理——这里显式清除并告警
                    error!(
                        "Failed to parse proxy URL for SSRF resolve client; \
                         request will NOT use the proxy: {}; proxy_url={}",
                        e,
                        redact_proxy_url(&proxy_url)
                    );
                }
            }
        }
        b.build()
            .map_err(|e| EngineError::Other(format!("Failed to build SSRF resolve client: {}", e)))
    }

    /// 请求级 TLS 绕过临时客户端构建（自 [`Self::build_request_client`] 提取）
    fn build_temp_tls_bypass_client(
        &self,
        request_cookie_jar: Arc<reqwest::cookie::Jar>,
        need_tls_bypass: bool,
        handle: &mut ClientHandle,
    ) -> Result<Option<reqwest::Client>, EngineError> {
        let temp_client: Option<reqwest::Client> = if need_tls_bypass {
            let mut b = reqwest::Client::builder()
                .timeout(Duration::from_secs(self.timeout_seconds))
                .connect_timeout(crate::utils::http_client::connect_timeout_for(
                    Duration::from_secs(self.timeout_seconds),
                ))
                .cookie_provider(Arc::clone(&request_cookie_jar))
                .redirect(crate::utils::http_client::create_ssrf_safe_redirect_policy(
                    5,
                ))
                .local_address(Some(std::net::Ipv4Addr::UNSPECIFIED.into()))
                .danger_accept_invalid_certs(true);
            if let Some(proxy_url) = handle.used_proxy_url.take() {
                match build_all_proxy(&proxy_url) {
                    Ok(px) => b = b.proxy(px),
                    Err(e) => {
                        error!(
                            "Failed to parse proxy URL for temp TLS-bypass client; \
                             request will NOT use the proxy: {}; proxy_url={}",
                            e,
                            redact_proxy_url(&proxy_url)
                        );
                    }
                }
            }
            Some(
                b.build()
                    .map_err(|e| EngineError::Other(format!("Failed build temp client: {}", e)))?,
            )
        } else {
            None
        };
        Ok(temp_client)
    }
}
