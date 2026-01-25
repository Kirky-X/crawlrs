// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 地理限制管理示例
//!
//! 演示如何配置团队地理访问限制。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin geo_restrictions
//! ```
//!
//! ## 核心功能
//!
//! - IP 白名单/黑名单配置
//! - 国家/地区访问控制
//! - 域名黑名单

use tracing::info;
use std::net::IpAddr;
use uuid::Uuid;

// 模拟地理限制配置
#[derive(Debug, Clone)]
struct TeamGeoRestrictions {
    pub enable_geo_restrictions: bool,
    pub allowed_countries: Option<Vec<String>>,
    pub blocked_countries: Option<Vec<String>>,
    pub ip_whitelist: Option<Vec<String>>,
    pub domain_blacklist: Option<Vec<String>>,
}

impl Default for TeamGeoRestrictions {
    fn default() -> Self {
        Self {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "GB".to_string(), "DE".to_string(), "JP".to_string()]),
            blocked_countries: Some(vec!["CN".to_string(), "RU".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
            domain_blacklist: Some(vec!["malicious.com".to_string()]),
        }
    }
}

// 模拟地理限制验证结果
#[derive(Debug, Clone)]
enum GeoRestrictionResult {
    Allowed,
    Blocked(String),
}

fn validate_ip_whitelist(ip: IpAddr, whitelist: &Option<Vec<String>>) -> bool {
    if let Some(ref whitelists) = whitelist {
        for cidr in whitelists {
            if is_ip_in_cidr(ip, cidr) {
                return true; // 在白名单中，允许
            }
        }
    }
    false
}

fn is_ip_in_cidr(ip: IpAddr, cidr: &str) -> bool {
    // 简化的 CIDR 检查（实际使用 ipnet  crate）
    cidr.contains(&format!("{}", ip))
        || (ip.is_ipv4() && cidr == "0.0.0.0/0")
}

fn validate_domain(domain: &str, blacklist: &Option<Vec<String>>) -> Result<(), String> {
    if let Some(ref blocked_domains) = blacklist {
        for bad_domain in blocked_domains {
            if domain.contains(bad_domain) || bad_domain.contains(domain) {
                return Err(format!("Domain '{}' is blocked", domain));
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("=== 地理限制管理示例 ===\n");

    let team_id = Uuid::new_v4();
    let restrictions = TeamGeoRestrictions::default();

    info!("Team ID: {}", team_id);
    info!("Geo restrictions enabled: {}", restrictions.enable_geo_restrictions);

    // 验证 IP 白名单
    info!("\n--- IP 白名单验证 ---");
    let test_ips = vec!["192.168.1.100", "10.0.0.1", "8.8.8.8"];
    for ip_str in test_ips {
        match ip_str.parse::<IpAddr>() {
            Ok(ip) => {
                if validate_ip_whitelist(ip, &restrictions.ip_whitelist) {
                    info!("  {}: ✅ Allowed (whitelisted)", ip_str);
                } else {
                    info!("  {}: ⚠️ Requires further validation", ip_str);
                }
            }
            Err(_) => info!("  {}: Invalid IP", ip_str),
        }
    }

    // 验证域名黑名单
    info!("\n--- 域名黑名单验证 ---");
    let test_domains = vec!["example.com", "malicious.com", "safe-site.org"];
    for domain in test_domains {
        match validate_domain(domain, &restrictions.domain_blacklist) {
            Ok(()) => info!("  {}: ✅ Allowed", domain),
            Err(msg) => info!("  {}: Blocked - {}", domain, msg),
        }
    }

    // 国家/地区配置
    info!("\n--- 国家/地区配置 ---");
    if let Some(ref allowed) = restrictions.allowed_countries {
        info!("  Allowed countries: {}", allowed.join(", "));
    }
    if let Some(ref blocked) = restrictions.blocked_countries {
        info!("  Blocked countries: {}", blocked.join(", "));
    }

    info!("\n=== 地理限制示例完成 ===");
}
