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
use tracing::{debug, error, warn};

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
}
