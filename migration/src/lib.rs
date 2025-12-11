pub use sea_orm_migration::prelude::*;

mod m20240101_000001_create_tasks;
mod m20240101_000002_create_crawls;
mod m20240101_000003_create_webhook_events;
mod m20240101_000004_create_teams_and_keys;
mod m20251211_030836_create_indexes;
mod m20251211_040000_create_scrape_results;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_create_tasks::Migration),
            Box::new(m20240101_000002_create_crawls::Migration),
            Box::new(m20240101_000003_create_webhook_events::Migration),
            Box::new(m20240101_000004_create_teams_and_keys::Migration),
            Box::new(m20251211_030836_create_indexes::Migration),
            Box::new(m20251211_040000_create_scrape_results::Migration),
        ]
    }
}
