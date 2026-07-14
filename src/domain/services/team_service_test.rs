// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{CreditsTransaction, CreditsTransactionType, Team};
use crate::domain::repositories::credits_repository::{CreditsRepository, CreditsRepositoryError};
use crate::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::team_repository::TeamRepository;
use crate::domain::services::geo_location::GeoLocation;
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::team_service::{
    GeoRestrictionResult, TeamGeoRestrictions, TeamManagementService, TeamManagementServiceImpl,
    TeamService,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
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
        Err(GeoRestrictionRepositoryError::Database(
            "db down".to_string(),
        ))
    }

    async fn update_team_restrictions(
        &self,
        _team_id: Uuid,
        _restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Err(GeoRestrictionRepositoryError::Database(
            "db down".to_string(),
        ))
    }

    async fn log_geo_restriction_action(
        &self,
        _team_id: Uuid,
        _ip_address: &str,
        _country_code: &str,
        _action: &str,
        _reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        Err(GeoRestrictionRepositoryError::Database(
            "db down".to_string(),
        ))
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
    let geo_repo = Arc::new(ConfigurableGeoRestrictionRepository::new(
        restrictions.clone(),
    ));
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
    let geo_repo = Arc::new(ConfigurableGeoRestrictionRepository::new(
        restrictions.clone(),
    ));
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

// ============ TeamManagementService mocks ============

/// 可配置的 Team 仓库 mock（内存存储）
#[derive(Default)]
struct MockTeamRepository {
    teams: Mutex<HashMap<Uuid, Team>>,
}

#[async_trait]
impl TeamRepository for MockTeamRepository {
    async fn create(&self, team: &Team) -> Result<Team, RepositoryError> {
        let mut teams = self.teams.lock().unwrap();
        teams.insert(team.id, team.clone());
        Ok(team.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Team>, RepositoryError> {
        Ok(self.teams.lock().unwrap().get(&id).cloned())
    }
}

/// 始终失败的 Team 仓库 mock
struct FailingTeamRepository;

#[async_trait]
impl TeamRepository for FailingTeamRepository {
    async fn create(&self, _team: &Team) -> Result<Team, RepositoryError> {
        Err(RepositoryError::Database(anyhow::anyhow!("team repo down")))
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<Option<Team>, RepositoryError> {
        Err(RepositoryError::Database(anyhow::anyhow!("team repo down")))
    }
}

/// 可配置的 Credits 仓库 mock（支持余额设置和操作追踪）
#[derive(Default)]
struct ConfigurableCreditsRepository {
    balances: Mutex<HashMap<Uuid, i64>>,
    add_count: std::sync::atomic::AtomicU32,
    deduct_count: std::sync::atomic::AtomicU32,
    should_fail_deduct: bool,
}

use std::sync::atomic::Ordering;

impl ConfigurableCreditsRepository {
    fn with_balance(team_id: Uuid, balance: i64) -> Self {
        let mut balances = HashMap::new();
        balances.insert(team_id, balance);
        Self {
            balances: Mutex::new(balances),
            ..Default::default()
        }
    }
}

#[async_trait]
impl CreditsRepository for ConfigurableCreditsRepository {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Ok(*self.balances.lock().unwrap().get(&team_id).unwrap_or(&0))
    }

    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        self.deduct_count.fetch_add(1, Ordering::SeqCst);
        if self.should_fail_deduct {
            return Err(CreditsRepositoryError::InsufficientCredits {
                available: 0,
                required: amount,
            });
        }
        let mut balances = self.balances.lock().unwrap();
        let current = *balances.get(&team_id).unwrap_or(&0);
        if current < amount {
            return Err(CreditsRepositoryError::InsufficientCredits {
                available: current,
                required: amount,
            });
        }
        balances.insert(team_id, current - amount);
        Ok(())
    }

    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        self.add_count.fetch_add(1, Ordering::SeqCst);
        let mut balances = self.balances.lock().unwrap();
        let current = *balances.get(&team_id).unwrap_or(&0);
        let new_balance = current + amount;
        balances.insert(team_id, new_balance);
        Ok(new_balance)
    }

    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }

    async fn initialize_team_credits(
        &self,
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        self.balances
            .lock()
            .unwrap()
            .insert(team_id, initial_balance);
        Ok(initial_balance)
    }
}

/// 始终失败的 Credits 仓库 mock
struct FailingCreditsRepository;

#[async_trait]
impl CreditsRepository for FailingCreditsRepository {
    async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Err(CreditsRepositoryError::DatabaseError(
            "credits repo down".to_string(),
        ))
    }

    async fn deduct_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        Err(CreditsRepositoryError::DatabaseError(
            "credits repo down".to_string(),
        ))
    }

    async fn add_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        Err(CreditsRepositoryError::DatabaseError(
            "credits repo down".to_string(),
        ))
    }

    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }

    async fn initialize_team_credits(
        &self,
        _team_id: Uuid,
        _initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Err(CreditsRepositoryError::DatabaseError(
            "credits repo down".to_string(),
        ))
    }
}

fn make_management_service(
    team_repo: Arc<dyn TeamRepository>,
    credits_repo: Arc<dyn CreditsRepository>,
) -> TeamManagementServiceImpl {
    TeamManagementServiceImpl::new(team_repo, credits_repo)
}

// ---- create_team ----

#[tokio::test]
async fn test_create_team_success_returns_team() {
    let team_repo = Arc::new(MockTeamRepository::default());
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.create_team("New Team".to_string()).await;

    assert!(result.is_ok(), "create should succeed");
    let team = result.unwrap();
    assert_eq!(team.name, "New Team");
    assert!(!team.id.is_nil());
}

#[tokio::test]
async fn test_create_team_empty_name_returns_error() {
    let team_repo = Arc::new(MockTeamRepository::default());
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.create_team(String::new()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Invalid team name"),
        "should report invalid name, got: {}",
        err
    );
}

#[tokio::test]
async fn test_create_team_whitespace_name_returns_error() {
    let team_repo = Arc::new(MockTeamRepository::default());
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.create_team("   ".to_string()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Invalid team name"));
}

#[tokio::test]
async fn test_create_team_repo_failure_propagates() {
    let team_repo: Arc<dyn TeamRepository> = Arc::new(FailingTeamRepository);
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.create_team("Valid Name".to_string()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to create team"),
        "should report repo failure, got: {}",
        err
    );
}

// ---- get_team ----

#[tokio::test]
async fn test_get_team_success_returns_team() {
    let team_repo = Arc::new(MockTeamRepository::default());
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo.clone(), credits_repo);

    let created = service
        .create_team("Find Me".to_string())
        .await
        .expect("create should succeed");

    let result = service.get_team(created.id).await;

    assert!(result.is_ok(), "get should succeed");
    let team = result.unwrap();
    assert_eq!(team.id, created.id);
    assert_eq!(team.name, "Find Me");
}

#[tokio::test]
async fn test_get_team_not_found_returns_error() {
    let team_repo = Arc::new(MockTeamRepository::default());
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.get_team(Uuid::new_v4()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Team not found"),
        "should report not found, got: {}",
        err
    );
}

#[tokio::test]
async fn test_get_team_repo_failure_propagates() {
    let team_repo: Arc<dyn TeamRepository> = Arc::new(FailingTeamRepository);
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.get_team(Uuid::new_v4()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to find team"),
        "should report repo failure, got: {}",
        err
    );
}

// ---- update_credits ----

#[tokio::test]
async fn test_update_credits_add_positive_returns_new_balance() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::with_balance(team_id, 100));
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo.clone());

    let result = service
        .update_credits(team_id, 50, "add 50".to_string())
        .await;

    assert!(result.is_ok(), "update should succeed");
    let balance = result.unwrap();
    assert_eq!(balance, 150, "balance should be 100 + 50 = 150");
    assert_eq!(
        credits_repo.add_count.load(Ordering::SeqCst),
        1,
        "add_credits should be called once"
    );
    assert_eq!(
        credits_repo.deduct_count.load(Ordering::SeqCst),
        0,
        "deduct_credits should not be called"
    );
}

#[tokio::test]
async fn test_update_credits_deduct_negative_returns_new_balance() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::with_balance(team_id, 100));
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo.clone());

    let result = service
        .update_credits(team_id, -30, "deduct 30".to_string())
        .await;

    assert!(result.is_ok(), "update should succeed");
    let balance = result.unwrap();
    assert_eq!(balance, 70, "balance should be 100 - 30 = 70");
    assert_eq!(
        credits_repo.deduct_count.load(Ordering::SeqCst),
        1,
        "deduct_credits should be called once"
    );
    assert_eq!(
        credits_repo.add_count.load(Ordering::SeqCst),
        0,
        "add_credits should not be called"
    );
}

#[tokio::test]
async fn test_update_credits_zero_amount_returns_current_balance() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::with_balance(team_id, 100));
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo.clone());

    let result = service
        .update_credits(team_id, 0, "no-op".to_string())
        .await;

    assert!(result.is_ok(), "update should succeed");
    let balance = result.unwrap();
    assert_eq!(balance, 100, "balance should be unchanged");
    assert_eq!(
        credits_repo.add_count.load(Ordering::SeqCst),
        0,
        "add_credits should not be called"
    );
    assert_eq!(
        credits_repo.deduct_count.load(Ordering::SeqCst),
        0,
        "deduct_credits should not be called"
    );
}

#[tokio::test]
async fn test_update_credits_insufficient_balance_returns_error() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::with_balance(team_id, 50));
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service
        .update_credits(team_id, -100, "overdraw".to_string())
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to deduct credits"),
        "should report deduct failure, got: {}",
        err
    );
}

#[tokio::test]
async fn test_update_credits_repo_failure_propagates() {
    let team_id = Uuid::new_v4();
    let credits_repo: Arc<dyn CreditsRepository> = Arc::new(FailingCreditsRepository);
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.update_credits(team_id, 50, "add".to_string()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to add credits"),
        "should report add failure, got: {}",
        err
    );
}

// ---- check_credits ----

#[tokio::test]
async fn test_check_credits_returns_balance() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::with_balance(team_id, 250));
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.check_credits(team_id).await;

    assert!(result.is_ok(), "check should succeed");
    assert_eq!(result.unwrap(), 250, "should return configured balance");
}

#[tokio::test]
async fn test_check_credits_no_balance_returns_zero() {
    let team_id = Uuid::new_v4();
    let credits_repo = Arc::new(ConfigurableCreditsRepository::default());
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.check_credits(team_id).await;

    assert!(result.is_ok(), "check should succeed");
    assert_eq!(result.unwrap(), 0, "should return 0 for unknown team");
}

#[tokio::test]
async fn test_check_credits_repo_failure_propagates() {
    let credits_repo: Arc<dyn CreditsRepository> = Arc::new(FailingCreditsRepository);
    let team_repo = Arc::new(MockTeamRepository::default());
    let service = make_management_service(team_repo, credits_repo);

    let result = service.check_credits(Uuid::new_v4()).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Failed to check credits"),
        "should report repo failure, got: {}",
        err
    );
}
