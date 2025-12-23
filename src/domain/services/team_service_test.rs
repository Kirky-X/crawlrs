#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::geo_restriction_repository::{GeoRestrictionRepository, GeoRestrictionRepositoryError};
    use crate::infrastructure::geolocation::GeoLocationService;
    use crate::domain::services::team_service::{TeamService, TeamGeoRestrictions, GeoRestrictionResult};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use uuid::Uuid;

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

    #[test]
    fn test_domain_blacklist() {
        let geo_repo = Arc::new(MockGeoRestrictionRepository);
        let geo_service = GeoLocationService::new(); // Assuming default constructor
        let service = TeamService::new(geo_service, geo_repo);

        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            domain_blacklist: Some(vec!["example.com".to_string()]),
            ..Default::default()
        };

        // Blocked domain
        let result = service.validate_domain_blacklist("www.example.com", &restrictions);
        assert!(matches!(result, Ok(GeoRestrictionResult::Denied(_))));

        // Allowed domain
        let result = service.validate_domain_blacklist("www.google.com", &restrictions);
        assert!(matches!(result, Ok(GeoRestrictionResult::Allowed)));
    }
}
