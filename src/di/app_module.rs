use shaku::module;

#[cfg(feature = "metrics")]
module! {
    pub AppModule {
        components = [
            crate::di::infrastructure_module::SettingsComponent,
            crate::di::infrastructure_module::HttpClientComponent,
            crate::di::infrastructure_module::DatabasePoolComponent,
            crate::di::infrastructure_module::RedisClientComponent,
            crate::di::infrastructure_module::TaskRepositoryComponent,
            crate::di::infrastructure_module::TasksBacklogRepositoryComponent,
            crate::di::infrastructure_module::WebhookEventRepositoryComponent,
            crate::di::infrastructure_module::CreditsRepositoryComponent,
            crate::di::infrastructure_module::CrawlRepositoryComponent,
            crate::di::infrastructure_module::ScrapeResultRepositoryComponent,
            crate::di::infrastructure_module::WebhookRepositoryComponent,
            crate::di::infrastructure_module::GeoRestrictionRepositoryComponent,
            crate::di::infrastructure_module::StorageRepositoryComponent,
            crate::di::infrastructure_module::AuditLogRepositoryComponent,
            crate::utils::robots::RobotsChecker,
            crate::di::engines_module::EngineRouterComponent,
            crate::di::engines_module::EngineClientComponent,
            crate::di::service_module::TemplateLoaderComponent,
            crate::domain::services::llm_service::LLMService,
            crate::di::service_module::RateLimitingServiceComponent,
            crate::di::service_module::TeamServiceComponent,
            crate::di::service_module::GeoLocationServiceComponent,
            crate::di::service_module::AuditServiceComponent,
            crate::di::infrastructure_module::AuthScopeRepositoryComponent,
            crate::di::service_module::AuthScopeServiceComponent,
            crate::di::service_module::SearchServiceComponent,
            crate::di::search_module::SearchClientComponent,
            crate::di::service_module::TeamSemaphoreComponent,
            crate::infrastructure::observability::metrics::SystemMonitorComponent,
        ],
        providers = [],
    }
}

#[cfg(not(feature = "metrics"))]
module! {
    pub AppModule {
        components = [
            crate::di::infrastructure_module::SettingsComponent,
            crate::di::infrastructure_module::HttpClientComponent,
            crate::di::infrastructure_module::DatabasePoolComponent,
            crate::di::infrastructure_module::RedisClientComponent,
            crate::di::infrastructure_module::TaskRepositoryComponent,
            crate::di::infrastructure_module::TasksBacklogRepositoryComponent,
            crate::di::infrastructure_module::WebhookEventRepositoryComponent,
            crate::di::infrastructure_module::CreditsRepositoryComponent,
            crate::di::infrastructure_module::CrawlRepositoryComponent,
            crate::di::infrastructure_module::ScrapeResultRepositoryComponent,
            crate::di::infrastructure_module::WebhookRepositoryComponent,
            crate::di::infrastructure_module::GeoRestrictionRepositoryComponent,
            crate::di::infrastructure_module::StorageRepositoryComponent,
            crate::di::infrastructure_module::AuditLogRepositoryComponent,
            crate::utils::robots::RobotsChecker,
            crate::di::engines_module::EngineRouterComponent,
            crate::di::engines_module::EngineClientComponent,
            crate::di::service_module::TemplateLoaderComponent,
            crate::domain::services::llm_service::LLMService,
            crate::di::service_module::RateLimitingServiceComponent,
            crate::di::service_module::TeamServiceComponent,
            crate::di::service_module::GeoLocationServiceComponent,
            crate::di::service_module::AuditServiceComponent,
            crate::di::infrastructure_module::AuthScopeRepositoryComponent,
            crate::di::service_module::AuthScopeServiceComponent,
            crate::di::service_module::SearchServiceComponent,
            crate::di::search_module::SearchClientComponent,
            crate::di::service_module::TeamSemaphoreComponent,
        ],
        providers = [],
    }
}
