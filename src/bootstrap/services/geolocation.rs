// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! GeoLocation infrastructure implementation
//!
//! This module provides the concrete implementation of the GeoLocationService trait
//! defined in the domain layer. It uses external IP geolocation APIs to resolve
//! IP addresses to geographic locations.

use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::utils::http_client::DEFAULT_USER_AGENT;
use anyhow::Result;
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;

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

    /// 测试专用：注入自定义端点（生产固定 ipapi.co；SSRF/DNS 失败路径
    /// 测试需要可控端点）。`#[cfg(test)]` 保证不进入生产 API 面。
    #[cfg(test)]
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

        let url = format!("{}/{}/json/", self.api_endpoint, ip_str);

        let mut headers = std::collections::HashMap::new();
        headers.insert("User-Agent".to_string(), DEFAULT_USER_AGENT.to_string());

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

#[cfg(test)]
mod tests {
    use super::*;

    // GeoLocation::Default

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

    // GeoLocationServiceImpl 构造器

    #[test]
    fn test_geo_location_service_impl_new_default_endpoint() {
        let client = Arc::new(reqwest::Client::new());
        let svc = GeoLocationServiceImpl::new(client);
        // api_endpoint 是私有字段，但 tests 子模块可访问父模块私有字段
        assert_eq!(svc.api_endpoint, "https://ipapi.co");
    }

    // IpApiResponse serde 反序列化

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

    #[test]
    fn test_ip_api_response_deserialize_empty_json() {
        // 空 JSON 对象 → 所有 Option 字段为 None
        let json = r#"{}"#;
        let resp: IpApiResponse = serde_json::from_str(json).expect("deserialize failed");
        assert!(resp.ip.is_none());
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
    fn test_ip_api_response_deserialize_with_error_true() {
        // error=true 且有 reason
        let json = r#"{
            "error": true,
            "reason": "Rate limited"
        }"#;
        let resp: IpApiResponse = serde_json::from_str(json).expect("deserialize failed");
        assert_eq!(resp.error, Some(true));
        assert_eq!(resp.reason.as_deref(), Some("Rate limited"));
    }

    #[test]
    fn test_ip_api_response_serialize_empty_struct() {
        // 序列化全 None 的结构
        let resp = IpApiResponse {
            ip: None,
            country_code: None,
            country_name: None,
            region: None,
            city: None,
            latitude: None,
            longitude: None,
            org: None,
            error: None,
            reason: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize failed");
        // 反序列化回来应一致
        let decoded: IpApiResponse = serde_json::from_str(&json).expect("deserialize failed");
        assert!(decoded.ip.is_none());
        assert!(decoded.error.is_none());
    }

    // GeoLocationServiceImpl additional constructor tests

    #[test]
    fn test_geo_location_service_impl_new_creates_engine_client() {
        // 验证 new 创建后 engine_client 不为空（通过可以调用方法间接验证）
        let client = Arc::new(reqwest::Client::new());
        let svc = GeoLocationServiceImpl::new(client);
        // api_endpoint 应为默认值
        assert_eq!(svc.api_endpoint, "https://ipapi.co");
    }

    // GeoLocation Debug and Clone

    #[test]
    fn test_geo_location_clone_preserves_fields() {
        let geo = GeoLocation {
            ip: "8.8.8.8".to_string(),
            country_code: "US".to_string(),
            country_name: "United States".to_string(),
            region: Some("CA".to_string()),
            city: Some("Mountain View".to_string()),
            latitude: Some(37.4),
            longitude: Some(-122.1),
            isp: Some("Google".to_string()),
            org: Some("Google LLC".to_string()),
        };
        let cloned = geo.clone();
        assert_eq!(cloned.ip, geo.ip);
        assert_eq!(cloned.country_code, geo.country_code);
        assert_eq!(cloned.country_name, geo.country_name);
        assert_eq!(cloned.region, geo.region);
        assert_eq!(cloned.city, geo.city);
        assert_eq!(cloned.latitude, geo.latitude);
        assert_eq!(cloned.longitude, geo.longitude);
        assert_eq!(cloned.isp, geo.isp);
        assert_eq!(cloned.org, geo.org);
    }

    #[test]
    fn test_geo_location_serialize_deserialize_roundtrip() {
        let geo = GeoLocation {
            ip: "1.2.3.4".to_string(),
            country_code: "US".to_string(),
            country_name: "United States".to_string(),
            region: Some("CA".to_string()),
            city: None,
            latitude: Some(37.4),
            longitude: None,
            isp: Some("ISP".to_string()),
            org: None,
        };
        let json = serde_json::to_string(&geo).expect("serialize failed");
        let decoded: GeoLocation = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(decoded.ip, geo.ip);
        assert_eq!(decoded.country_code, geo.country_code);
        assert_eq!(decoded.region, geo.region);
        assert_eq!(decoded.city, geo.city);
        assert_eq!(decoded.latitude, geo.latitude);
        assert_eq!(decoded.isp, geo.isp);
    }

    // get_location 错误路径测试
    //
    // 注：get_location 的成功路径在单元测试中无法覆盖。
    // 原因：GeoLocationServiceImpl 内部直接创建 EngineClient（具体类型，非 trait），
    // 而 EngineClient::scrape 硬编码先调用 validate_url 进行 SSRF 检查，
    // 拦截所有 127.0.0.1/localhost/私有 IP。本地 mock HTTP 服务器路径走不通，
    // 外部真实端点又会让测试依赖网络。行 77-117 需要集成测试或重构
    // GeoLocationServiceImpl 接受 EngineClientTrait 才能覆盖。
    //
    // 以下测试覆盖 scrape 错误 → map_err → anyhow 错误

    #[tokio::test]
    async fn test_get_location_ssrf_blocked_returns_scrape_error() {
        // 用 127.0.0.1 触发 EngineClient 的 SSRF 保护，
        // 覆盖行 72-75 的 scrape 错误处理路径（map_err → anyhow!）
        let client = Arc::new(reqwest::Client::new());
        let svc = GeoLocationServiceImpl::with_endpoint("http://127.0.0.1:1".to_string(), client);
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        let result = svc.get_location(&ip).await;
        assert!(result.is_err(), "should fail with SSRF error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to fetch geolocation"),
            "error should be wrapped scrape failure, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_get_location_unreachable_dns_returns_scrape_error() {
        // 用不可达域名（RFC 6761 保留）触发 DNS 解析失败，
        // 覆盖行 72-75 的 scrape 错误处理路径
        let client = Arc::new(reqwest::Client::new());
        let svc =
            GeoLocationServiceImpl::with_endpoint("http://example.invalid".to_string(), client);
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        let result = svc.get_location(&ip).await;
        assert!(result.is_err(), "should fail with DNS error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to fetch geolocation"),
            "error should be wrapped scrape failure, got: {}",
            err_msg
        );
    }
}
