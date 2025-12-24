#[cfg(test)]
mod tests {
    use crate::domain::services::team_service::{
        GeoRestrictionResult, TeamGeoRestrictions, TeamService,
    };
    use crate::infrastructure::geolocation::GeoLocationService;
    use crate::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::Database;
    use std::sync::Arc;

    async fn setup_db() -> Arc<sea_orm::DatabaseConnection> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let db = Arc::new(db);
        Migrator::up(db.as_ref(), None).await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_domain_blacklist() {
        let db = setup_db().await;
        let geo_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db.clone()));
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
