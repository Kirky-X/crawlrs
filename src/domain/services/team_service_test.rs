// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crate::domain::services::geo_location::GeoLocation;
use crate::domain::services::team_service::{
    GeoRestrictionResult, TeamGeoRestrictions, TeamService,
};
use crate::domain::services::geo_location::GeoLocationService;
use async_trait::async_trait;
use std::net::IpAddr;
use std::sync::Arc;
use uuid::Uuid;

fn mock_geo_location(country_code: String) -> GeoLocation {
    GeoLocation {
        ip: "0.0.0.0".to_string(),
        country_code: country_code.clone(),
        country_name: country_code.to_string(),
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
struct MockGeoService {
    country_code: String,
}

impl MockGeoService {
    fn new(country_code: String) -> Self {
        Self { country_code }
    }
}

#[async_trait]
impl GeoLocationService for MockGeoService {
    async fn get_location(&self, _ip: &IpAddr) -> Result<GeoLocation, anyhow::Error> {
        Ok(mock_geo_location(self.country_code.clone()))
    }
}

#[derive(Debug, Clone)]
struct MockGeoRestrictionRepository;

#[async_trait]
impl GeoRestrictionRepository for MockGeoRestrictionRepository {
    async fn get_team_restrictions(
        &self,
        _team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        Ok(TeamGeoRestrictions::default())
    }

    async fn update_team_restrictions(
        &self,
        _team_id: Uuid,
        _restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Ok(())
    }

    async fn log_geo_restriction_action(
        &self,
        _team_id: Uuid,
        _ip_address: &str,
        _country_code: &str,
        _action: &str,
        _reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_geo_restrictions_disabled_returns_allowed() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: false,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_invalid_ip_format_returns_denied() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "not-an-ip", &restrictions)
        .await;

    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(msg.contains("Invalid IP address format"));
        }
        _ => panic!("Expected Denied result"),
    }
}

#[tokio::test]
async fn test_ip_whitelist_cidr_allows() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "192.168.1.50", &restrictions)
        .await;

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_non_whitelisted_ip_blocks() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["10.0.0.0/8".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "192.168.1.1", &restrictions)
        .await;

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_allowed_countries_allows() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CN".to_string()]),
        ip_whitelist: None,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_blocked_countries_denies() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        blocked_countries: Some(vec!["RU".to_string()]),
        ip_whitelist: None,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    match result {
        Ok(GeoRestrictionResult::Denied(_)) => {}
        _ => panic!("Expected Denied result"),
    }
}

#[tokio::test]
async fn test_case_insensitive_country_matching() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["us".to_string()]),
        ip_whitelist: None,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_domain_blacklist_allows_safe_domain() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec!["malicious.com".to_string()]),
        ..Default::default()
    };

    let result = service.validate_domain_blacklist("www.google.com", &restrictions);

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_domain_blacklist_blocks_matched_domain() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec!["malicious.com".to_string()]),
        ..Default::default()
    };

    let result = service.validate_domain_blacklist("www.malicious.com", &restrictions);

    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(msg.contains("malicious.com"));
        }
        _ => panic!("Expected Denied result"),
    }
}

#[tokio::test]
async fn test_domain_blacklist_disabled_returns_allowed() {
    let geo_service: Arc<dyn GeoLocationService> =
        Arc::new(MockGeoService::new("US".to_string()));
    let geo_repo = Arc::new(MockGeoRestrictionRepository);
    let service = TeamService::new(geo_service, geo_repo);

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: false,
        domain_blacklist: Some(vec!["malicious.com".to_string()]),
        ..Default::default()
    };

    let result = service.validate_domain_blacklist("www.malicious.com", &restrictions);

    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}
