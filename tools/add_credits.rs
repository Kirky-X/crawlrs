// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Admin CLI: add credits to a team.
//!
//! Usage:
//!   DATABASE_URL=postgres://crawlrs@localhost:5443/crawlrs \
//!     cargo run --bin add_credits -- <team-uuid> <amount> [description]

use std::sync::Arc;

use crawlrs::domain::models::CreditsTransactionType;
use crawlrs::domain::repositories::credits_repository::CreditsRepository;
use crawlrs::infrastructure::database::repositories::credits_repo_impl::CreditsRepositoryImpl;
use dbnexus::{DbConfig, DbPool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .map_err(|_| "DATABASE_URL or TEST_DATABASE_URL must be set")?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "Usage: {} <team-uuid> <amount> [description]",
            args.get(0).map(|s| s.as_str()).unwrap_or("add_credits")
        );
        std::process::exit(1);
    }
    let team_id = uuid::Uuid::parse_str(&args[1])?;
    let amount: i64 = args[2].parse()?;
    let description = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "Admin: manual adjustment".to_string());

    println!("Connecting to database...");
    let cfg = DbConfig {
        url,
        ..Default::default()
    };
    let pool = Arc::new(DbPool::with_config(cfg).await?);

    let repo = CreditsRepositoryImpl::new(pool);

    println!(
        "Adding {} credits to team {} ({})...",
        amount, team_id, description
    );
    // add_credits stored procedure does not return the new balance (returns Ok(0) placeholder);
    // query the actual balance separately to avoid misleading CLI output.
    repo.add_credits(
        team_id,
        amount,
        CreditsTransactionType::ManualAdjustment,
        description,
        None,
    )
    .await?;

    let new_balance = repo.get_balance(team_id).await?;

    println!("Successfully added {} credits to team {}", amount, team_id);
    println!("New balance: {}", new_balance);

    Ok(())
}
