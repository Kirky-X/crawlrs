// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! GeoLocation domain service trait and models
//!
//! This module defines the domain-level abstraction for IP geolocation services.
//! The trait is defined in the domain layer, while implementations reside in the
//! infrastructure layer, following the Dependency Inversion Principle.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use shaku::Interface;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// IP地理位置信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    /// IP地址
    pub ip: String,
    /// 国家代码 (ISO 3166-1 alpha-2)
    pub country_code: String,
    /// 国家名称
    pub country_name: String,
    /// 地区/州
    pub region: Option<String>,
    /// 城市
    pub city: Option<String>,
    /// 纬度
    pub latitude: Option<f64>,
    /// 经度
    pub longitude: Option<f64>,
    /// ISP
    pub isp: Option<String>,
    /// 组织
    pub org: Option<String>,
}

impl Default for GeoLocation {
    fn default() -> Self {
        Self {
            ip: String::new(),
            country_code: "UNKNOWN".to_string(),
            country_name: "Unknown".to_string(),
            region: None,
            city: None,
            latitude: None,
            longitude: None,
            isp: None,
            org: None,
        }
    }
}

/// IP地理定位服务Trait
///
/// 定义IP地址地理位置查询的抽象接口。
/// 实现此trait的具体服务位于基础设施层。
#[async_trait::async_trait]
pub trait GeoLocationService: Interface + Send + Sync {
    /// 获取指定IP地址的地理位置信息
    ///
    /// # 参数
    ///
    /// * `ip` - IP地址
    ///
    /// # 返回值
    ///
    /// * `Ok(GeoLocation)` - 地理位置信息
    /// * `Err(anyhow::Error)` - 查询失败
    async fn get_location(&self, ip: &IpAddr) -> Result<GeoLocation>;
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
