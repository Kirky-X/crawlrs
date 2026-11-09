// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 重定向 DNS 级 SSRF 检查（从 `http_client.rs` 拆分）

/// DNS 级检查（检查 3，自 [`create_ssrf_safe_redirect_policy`] 提取）：
/// 重定向目标 hostname 解析出的全部 IP 必须是公网地址，命中阻断条件时返回 `true`。
///
/// 阻断「302 → 攻击者域名 → 解析到 169.254.169.254/内网 IP」的绕过
/// （该路径不受初始请求 resolve_to_addrs pin 的约束）。策略闭包为同步
/// 上下文，使用阻塞式解析；解析失败按 fail-closed 拒绝跟随。
pub(super) fn redirect_dns_check_blocks(redirect_url: &reqwest::Url) -> bool {
    use crate::infrastructure::security::ssrf::is_private_ip;
    use std::net::ToSocketAddrs;

    if let Some(host) = redirect_url.host_str() {
        let port =
            redirect_url
                .port_or_known_default()
                .unwrap_or(if redirect_url.scheme() == "https" {
                    443
                } else {
                    80
                });
        match (host, port).to_socket_addrs() {
            Ok(addrs) => {
                let mut any = false;
                for addr in addrs {
                    any = true;
                    if is_private_ip(addr.ip()) {
                        log::warn!(
                            "SSRF protection: Redirect host {} resolves to private IP {}, blocking",
                            host,
                            addr.ip()
                        );
                        return true;
                    }
                }
                if !any {
                    log::warn!(
                        "SSRF protection: No DNS result for redirect host {}, blocking",
                        host
                    );
                    return true;
                }
            }
            Err(e) => {
                log::warn!(
                    "SSRF protection: DNS resolution failed for redirect host {} ({}), blocking",
                    host,
                    e
                );
                return true;
            }
        }
    }
    false
}
