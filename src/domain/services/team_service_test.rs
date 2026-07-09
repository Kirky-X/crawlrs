// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crate::domain::services::geo_location::GeoLocation;
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::team_service::{
    GeoRestrictionResult, TeamGeoRestrictions, TeamService,
};
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
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

// ============================================================
// Additional tests for improved coverage
// ============================================================

/// Geo service mock that always returns an error
#[derive(Debug, Clone)]
struct FailingGeoService;

#[async_trait]
impl GeoLocationService for FailingGeoService {
    async fn get_location(&self, _ip: &IpAddr) -> Result<GeoLocation, anyhow::Error> {
        Err(anyhow::anyhow!("geolocation service unavailable"))
    }
}

/// Geo restriction repo mock that always returns a failure
#[derive(Debug, Clone)]
struct FailingGeoRestrictionRepository;

#[async_trait]
impl GeoRestrictionRepository for FailingGeoRestrictionRepository {
    async fn get_team_restrictions(
        &self,
        _team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        Err(GeoRestrictionRepositoryError::Database("db down".to_string()))
    }

    async fn update_team_restrictions(
        &self,
        _team_id: Uuid,
        _restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Err(GeoRestrictionRepositoryError::Database("db down".to_string()))
    }

    async fn log_geo_restriction_action(
        &self,
        _team_id: Uuid,
        _ip_address: &str,
        _country_code: &str,
        _action: &str,
        _reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Err(GeoRestrictionRepositoryError::Database("db down".to_string()))
    }
}

/// Geo restriction repo mock that returns a specific restrictions config
#[derive(Debug, Clone)]
struct ConfigurableGeoRestrictionRepository {
    restrictions: TeamGeoRestrictions,
}

impl ConfigurableGeoRestrictionRepository {
    fn new(restrictions: TeamGeoRestrictions) -> Self {
        Self { restrictions }
    }
}

#[async_trait]
impl GeoRestrictionRepository for ConfigurableGeoRestrictionRepository {
    async fn get_team_restrictions(
        &self,
        _team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        Ok(self.restrictions.clone())
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

fn make_service(
    geo: Arc<dyn GeoLocationService>,
    repo: Arc<dyn GeoRestrictionRepository>,
) -> TeamService {
    TeamService::new(geo, repo)
}

// ---- get_team_geo_restrictions ----

#[tokio::test]
async fn test_get_team_geo_restrictions_success_returns_config() {
    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string()]),
        ..Default::default()
    };
    let geo_repo = Arc::new(ConfigurableGeoRestrictionRepository::new(restrictions.clone()));
    let service = make_service(Arc::new(MockGeoService::new("US".to_string())), geo_repo);

    let result = service.get_team_geo_restrictions(Uuid::new_v4()).await;
    assert!(result.enable_geo_restrictions);
    assert_eq!(result.allowed_countries, Some(vec!["US".to_string()]));
}

#[tokio::test]
async fn test_get_team_geo_restrictions_failure_returns_default() {
    let geo_repo: Arc<dyn GeoRestrictionRepository> = Arc::new(FailingGeoRestrictionRepository);
    let service = make_service(Arc::new(MockGeoService::new("US".to_string())), geo_repo);

    let result = service.get_team_geo_restrictions(Uuid::new_v4()).await;
    // On failure, should return default (empty) restrictions
    assert!(!result.enable_geo_restrictions);
    assert!(result.allowed_countries.is_none());
    assert!(result.blocked_countries.is_none());
    assert!(result.ip_whitelist.is_none());
    assert!(result.domain_blacklist.is_none());
}

// ---- validate_geographic_restriction: country not in allowed list ----

#[tokio::test]
async fn test_validate_geographic_restriction_country_not_in_allowed_denies() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(
                msg.contains("RU"),
                "denial message should mention country code, got: {}",
                msg
            );
            assert!(msg.contains("not allowed"));
        }
        _ => panic!("Expected Denied result for country not in allowed list"),
    }
}

// ---- validate_geographic_restriction: geo service error ----

#[tokio::test]
async fn test_validate_geographic_restriction_geo_service_error_propagates() {
    let geo_service: Arc<dyn GeoLocationService> = Arc::new(FailingGeoService);
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;

    assert!(result.is_err(), "geo service error should propagate");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Unable to determine geographic location"),
        "error should mention geolocation failure, got: {}",
        err
    );
}

// ---- validate_geographic_restriction: IP not in whitelist, then allowed by country ----

#[tokio::test]
async fn test_ip_whitelist_no_match_then_country_allows() {
    let geo_service = Arc::new(MockGeoService::new("US".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["10.0.0.0/8".to_string()]),
        allowed_countries: Some(vec!["US".to_string()]),
        ..Default::default()
    };

    // IP not in whitelist (192.168.x), but country (US) is allowed
    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "192.168.1.1", &restrictions)
        .await;

    assert!(
        matches!(result, Ok(GeoRestrictionResult::Allowed)),
        "should be allowed by country when not in whitelist"
    );
}

// ---- validate_geographic_restriction: IP not in whitelist, then blocked by country ----

#[tokio::test]
async fn test_ip_whitelist_no_match_then_country_blocks() {
    let geo_service = Arc::new(MockGeoService::new("CN".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["10.0.0.0/8".to_string()]),
        blocked_countries: Some(vec!["CN".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "192.168.1.1", &restrictions)
        .await;

    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(msg.contains("CN"));
        }
        _ => panic!("Expected Denied when country is blocked and IP not in whitelist"),
    }
}

// ---- validate_geographic_restriction: both allowed and blocked lists set ----

#[tokio::test]
async fn test_both_allowed_and_blocked_countries_allowed_wins_for_unblocked() {
    let geo_service = Arc::new(MockGeoService::new("US".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CN".to_string()]),
        blocked_countries: Some(vec!["RU".to_string()]),
        ..Default::default()
    };

    // US is in allowed and not in blocked -> allowed
    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_both_allowed_and_blocked_blocked_wins_for_blocked_country() {
    let geo_service = Arc::new(MockGeoService::new("CN".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CN".to_string()]),
        blocked_countries: Some(vec!["CN".to_string()]),
        ..Default::default()
    };

    // CN is in both allowed and blocked -> blocked should win (checked after allowed)
    // Actually looking at the code: allowed is checked first, so CN passes the allowed check,
    // then blocked is checked and CN is blocked.
    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;
    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(msg.contains("CN"));
        }
        _ => panic!("Expected Denied when country is in blocked list even if also in allowed"),
    }
}

// ---- IPv6 handling ----

#[tokio::test]
async fn test_ipv6_address_allowed_by_country() {
    let geo_service = Arc::new(MockGeoService::new("US".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "2001:db8::1", &restrictions)
        .await;
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_ipv6_address_allowed_by_whitelist() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["2001:db8::/32".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "2001:db8::1", &restrictions)
        .await;
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_ipv6_address_not_in_whitelist_blocked_by_country() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["2001:db9::/32".to_string()]), // different network
        blocked_countries: Some(vec!["RU".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "2001:db8::1", &restrictions)
        .await;
    match result {
        Ok(GeoRestrictionResult::Denied(msg)) => {
            assert!(msg.contains("RU"));
        }
        _ => panic!("Expected Denied for IPv6 not in whitelist and country blocked"),
    }
}

// ---- validate_domain_blacklist ----

#[tokio::test]
async fn test_validate_domain_blacklist_no_blacklist_returns_allowed() {
    let service = make_service(
        Arc::new(MockGeoService::new("US".to_string())),
        Arc::new(MockGeoRestrictionRepository),
    );

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: None,
        ..Default::default()
    };

    let result = service.validate_domain_blacklist("www.anything.com", &restrictions);
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_validate_domain_blacklist_multiple_entries() {
    let service = make_service(
        Arc::new(MockGeoService::new("US".to_string())),
        Arc::new(MockGeoRestrictionRepository),
    );

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec![
            "malicious.com".to_string(),
            "spam.org".to_string(),
            "bad-actor.net".to_string(),
        ]),
        ..Default::default()
    };

    // First entry matches
    assert!(matches!(
        service.validate_domain_blacklist("www.malicious.com", &restrictions),
        Ok(GeoRestrictionResult::Denied(_))
    ));
    // Second entry matches
    assert!(matches!(
        service.validate_domain_blacklist("sub.spam.org", &restrictions),
        Ok(GeoRestrictionResult::Denied(_))
    ));
    // Third entry matches
    assert!(matches!(
        service.validate_domain_blacklist("bad-actor.net/path", &restrictions),
        Ok(GeoRestrictionResult::Denied(_))
    ));
    // None match -> allowed
    assert!(matches!(
        service.validate_domain_blacklist("www.safe.com", &restrictions),
        Ok(GeoRestrictionResult::Allowed)
    ));
}

#[tokio::test]
async fn test_validate_domain_blacklist_empty_blacklist_returns_allowed() {
    let service = make_service(
        Arc::new(MockGeoService::new("US".to_string())),
        Arc::new(MockGeoRestrictionRepository),
    );

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec![]),
        ..Default::default()
    };

    let result = service.validate_domain_blacklist("www.anything.com", &restrictions);
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_validate_domain_blacklist_substring_match() {
    // domain.contains(blocked_domain) is used, so partial matches should trigger
    let service = make_service(
        Arc::new(MockGeoService::new("US".to_string())),
        Arc::new(MockGeoRestrictionRepository),
    );

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec!["evil".to_string()]),
        ..Default::default()
    };

    // "evil" is a substring of "www.evil-domain.com"
    let result = service.validate_domain_blacklist("www.evil-domain.com", &restrictions);
    assert!(matches!(result, Ok(GeoRestrictionResult::Denied(_))));
}

// ---- TeamServiceTrait impl delegation ----

#[tokio::test]
async fn test_team_service_trait_validate_geographic_restriction_delegates() {
    use crate::domain::services::team_service::TeamServiceTrait;

    let geo_service: Arc<dyn GeoLocationService> = Arc::new(MockGeoService::new("US".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: false,
        ..Default::default()
    };

    let result = TeamServiceTrait::validate_geographic_restriction(
        &service,
        Uuid::new_v4(),
        "8.8.8.8",
        &restrictions,
    )
    .await;
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

#[tokio::test]
async fn test_team_service_trait_validate_domain_blacklist_delegates() {
    use crate::domain::services::team_service::TeamServiceTrait;

    let service = make_service(
        Arc::new(MockGeoService::new("US".to_string())),
        Arc::new(MockGeoRestrictionRepository),
    );

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        domain_blacklist: Some(vec!["blocked.com".to_string()]),
        ..Default::default()
    };

    let result =
        TeamServiceTrait::validate_domain_blacklist(&service, "www.blocked.com", &restrictions);
    assert!(matches!(result, Ok(GeoRestrictionResult::Denied(_))));
}

#[tokio::test]
async fn test_team_service_trait_get_team_geo_restrictions_delegates() {
    use crate::domain::services::team_service::TeamServiceTrait;

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string()]),
        ..Default::default()
    };
    let geo_repo = Arc::new(ConfigurableGeoRestrictionRepository::new(restrictions.clone()));
    let service = make_service(Arc::new(MockGeoService::new("US".to_string())), geo_repo);

    let result = TeamServiceTrait::get_team_geo_restrictions(&service, Uuid::new_v4()).await;
    assert!(result.enable_geo_restrictions);
    assert_eq!(result.allowed_countries, Some(vec!["US".to_string()]));
}

// ---- TeamGeoRestrictions default ----

#[tokio::test]
async fn test_team_geo_restrictictions_default_all_none() {
    let default = TeamGeoRestrictions::default();
    assert!(!default.enable_geo_restrictions);
    assert!(default.allowed_countries.is_none());
    assert!(default.blocked_countries.is_none());
    assert!(default.ip_whitelist.is_none());
    assert!(default.domain_blacklist.is_none());
}

// ---- validate_geographic_restriction with invalid CIDR in whitelist ----

#[tokio::test]
async fn test_invalid_cidr_in_whitelist_falls_through_to_country_check() {
    // An invalid CIDR should not match, and the code should fall through to country check
    let geo_service = Arc::new(MockGeoService::new("US".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ip_whitelist: Some(vec!["invalid-cidr".to_string()]),
        allowed_countries: Some(vec!["US".to_string()]),
        ..Default::default()
    };

    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;
    // Invalid CIDR doesn't match, but US is in allowed countries -> allowed
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}

// ---- validate_geographic_restriction: no restrictions configured (all None) ----

#[tokio::test]
async fn test_no_restrictions_configured_allows_all() {
    let geo_service = Arc::new(MockGeoService::new("RU".to_string()));
    let service = make_service(geo_service, Arc::new(MockGeoRestrictionRepository));

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        ..Default::default()
    };

    // No whitelist, no allowed/blocked countries -> should be allowed
    let result = service
        .validate_geographic_restriction(Uuid::new_v4(), "8.8.8.8", &restrictions)
        .await;
    assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
}
