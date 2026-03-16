// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits Mapper - converts between Credits domain model and database entity

use crate::domain::models::{Credits, CreditsTransaction, CreditsTransactionType};
use crate::infrastructure::database::entities::{credits, credits_transactions};

/// Mapper for converting between Credits domain model and database entity
pub struct CreditsMapper;

impl CreditsMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: credits::Model) -> Credits {
        Credits::with_timestamps(
            entity.id,
            entity.team_id,
            entity.balance,
            entity.created_at.with_timezone(&chrono::Utc),
            entity.updated_at.with_timezone(&chrono::Utc),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &Credits) -> credits::Model {
        credits::Model {
            id: domain.id,
            team_id: domain.team_id,
            balance: domain.balance(),
            created_at: domain.created_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
            updated_at: domain.updated_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<credits::Model>) -> Vec<Credits> {
        entities.into_iter().map(Self::to_domain).collect()
    }
}

/// Mapper for converting between CreditsTransaction domain model and database entity
pub struct CreditsTransactionMapper;

impl CreditsTransactionMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: credits_transactions::Model) -> CreditsTransaction {
        CreditsTransaction::with_timestamp(
            entity.id,
            entity.team_id,
            entity.amount,
            Self::parse_transaction_type(&entity.transaction_type),
            entity.description,
            entity.reference_id,
            entity.created_at.with_timezone(&chrono::Utc),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &CreditsTransaction) -> credits_transactions::Model {
        credits_transactions::Model {
            id: domain.id,
            team_id: domain.team_id,
            amount: domain.amount,
            transaction_type: domain.transaction_type.to_string(),
            description: domain.description.clone(),
            reference_id: domain.reference_id,
            created_at: domain.created_at.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<credits_transactions::Model>) -> Vec<CreditsTransaction> {
        entities.into_iter().map(Self::to_domain).collect()
    }

    /// Parse transaction type from string
    fn parse_transaction_type(s: &str) -> CreditsTransactionType {
        s.parse().unwrap_or(CreditsTransactionType::ManualAdjustment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_credits_mapper_roundtrip() {
        let now = Utc::now();
        let domain = Credits::with_timestamps(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1000,
            now,
            now,
        );

        let entity = CreditsMapper::to_entity(&domain);
        let back_to_domain = CreditsMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.team_id, back_to_domain.team_id);
        assert_eq!(domain.balance(), back_to_domain.balance());
    }

    #[test]
    fn test_transaction_mapper_roundtrip() {
        let now = Utc::now();
        let domain = CreditsTransaction::with_timestamp(
            Uuid::new_v4(),
            Uuid::new_v4(),
            -100,
            CreditsTransactionType::Scrape,
            "Test transaction".to_string(),
            Some(Uuid::new_v4()),
            now,
        );

        let entity = CreditsTransactionMapper::to_entity(&domain);
        let back_to_domain = CreditsTransactionMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.amount, back_to_domain.amount);
        assert_eq!(domain.transaction_type, back_to_domain.transaction_type);
    }
}
