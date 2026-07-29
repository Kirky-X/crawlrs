// 临时工具：生成 admin API Key 用于 E2E 测试
// 用法：DATABASE_URL=... CRAWLRS__AUTH__JWT_SECRET=... cargo run --bin gen_admin_key

use std::sync::Arc;
use chrono::Utc;
use crawlrs::bootstrap::services::init_garrison_auth;
use crawlrs::common::time_utils;
use crawlrs::infrastructure::auth::get_garrison_dao;
use crawlrs::infrastructure::database::entities::api_key::{ActiveModel, Entity};
use crawlrs::infrastructure::database::entities::team::{ActiveModel as TeamActiveModel, Entity as TeamEntity};
use crawlrs::bootstrap::config::load_settings;
use dbnexus::{DbConfig, DbPool};
use sea_orm::{ActiveValue, EntityTrait};
use uuid::Uuid;

const GARRISON_NAMESPACE: &str = "crawlrs";
const DEFAULT_EXPIRES_IN_SECS: i64 = 30 * 24 * 60 * 60;
const PERM_ADMIN: &str = "crawlrs:admin";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .map_err(|_| "DATABASE_URL or TEST_DATABASE_URL must be set")?;

    println!("Connecting to database...");
    let cfg = DbConfig { url, ..Default::default() };
    let pool = Arc::new(DbPool::with_config(cfg).await?);

    println!("Loading settings...");
    let settings = Arc::new(load_settings()?);

    println!("Initializing garrison auth...");
    init_garrison_auth(&settings, pool.clone()).await?;

    // 创建 team
    let team_id = Uuid::new_v4();
    let session = pool.get_session("admin").await?;
    let conn = session.connection()?;
    let now = time_utils::to_db_datetime(Utc::now());
    let team_active = TeamActiveModel {
        id: ActiveValue::Set(team_id),
        name: ActiveValue::Set("e2e-test-team".to_string()),
        allowed_countries: ActiveValue::Set(None),
        blocked_countries: ActiveValue::Set(None),
        ip_whitelist: ActiveValue::Set(None),
        domain_blacklist: ActiveValue::Set(None),
        enable_geo_restrictions: ActiveValue::Set(false),
        created_at: ActiveValue::Set(now),
        updated_at: ActiveValue::Set(now),
    };
    TeamEntity::insert(team_active).exec(conn).await?;
    println!("Created team: {}", team_id);

    // 生成 API key
    let api_key_id = Uuid::new_v4();
    let dao = get_garrison_dao().expect("GARRISON_DAO must be injected");
    let handler = garrison::protocol::apikey::ApiKeyHandler::new(dao);
    let plaintext_key = handler
        .generate_with_namespace(
            api_key_id.to_string(),
            GARRISON_NAMESPACE,
            vec![PERM_ADMIN.to_string()],
            DEFAULT_EXPIRES_IN_SECS,
        )
        .await?;

    let garrison_key_id = plaintext_key
        .split_once('.')
        .map(|(k_id, _)| k_id.to_string())
        .ok_or("garrison returned malformed key")?;

    #[allow(deprecated)]
    let api_key_active = ActiveModel {
        id: ActiveValue::Set(api_key_id),
        team_id: ActiveValue::Set(team_id),
        key: ActiveValue::Set(garrison_key_id),
        key_hash: ActiveValue::Set(None),
        created_at: ActiveValue::Set(now),
        updated_at: ActiveValue::Set(None),
    };
    Entity::insert(api_key_active).exec(conn).await?;

    println!("\n=== E2E Test API Key ===");
    println!("API_KEY={}", plaintext_key);
    println!("TEAM_ID={}", team_id);
    println!("API_KEY_ID={}", api_key_id);
    Ok(())
}
