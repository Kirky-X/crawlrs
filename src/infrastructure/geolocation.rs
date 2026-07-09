// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! GeoLocation infrastructure implementation
//!
//! This module provides the concrete implementation of the GeoLocationService trait
//! defined in the domain layer. It uses external IP geolocation APIs to resolve
//! IP addresses to geographic locations.

use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use log::{debug, error, warn};

/// IP地理定位服务实现
pub struct GeoLocationServiceImpl {
    /// API端点 (默认为 ipapi.co)
    api_endpoint: String,
    engine_client: Arc<EngineClient>,
}

impl GeoLocationServiceImpl {
    /// 创建新的地理定位服务实例
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        let engine_client = Self::create_engine_client(client);
        Self {
            api_endpoint: "https://ipapi.co".to_string(),
            engine_client,
        }
    }

    /// 使用自定义API端点创建服务实例
    pub fn with_endpoint(api_endpoint: String, client: Arc<reqwest::Client>) -> Self {
        let engine_client = Self::create_engine_client(client);
        Self {
            api_endpoint,
            engine_client,
        }
    }

    fn create_engine_client(http_client: Arc<reqwest::Client>) -> Arc<EngineClient> {
        let reqwest_engine = ReqwestEngine::new(http_client);
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
        Arc::new(EngineClient::with_router(router))
    }
}

#[async_trait::async_trait]
impl GeoLocationService for GeoLocationServiceImpl {
    async fn get_location(&self, ip: &IpAddr) -> Result<GeoLocation> {
        let ip_str = ip.to_string();
        debug!("Getting geolocation for IP: {}", ip_str);

        // 构建API请求URL
        let url = format!("{}/{}/json/", self.api_endpoint, ip_str);

        // 发送请求
        let mut headers = std::collections::HashMap::new();
        headers.insert("User-Agent".to_string(), "crawlrs/0.1.0".to_string());

        let request = ScrapeRequest::new(&url)
            .with_options(ScrapeOptions::builder().headers(headers).build());

        let response = self.engine_client.scrape(&request).await.map_err(|e| {
            error!("Failed to fetch geolocation for IP {}: {}", ip_str, e);
            anyhow::anyhow!("Failed to fetch geolocation: {}", e)
        })?;

        if !response.is_success() {
            error!(
                "Geolocation API returned error status: {} for IP {}",
                response.status_code, ip_str
            );
            return Err(anyhow::anyhow!(
                "Geolocation API error: {}",
                response.status_code
            ));
        }

        let api_response: IpApiResponse = serde_json::from_str(&response.content).map_err(|e| {
            error!(
                "Failed to parse geolocation response for IP {}: {}",
                ip_str, e
            );
            anyhow::anyhow!("Failed to parse geolocation response: {}", e)
        })?;

        // 转换为GeoLocation结构
        let geo_location = GeoLocation {
            ip: ip_str.clone(),
            country_code: api_response.country_code.unwrap_or_else(|| {
                warn!("No country code found for IP: {}", ip_str);
                "UNKNOWN".to_string()
            }),
            country_name: api_response.country_name.unwrap_or_default(),
            region: api_response.region,
            city: api_response.city,
            latitude: api_response.latitude,
            longitude: api_response.longitude,
            isp: api_response.org.clone(),
            org: api_response.org,
        };

        debug!(
            "Successfully retrieved geolocation for IP {}: {:?}",
            ip_str, geo_location
        );
        Ok(geo_location)
    }
}

impl GeoLocationServiceImpl {
    /// 批量获取IP地址的地理位置信息
    ///
    /// # 参数
    ///
    /// * `ips` - IP地址列表
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<GeoLocation>)` - 地理位置信息列表
    /// * `Err(anyhow::Error)` - 获取失败
    pub async fn get_locations(&self, ips: &[IpAddr]) -> Result<Vec<GeoLocation>> {
        let mut results = Vec::new();

        for ip in ips {
            match self.get_location(ip).await {
                Ok(location) => results.push(location),
                Err(e) => {
                    warn!("Failed to get geolocation for IP {}: {}", ip, e);
                    // 对于失败的IP，返回一个默认的未知位置
                    results.push(GeoLocation {
                        ip: ip.to_string(),
                        ..Default::default()
                    });
                }
            }
        }

        Ok(results)
    }
}

/// IP API 响应结构
#[derive(Debug, Serialize, Deserialize)]
struct IpApiResponse {
    ip: Option<String>,
    #[serde(rename = "country_code")]
    country_code: Option<String>,
    #[serde(rename = "country_name")]
    country_name: Option<String>,
    region: Option<String>,
    city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    org: Option<String>,
    error: Option<bool>,
    reason: Option<String>,
}

/// 检查IP地址是否在CIDR范围内
///
/// # 参数
///
/// * `ip` - IP地址
/// * `cidr` - CIDR表示法 (如 "192.168.1.0/24")
///
/// # 返回值
///
/// * `bool` - 如果IP在CIDR范围内返回true，否则返回false
pub fn is_ip_in_cidr(ip: &IpAddr, cidr: &str) -> bool {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    let network_str = parts[0];
    let prefix_length = parts[1].parse::<u8>().unwrap_or(0);

    match (ip, network_str.parse::<IpAddr>()) {
        (IpAddr::V4(ip_v4), Ok(IpAddr::V4(network_v4))) => {
            is_ipv4_in_cidr(ip_v4, &network_v4, prefix_length)
        }
        (IpAddr::V6(ip_v6), Ok(IpAddr::V6(network_v6))) => {
            is_ipv6_in_cidr(ip_v6, &network_v6, prefix_length)
        }
        _ => false,
    }
}

fn is_ipv4_in_cidr(ip: &Ipv4Addr, network: &Ipv4Addr, prefix_length: u8) -> bool {
    if prefix_length > 32 {
        return false;
    }

    let ip_int = u32::from_be_bytes(ip.octets());
    let network_int = u32::from_be_bytes(network.octets());
    let mask = if prefix_length == 0 {
        0
    } else {
        (!0u32) << (32 - prefix_length)
    };

    (ip_int & mask) == (network_int & mask)
}

fn is_ipv6_in_cidr(ip: &Ipv6Addr, network: &Ipv6Addr, prefix_length: u8) -> bool {
    if prefix_length > 128 {
        return false;
    }

    let ip_segments = ip.segments();
    let network_segments = network.segments();

    let full_segments = prefix_length / 16;
    let remaining_bits = prefix_length % 16;

    // 检查完整的段
    for i in 0..full_segments.min(8) {
        if ip_segments[i as usize] != network_segments[i as usize] {
            return false;
        }
    }

    // 检查剩余的位
    if remaining_bits > 0 && full_segments < 8 {
        let mask = (!0u16) << (16 - remaining_bits);
        if (ip_segments[full_segments as usize] & mask)
            != (network_segments[full_segments as usize] & mask)
        {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_is_ipv4_in_cidr() {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let network = Ipv4Addr::new(192, 168, 1, 0);
        assert!(is_ipv4_in_cidr(&ip, &network, 24));

        let ip = Ipv4Addr::new(10, 0, 0, 5);
        let network = Ipv4Addr::new(10, 0, 0, 0);
        assert!(is_ipv4_in_cidr(&ip, &network, 8));

        let ip = Ipv4Addr::new(172, 16, 0, 1);
        let network = Ipv4Addr::new(172, 16, 0, 0);
        assert!(is_ipv4_in_cidr(&ip, &network, 16));

        // 不在范围内的情况
        let ip = Ipv4Addr::new(192, 168, 2, 100);
        let network = Ipv4Addr::new(192, 168, 1, 0);
        assert!(!is_ipv4_in_cidr(&ip, &network, 24));
    }

    #[test]
    fn test_is_ipv6_in_cidr() {
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0x8a2e, 0x370, 0x7334);
        let network = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0, 0, 0);
        assert!(is_ipv6_in_cidr(&ip, &network, 48));

        // 不在范围内的情况
        let ip = Ipv6Addr::new(0x2001, 0x0db9, 0x85a3, 0, 0, 0x8a2e, 0x370, 0x7334);
        let network = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0, 0, 0);
        assert!(!is_ipv6_in_cidr(&ip, &network, 48));
    }

    #[test]
    fn test_is_ip_in_cidr() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "192.168.1.0/24"));
        assert!(!is_ip_in_cidr(&ip, "192.168.2.0/24"));

        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "2001:db8::/32"));
        assert!(!is_ip_in_cidr(&ip, "2001:db9::/32"));
    }

    // =========================================================================
    // is_ip_in_cidr 边界与异常输入
    // =========================================================================

    #[test]
    fn test_is_ip_in_cidr_no_slash_returns_false() {
        // 无 "/" → parts.len() != 2 → false
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "192.168.1.0"));
    }

    #[test]
    fn test_is_ip_in_cidr_multiple_slashes_returns_false() {
        // 多个 "/" → parts.len() > 2 → false
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "192.168.1.0/24/16"));
    }

    #[test]
    fn test_is_ip_in_cidr_non_numeric_prefix_defaults_to_zero() {
        // 非数字 prefix_length → unwrap_or(0) → prefix_length=0 → 全匹配 true
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "192.168.1.0/abc"));
    }

    #[test]
    fn test_is_ip_in_cidr_prefix_zero_matches_all_ipv4() {
        // prefix_length = 0 → IPv4 全匹配
        let ip: IpAddr = "203.0.113.42".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "0.0.0.0/0"));
        assert!(is_ip_in_cidr(&ip, "10.0.0.0/0"));
    }

    #[test]
    fn test_is_ip_in_cidr_prefix_zero_matches_all_ipv6() {
        // prefix_length = 0 → IPv6 全匹配
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "::/0"));
    }

    #[test]
    fn test_is_ip_in_cidr_ipv4_prefix_too_large_returns_false() {
        // prefix_length > 32 → false（通过 is_ipv4_in_cidr 内部检查）
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "192.168.1.0/33"));
    }

    #[test]
    fn test_is_ip_in_cidr_ipv6_prefix_too_large_returns_false() {
        // prefix_length > 128 → false
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "2001:db8::/129"));
    }

    #[test]
    fn test_is_ip_in_cidr_ipv4_exact_match_prefix_32() {
        // prefix_length = 32 → IPv4 精确匹配
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "192.168.1.100/32"));
        assert!(!is_ip_in_cidr(&ip, "192.168.1.101/32"));
    }

    #[test]
    fn test_is_ip_in_cidr_ipv6_exact_match_prefix_128() {
        // prefix_length = 128 → IPv6 精确匹配
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(is_ip_in_cidr(&ip, "2001:db8::1/128"));
        assert!(!is_ip_in_cidr(&ip, "2001:db8::2/128"));
    }

    #[test]
    fn test_is_ip_in_cidr_ip_version_mismatch_returns_false() {
        // IPv4 ip vs IPv6 network → false
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "::1/128"));
        // IPv6 ip vs IPv4 network → false
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "192.168.1.0/24"));
    }

    #[test]
    fn test_is_ip_in_cidr_unparseable_network_returns_false() {
        // network 部分无法解析为 IpAddr → Ok 分支不匹配 → _ => false
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!is_ip_in_cidr(&ip, "not-an-ip/24"));
    }

    // =========================================================================
    // is_ipv4_in_cidr 边界
    // =========================================================================

    #[test]
    fn test_is_ipv4_in_cidr_prefix_zero_always_true() {
        // prefix_length = 0 → mask = 0 → 任何 IP 都匹配
        let ip = Ipv4Addr::new(203, 0, 113, 42);
        let network = Ipv4Addr::new(10, 0, 0, 1);
        assert!(is_ipv4_in_cidr(&ip, &network, 0));
    }

    #[test]
    fn test_is_ipv4_in_cidr_prefix_32_exact_match() {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        assert!(is_ipv4_in_cidr(&ip, &ip, 32));
        let other = Ipv4Addr::new(192, 168, 1, 101);
        assert!(!is_ipv4_in_cidr(&ip, &other, 32));
    }

    #[test]
    fn test_is_ipv4_in_cidr_prefix_33_returns_false() {
        let ip = Ipv4Addr::new(192, 168, 1, 1);
        let network = Ipv4Addr::new(192, 168, 1, 0);
        assert!(!is_ipv4_in_cidr(&ip, &network, 33));
    }

    #[test]
    fn test_is_ipv4_in_cidr_boundary_match() {
        // /24 边界：网络部分相同，主机部分不同
        let ip = Ipv4Addr::new(10, 20, 30, 40);
        let network = Ipv4Addr::new(10, 20, 30, 0);
        assert!(is_ipv4_in_cidr(&ip, &network, 24));
        let out_ip = Ipv4Addr::new(10, 20, 31, 40);
        assert!(!is_ipv4_in_cidr(&out_ip, &network, 24));
    }

    // =========================================================================
    // is_ipv6_in_cidr 边界
    // =========================================================================

    #[test]
    fn test_is_ipv6_in_cidr_prefix_zero_always_true() {
        // prefix_length = 0 → 全匹配
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1);
        let network = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0x1);
        assert!(is_ipv6_in_cidr(&ip, &network, 0));
    }

    #[test]
    fn test_is_ipv6_in_cidr_prefix_128_exact_match() {
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1);
        assert!(is_ipv6_in_cidr(&ip, &ip, 128));
        let other = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x2);
        assert!(!is_ipv6_in_cidr(&ip, &other, 128));
    }

    #[test]
    fn test_is_ipv6_in_cidr_prefix_129_returns_false() {
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1);
        let network = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0);
        assert!(!is_ipv6_in_cidr(&ip, &network, 129));
    }

    #[test]
    fn test_is_ipv6_in_cidr_remaining_bits_zero() {
        // prefix_length 整除 16（如 48），remaining_bits=0 分支
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x1234, 0, 0, 0, 0x1);
        let network = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0, 0, 0);
        assert!(is_ipv6_in_cidr(&ip, &network, 48));
        // 前三段不同 → false
        let diff_network = Ipv6Addr::new(0x2001, 0x0db8, 0x85a4, 0, 0, 0, 0, 0);
        assert!(!is_ipv6_in_cidr(&ip, &diff_network, 48));
    }

    #[test]
    fn test_is_ipv6_in_cidr_remaining_bits_nonzero_partial_segment() {
        // prefix_length 不整除 16（如 17），remaining_bits=1 分支
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x1);
        // prefix 17: 第 0 段完整匹配(2001)，第 1 段前 1 位匹配(0x0db8 & 0x8000 == 0x0db8 & 0x8000)
        // 0x0db8 的最高位是 0（0x0db8 < 0x8000），所以 network 第 1 段最高位也需为 0
        let network = Ipv6Addr::new(0x2001, 0x0000, 0, 0, 0, 0, 0, 0);
        assert!(is_ipv6_in_cidr(&ip, &network, 17));

        // network 第 1 段最高位为 1 → 不匹配
        let network_msb = Ipv6Addr::new(0x2001, 0x8000, 0, 0, 0, 0, 0, 0);
        assert!(!is_ipv6_in_cidr(&ip, &network_msb, 17));
    }

    #[test]
    fn test_is_ipv6_in_cidr_remaining_bits_partial_match_and_mismatch() {
        // prefix 120 = 7*16 + 8，remaining_bits=8，第 7 段前 8 位
        let ip = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0x8a2e, 0x370, 0x7334);
        // 第 7 段前 8 位匹配（0x7334 & 0xFF00 == 0x7300 == network & 0xFF00）
        let network_match = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0x8a2e, 0x370, 0x7300);
        assert!(is_ipv6_in_cidr(&ip, &network_match, 120));
        // 第 7 段前 8 位不匹配
        let network_mismatch = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0x8a2e, 0x370, 0xff34);
        assert!(!is_ipv6_in_cidr(&ip, &network_mismatch, 120));
    }

    // =========================================================================
    // GeoLocation::Default
    // =========================================================================

    #[test]
    fn test_geo_location_default_fields() {
        let geo = GeoLocation::default();
        assert_eq!(geo.ip, "");
        assert_eq!(geo.country_code, "UNKNOWN");
        assert_eq!(geo.country_name, "Unknown");
        assert!(geo.region.is_none());
        assert!(geo.city.is_none());
        assert!(geo.latitude.is_none());
        assert!(geo.longitude.is_none());
        assert!(geo.isp.is_none());
        assert!(geo.org.is_none());
    }

    // =========================================================================
    // GeoLocationServiceImpl 构造器
    // =========================================================================

    #[test]
    fn test_geo_location_service_impl_new_default_endpoint() {
        let client = Arc::new(reqwest::Client::new());
        let svc = GeoLocationServiceImpl::new(client);
        // api_endpoint 是私有字段，但 tests 子模块可访问父模块私有字段
        assert_eq!(svc.api_endpoint, "https://ipapi.co");
    }

    #[test]
    fn test_geo_location_service_impl_with_endpoint_custom() {
        let client = Arc::new(reqwest::Client::new());
        let svc = GeoLocationServiceImpl::with_endpoint(
            "https://ip-api.com/json".to_string(),
            client,
        );
        assert_eq!(svc.api_endpoint, "https://ip-api.com/json");
    }

    // =========================================================================
    // IpApiResponse serde 反序列化
    // =========================================================================

    #[test]
    fn test_ip_api_response_deserialize_full() {
        let json = r#"{
            "ip": "8.8.8.8",
            "country_code": "US",
            "country_name": "United States",
            "region": "California",
            "city": "Mountain View",
            "latitude": 37.4056,
            "longitude": -122.0775,
            "org": "Google LLC",
            "error": false,
            "reason": null
        }"#;
        let resp: IpApiResponse = serde_json::from_str(json).expect("deserialize failed");
        assert_eq!(resp.ip.as_deref(), Some("8.8.8.8"));
        assert_eq!(resp.country_code.as_deref(), Some("US"));
        assert_eq!(resp.country_name.as_deref(), Some("United States"));
        assert_eq!(resp.region.as_deref(), Some("California"));
        assert_eq!(resp.city.as_deref(), Some("Mountain View"));
        assert!((resp.latitude.unwrap() - 37.4056).abs() < 1e-4);
        assert!((resp.longitude.unwrap() - (-122.0775)).abs() < 1e-4);
        assert_eq!(resp.org.as_deref(), Some("Google LLC"));
        assert_eq!(resp.error, Some(false));
        assert!(resp.reason.is_none());
    }

    #[test]
    fn test_ip_api_response_deserialize_partial() {
        // 缺失字段应反序列化为 None
        let json = r#"{"ip": "1.2.3.4"}"#;
        let resp: IpApiResponse = serde_json::from_str(json).expect("deserialize failed");
        assert_eq!(resp.ip.as_deref(), Some("1.2.3.4"));
        assert!(resp.country_code.is_none());
        assert!(resp.country_name.is_none());
        assert!(resp.region.is_none());
        assert!(resp.city.is_none());
        assert!(resp.latitude.is_none());
        assert!(resp.longitude.is_none());
        assert!(resp.org.is_none());
        assert!(resp.error.is_none());
        assert!(resp.reason.is_none());
    }

    #[test]
    fn test_ip_api_response_deserialize_error_response() {
        let json = r#"{
            "ip": "invalid",
            "error": true,
            "reason": "Invalid IP address"
        }"#;
        let resp: IpApiResponse = serde_json::from_str(json).expect("deserialize failed");
        assert_eq!(resp.error, Some(true));
        assert_eq!(resp.reason.as_deref(), Some("Invalid IP address"));
    }

    #[test]
    fn test_ip_api_response_serialize_roundtrip() {
        let original = IpApiResponse {
            ip: Some("8.8.4.4".to_string()),
            country_code: Some("US".to_string()),
            country_name: Some("United States".to_string()),
            region: None,
            city: Some("Mountain View".to_string()),
            latitude: Some(37.4),
            longitude: Some(-122.1),
            org: Some("Google".to_string()),
            error: Some(false),
            reason: None,
        };
        let json = serde_json::to_string(&original).expect("serialize failed");
        let decoded: IpApiResponse = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(decoded.ip, original.ip);
        assert_eq!(decoded.country_code, original.country_code);
        assert_eq!(decoded.country_name, original.country_name);
        assert_eq!(decoded.city, original.city);
        assert!((decoded.latitude.unwrap() - 37.4).abs() < 1e-6);
        assert_eq!(decoded.org, original.org);
        assert_eq!(decoded.error, original.error);
    }
}
