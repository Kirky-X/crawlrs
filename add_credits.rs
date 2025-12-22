use crawlrs::infrastructure::database::connection;
use crawlrs::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crawlrs::domain::repositories::credits_repository::{CreditsRepository, CreditsTransactionType};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    println!("Adding credits to default team...");
    
    // Get database connection
    let db = connection::get_db_connection().await?;
    let db = Arc::new(db);
    
    // Create credits repository
    let credits_repo = CreditsRepositoryImpl::new(db);
    
    // Default team ID (from the database)
    let default_team_id = Uuid::parse_str("00000000-0000-0000-0000-000000000000")?;
    
    // Add 100 credits to the default team
    let new_balance = credits_repo.add_credits(
        default_team_id,
        100,
        CreditsTransactionType::ManualAdjustment,
        "Admin: Adding credits for testing".to_string(),
        None,
    ).await?;
    
    println!("Successfully added 100 credits to team {}", default_team_id);
    println!("New balance: {}", new_balance);
    
    Ok(())
}